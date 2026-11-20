//! Cross-provider Codex history merge.
//!
//! ## Problem
//! Codex tags every conversation with a `model_provider` in TWO places:
//!   1. `state_*.sqlite` → `threads.model_provider` (drives the left-panel
//!      / `/resume` list), and
//!   2. each rollout transcript's first JSONL line
//!      (`session_meta.payload.model_provider`, read on resume).
//!
//! Older Codex clients hide sessions whose provider differs from the one
//! active. A user who has talked to Codex under several provider ids —
//! the official ChatGPT login (`openai`, lowercase), our third-party block
//! (`OpenAI`), a `gemini` config, … — only ever sees the slice matching
//! whatever provider is launched. The rest looks "lost".
//!
//! ## Fix
//! Right before EchoBird launches Codex, retag every prior session to the
//! provider Codex is ABOUT to launch with — read from the top-level
//! `model_provider` in `config.toml`, NOT hardcoded. Reading it live means
//! we always retag to the *active* provider (canonical `OpenAI`, or
//! whatever relay-mode wrote), so the merge can never tag sessions to a
//! provider that is then filtered out.
//!
//! ## Safety
//! Provider retagging must preserve conversation visibility and recovery.
//!   * Retag ONLY to the currently-active provider — never an arbitrary
//!     target. The provider filter hides sessions retagged to a non-active
//!     id (e.g. the official lowercase
//!     `openai`); reading the live config makes that impossible.
//!   * Touch ONLY the two provider-tag locations. We never write
//!     `config.toml`, `workspace_roots`, or anything else — that TOML
//!     mangling can corrupt unrelated configuration.
//!   * Back up each state DB (consistent `VACUUM INTO` snapshot, keep last
//!     N) BEFORE mutating, and skip the retag entirely if the backup fails.
//!   * Idempotent (`WHERE model_provider IS NOT ?`): re-running on every
//!     launch is a cheap self-heal.
//!   * Save the original rollout metadata before rewriting. Coordinate with
//!     Codex's writer locks and exclude active threads from both stores.
//!     Keep mtime/length checks as a fallback for older clients without locks.
//!   * Never fatal: a locked DB (Codex still running) or any error is
//!     logged and skipped; the next launch self-heals.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Rollouts modified within this window are assumed to belong to the live
/// session Codex may be appending to right now — skip them.
const ACTIVE_ROLLOUT_SKIP_SECS: u64 = 120;
/// How many timestamped pre-merge DB backups to retain per state DB.
const KEEP_BACKUPS: usize = 5;
/// Filename infix that marks our backups (kept out of the retag scan).
const BACKUP_MARKER: &str = ".eb-merge-bak-";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MergeReport {
    pub dbs_seen: usize,
    pub threads_retagged: usize,
    pub rollouts_retagged: usize,
    pub dbs_locked: usize,
    pub failures: usize,
    pub sessions_in_use: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMergeNotice {
    Partial,
    Deferred,
}

impl MergeReport {
    fn notice(&self) -> Option<HistoryMergeNotice> {
        if self.failures == 0 && self.dbs_locked == 0 && self.sessions_in_use == 0 {
            return None;
        }
        Some(if self.threads_retagged > 0 || self.rollouts_retagged > 0 {
            HistoryMergeNotice::Partial
        } else {
            HistoryMergeNotice::Deferred
        })
    }
}

struct HistoryCoordinationLock(File);

impl Drop for HistoryCoordinationLock {
    fn drop(&mut self) {
        // Closing one handle may leave a lock held by a clone or forked child.
        if let Err(e) = fs2::FileExt::unlock(&self.0) {
            log::warn!("[CodexMerge] could not release writer coordination: {e}");
        }
    }
}

/// Public entry — called from the Codex launch pre-flight in
/// `process_manager::start_tool`, after `config.toml` is written
/// and before Codex spawns. A notice lets the client report pending work
/// without preventing launch.
pub fn merge_codex_history(codex_home: &Path) -> Option<HistoryMergeNotice> {
    let active = match read_active_provider(codex_home) {
        Ok(Some(active)) => active,
        Ok(None) => return None,
        Err(error) => {
            log::warn!("[CodexMerge] cannot read the active provider; retry next launch: {error}");
            return Some(HistoryMergeNotice::Deferred);
        }
    };
    let report = retag_all(codex_home, &active);
    if report.threads_retagged > 0 || report.rollouts_retagged > 0 || report.dbs_locked > 0 {
        log::info!(
            "[CodexMerge] merged history under {active:?}: {} threads, {} rollouts \
             ({} dbs seen, {} locked-skip)",
            report.threads_retagged,
            report.rollouts_retagged,
            report.dbs_seen,
            report.dbs_locked
        );
    }
    report.notice()
}

/// Retag both stores to `active`. Split from the public entry for tests.
fn retag_all(codex_home: &Path, active: &str) -> MergeReport {
    let mut report = MergeReport::default();
    let (_coordination, mut skipped) = match lock_history_writers(codex_home) {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("[CodexMerge] writer coordination unavailable; retry next launch: {e}");
            report.failures += 1;
            return report;
        }
    };
    let mut files = Vec::new();
    let databases = match collect_jsonl(&codex_home.join("sessions"), 4, &mut files)
        .and_then(|()| collect_jsonl(&codex_home.join("archived_sessions"), 1, &mut files))
        .and_then(|()| find_state_dbs(codex_home))
    {
        Ok(databases) => databases,
        Err(error) => {
            log::warn!("[CodexMerge] history discovery failed; retry next launch: {error}");
            report.failures += 1;
            return report;
        }
    };
    // Old clients do not take writer locks. Also leave their fresh DB rows
    // alone instead of retagging the index while skipping the transcript.
    for path in &files {
        if is_live_session(path) {
            let header = match rollout_header(path) {
                Ok(header) => header,
                Err(error) => {
                    log::warn!("[CodexMerge] cannot read an active rollout {path:?}: {error}");
                    report.failures += 1;
                    return report;
                }
            };
            if let Some(id) = header
                .as_ref()
                .and_then(|header| header.pointer("/payload/id"))
                .and_then(serde_json::Value::as_str)
            {
                skipped.insert(id.to_owned());
            }
        }
    }
    for db in databases {
        report.dbs_seen += 1;
        match retag_db(&db, active, &skipped) {
            Ok(Some(n)) => report.threads_retagged += n,
            Ok(None) => {
                report.dbs_locked += 1;
                return report;
            }
            Err(e) => {
                log::warn!("[CodexMerge] state db {db:?}; retry next launch: {e}");
                report.failures += 1;
                return report;
            }
        }
    }
    retag_rollouts(&files, active, &skipped, &mut report);
    report
}

/// Codex 0.154's thread-store/local/writer_lock.rs holds the coordination
/// lock while acquiring/removing per-thread locks. Holding the same OS lock
/// here prevents new writers until migration ends; already-active writers
/// keep their own locks and are excluded. fs2 uses flock/LockFileEx, as does
/// Codex's std::fs::File locking, without raising our Rust MSRV.
fn lock_history_writers(
    codex_home: &Path,
) -> io::Result<(HistoryCoordinationLock, HashSet<String>)> {
    use fs2::FileExt;
    let dir = codex_home.join("thread-writer-locks");
    std::fs::create_dir_all(&dir)?;
    let coordination = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".coordination.lock"))?;
    coordination.try_lock_exclusive()?;
    let coordination = HistoryCoordinationLock(coordination);
    let mut active = HashSet::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("lock")
            || uuid::Uuid::parse_str(id).is_err()
        {
            continue;
        }
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        match file.try_lock_exclusive() {
            Ok(()) => fs2::FileExt::unlock(&file)?,
            Err(e) if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                active.insert(id.to_string());
            }
            Err(e) => return Err(e),
        }
    }
    Ok((coordination, active))
}

fn rollout_header(path: &Path) -> io::Result<Option<serde_json::Value>> {
    use std::io::BufRead;
    let mut first = String::new();
    io::BufReader::new(File::open(path)?).read_line(&mut first)?;
    Ok(serde_json::from_str::<serde_json::Value>(&first)
        .ok()
        .filter(|value| {
            value.get("type").and_then(serde_json::Value::as_str) == Some("session_meta")
        }))
}

// ─── config.toml: the active provider ────────────────────────────────────

fn read_active_provider(codex_home: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(codex_home.join("config.toml")) {
        Ok(text) => active_provider_id(&text)
            .map_err(|_| io::Error::other("Codex configuration is not valid TOML")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn active_provider_id(config_text: &str) -> Result<Option<String>, toml_edit::TomlError> {
    let document = config_text.parse::<toml_edit::DocumentMut>()?;
    Ok(document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned))
}

// ─── state_*.sqlite: threads.model_provider ─────────────────────────────

/// Codex moved its sqlite store from the home root into a `sqlite/` subdir
/// (cli 0.133 → 0.140) and bumps the generation suffix on schema resets
/// (… → `state_5` → eventually `state_6`). Scan both locations and glob
/// `state_*.sqlite` so we retag whichever the running Codex actually reads.
fn find_state_dbs(codex_home: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for dir in [codex_home.to_path_buf(), codex_home.join("sqlite")] {
        for entry in read_dir_if_present(&dir)?.into_iter().flatten() {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with("state_")
                && name.ends_with(".sqlite")
                && !name.contains(BACKUP_MARKER)
            {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// Retag every thread in one state DB to `active`.
/// `Ok(Some(n))` = success, n rows changed. `Ok(None)` = DB busy/locked
/// (Codex running) — skipped, next launch self-heals. `Err` = real error.
fn retag_db(
    db_path: &Path,
    active: &str,
    skipped: &HashSet<String>,
) -> Result<Option<usize>, String> {
    use rusqlite::{Connection, OpenFlags};

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open: {e}"))?;
    let _ = conn.busy_timeout(Duration::from_millis(3000));

    // No `threads.model_provider` column → nothing to do (alien schema).
    let has_col: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('threads') WHERE name = 'model_provider'",
            (),
            |r| r.get(0),
        )
        .map_err(|e| format!("schema: {e}"))?;
    if has_col == 0 {
        return Ok(Some(0));
    }

    conn.execute_batch("CREATE TEMP TABLE eb_merge_skipped (id TEXT PRIMARY KEY)")
        .map_err(|e| format!("skip table: {e}"))?;
    for id in skipped {
        conn.execute("INSERT INTO eb_merge_skipped VALUES (?1)", (id,))
            .map_err(|e| format!("skip thread: {e}"))?;
    }

    // How many rows actually need retagging? (Idempotent: usually 0.)
    let pending: i64 = match conn.query_row(
        "SELECT count(*) FROM threads WHERE model_provider IS NOT ?1
         AND id NOT IN (SELECT id FROM eb_merge_skipped)",
        (active,),
        |r| r.get(0),
    ) {
        Ok(n) => n,
        Err(e) if is_busy(&e) => return Ok(None),
        Err(e) => return Err(format!("count: {e}")),
    };
    if pending == 0 {
        return Ok(Some(0));
    }

    // Back up (consistent snapshot) BEFORE mutating; never rewrite
    // providers without a restore point.
    match backup_state_db(&conn, db_path) {
        BackupOutcome::Ok => prune_backups(db_path),
        BackupOutcome::Locked => return Ok(None),
        BackupOutcome::Failed(e) => {
            return Err(format!("backup failed: {e}"));
        }
    }

    match conn.execute(
        "UPDATE threads SET model_provider = ?1 WHERE model_provider IS NOT ?1
         AND id NOT IN (SELECT id FROM eb_merge_skipped)",
        (active,),
    ) {
        Ok(n) => Ok(Some(n)),
        Err(e) if is_busy(&e) => Ok(None),
        Err(e) => Err(format!("update: {e}")),
    }
}

enum BackupOutcome {
    Ok,
    Locked,
    Failed(String),
}

/// `VACUUM INTO` a consistent pre-merge snapshot next to the DB. The
/// snapshot name lacks a `.sqlite` suffix so `find_state_dbs` never
/// re-scans it.
fn backup_state_db(conn: &rusqlite::Connection, db_path: &Path) -> BackupOutcome {
    let bak = backup_path(db_path);
    // Single-quote the path for the SQL string literal (sqlite does no
    // backslash processing, so Windows paths are safe once quotes are
    // doubled).
    let escaped = bak.to_string_lossy().replace('\'', "''");
    match conn.execute_batch(&format!("VACUUM INTO '{escaped}'")) {
        Ok(()) => BackupOutcome::Ok,
        Err(e) if is_busy(&e) => BackupOutcome::Locked,
        Err(e) => BackupOutcome::Failed(e.to_string()),
    }
}

fn backup_path(db_path: &Path) -> PathBuf {
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S-%f");
    let name = db_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state.sqlite".to_string());
    db_path.with_file_name(format!(
        "{name}{BACKUP_MARKER}{ts}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn prune_backups(db_path: &Path) {
    let Some(dir) = db_path.parent() else {
        return;
    };
    let Some(name) = db_path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let prefix = format!("{name}{BACKUP_MARKER}");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut baks: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    baks.sort(); // timestamp suffix sorts chronologically
    while baks.len() > KEEP_BACKUPS {
        let _ = std::fs::remove_file(baks.remove(0));
    }
}

// ─── rollout JSONL: session_meta.model_provider ─────────────────────────

fn retag_rollouts(
    files: &[PathBuf],
    active: &str,
    skipped: &HashSet<String>,
    report: &mut MergeReport,
) {
    for file in files {
        let header = match rollout_header(file) {
            Ok(header) => header,
            Err(error) => {
                report.failures += 1;
                log::warn!("[CodexMerge] cannot read rollout {file:?}: {error}");
                continue;
            }
        };
        let id = header
            .as_ref()
            .and_then(|header| header.pointer("/payload/id"))
            .and_then(serde_json::Value::as_str);
        if is_live_session(file) || id.is_some_and(|id| skipped.contains(id)) {
            if header
                .as_ref()
                .and_then(|header| header.pointer("/payload/model_provider"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|provider| provider != active)
            {
                report.sessions_in_use += 1;
            }
            continue;
        }
        match rewrite_rollout_meta(file, active) {
            Ok(true) => report.rollouts_retagged += 1,
            Ok(false) => {}
            Err(e) => {
                report.failures += 1;
                log::warn!("[CodexMerge] rollout {file:?}: {e}");
            }
        }
    }
}

fn read_dir_if_present(path: &Path) -> io::Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn collect_jsonl(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if depth == 0 {
        return Ok(());
    }
    for entry in read_dir_if_present(dir)?.into_iter().flatten() {
        let path = entry?.path();
        if path.is_dir() {
            collect_jsonl(&path, depth - 1, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

/// True if the rollout was written so recently that Codex may still be
/// appending to it. On any mtime uncertainty we err toward "live" (skip).
fn is_live_session(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    meta.modified()
        .map(|m| {
            m.elapsed()
                .map(|d| d.as_secs() < ACTIVE_ROLLOUT_SKIP_SECS)
                .unwrap_or(true)
        })
        .unwrap_or(true)
}

/// Rewrite the first-line `session_meta.model_provider` to `active`,
/// preserving every other field and streaming the transcript body
/// verbatim. Returns Ok(true) if rewritten, Ok(false) if already correct /
/// not a session_meta / no provider field.
fn rewrite_rollout_meta(path: &Path, active: &str) -> std::io::Result<bool> {
    use std::io::{BufRead, Write};

    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let before = reader.get_ref().metadata()?;
    let mut first = String::new();
    if reader.read_line(&mut first)? == 0 {
        return Ok(false); // empty file
    }

    let trimmed = first.trim_end_matches(['\n', '\r']);
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Ok(false);
    };
    if value.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
        return Ok(false);
    }
    let Some(payload) = value.get_mut("payload").and_then(|p| p.as_object_mut()) else {
        return Ok(false);
    };
    // Only retag an existing, differing provider — never invent the field
    // where Codex didn't write one.
    match payload.get("model_provider").and_then(|v| v.as_str()) {
        Some(p) if p == active => return Ok(false),
        Some(_) => {}
        None => return Ok(false),
    }
    payload.insert(
        "model_provider".to_string(),
        serde_json::Value::String(active.to_string()),
    );
    let new_first = serde_json::to_string(&value).map_err(std::io::Error::other)?;

    // Save only the original first line: the body is copied verbatim and a
    // full-transcript backup on every switch would grow with chat history.
    // Never overwrite this evidence on subsequent provider changes.
    let backup = path.with_extension("jsonl.eb-merge-original-meta");
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
    {
        Ok(mut file) => {
            if let Err(e) = file
                .write_all(first.as_bytes())
                .and_then(|()| file.sync_all())
            {
                drop(file);
                let _ = std::fs::remove_file(&backup);
                return Err(e);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let saved: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&backup)?).map_err(io::Error::other)?;
            if saved.get("type") != value.get("type")
                || saved.pointer("/payload/id") != value.pointer("/payload/id")
                || saved
                    .pointer("/payload/model_provider")
                    .and_then(|v| v.as_str())
                    .is_none()
            {
                return Err(io::Error::other(
                    "rollout metadata backup does not match session",
                ));
            }
        }
        Err(e) => return Err(e),
    }

    // Atomic: new first line + verbatim remainder → temp in the same dir,
    // then rename over the original.
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rollout.jsonl");
    let tmp = dir.join(format!(".{file_name}.{}.eb-tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut out =
            std::io::BufWriter::new(OpenOptions::new().write(true).create_new(true).open(&tmp)?);
        out.write_all(new_first.as_bytes())?;
        out.write_all(b"\n")?;
        std::io::copy(&mut reader, &mut out)?; // lines 2..N, untouched
        out.flush()?;
        out.get_ref().sync_all()?;
        drop(out);
        drop(reader);
        replace_unchanged_rollout(path, &tmp, &before)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok(true)
}

fn replace_unchanged_rollout(
    path: &Path,
    tmp: &Path,
    before: &std::fs::Metadata,
) -> io::Result<()> {
    let after = std::fs::metadata(path)?;
    if after.len() != before.len() || after.modified()? != before.modified()? {
        return Err(io::Error::other(
            "rollout changed during merge; retry next launch",
        ));
    }
    std::fs::rename(tmp, path)
}

// ─── helpers ─────────────────────────────────────────────────────────────

fn is_busy(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::DatabaseBusy
                || err.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tmp_dir(label: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("echobird_merge_{label}_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_state_db(path: &Path, rows: &[(&str, &str)]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT);")
            .unwrap();
        for (id, prov) in rows {
            conn.execute(
                "INSERT INTO threads (id, model_provider) VALUES (?1, ?2)",
                (id, prov),
            )
            .unwrap();
        }
    }

    fn provider_count(path: &Path, provider: &str) -> i64 {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.query_row(
            "SELECT count(*) FROM threads WHERE model_provider = ?1",
            (provider,),
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn active_provider_id_reads_top_level_not_table_name() {
        let cfg = "model_provider = \"OpenAI\"\n\
                   model = \"gpt-5.5\"\n\
                   [model_providers.OpenAI]\n\
                   name = \"OpenAI\"\n";
        assert_eq!(active_provider_id(cfg).unwrap().as_deref(), Some("OpenAI"));
    }

    #[test]
    fn active_provider_id_handles_relay_comment_and_spacing() {
        // relay-mode provider, no spaces, trailing comment
        assert_eq!(
            active_provider_id("model_provider=\"anthropic\"  # relay\n")
                .unwrap()
                .as_deref(),
            Some("anthropic")
        );
        // a key that merely shares the prefix must be ignored
        assert_eq!(
            active_provider_id("model_provider_foo = \"x\"\n").unwrap(),
            None
        );
        // commented-out line ignored
        assert_eq!(
            active_provider_id("# model_provider = \"x\"\n").unwrap(),
            None
        );
        // a `name` inside a table must never win
        assert_eq!(
            active_provider_id("[model_providers.OpenAI]\nname = \"OpenAI\"\n").unwrap(),
            None
        );
        assert_eq!(
            active_provider_id("model_provider = 'native'")
                .unwrap()
                .as_deref(),
            Some("native")
        );
        assert!(active_provider_id("model_provider = \"native\"\n[").is_err());
    }

    #[test]
    fn retag_db_merges_all_providers_and_is_idempotent() {
        let dir = tmp_dir("db");
        let db = dir.join("state_5.sqlite");
        make_state_db(&db, &[("a", "gemini"), ("b", "openai"), ("c", "OpenAI")]);

        // gemini + openai retagged; the already-OpenAI row is left alone.
        assert_eq!(retag_db(&db, "OpenAI", &HashSet::new()).unwrap(), Some(2));
        assert_eq!(provider_count(&db, "OpenAI"), 3);

        // second run is a no-op
        assert_eq!(retag_db(&db, "OpenAI", &HashSet::new()).unwrap(), Some(0));

        // a pre-merge backup was written
        let has_backup = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(BACKUP_MARKER));
        assert!(has_backup, "backup must exist before any mutation");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retag_db_noop_when_threads_table_absent() {
        let dir = tmp_dir("noschema");
        let db = dir.join("state_9.sqlite");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE other (x INTEGER);")
            .unwrap();
        drop(conn);
        assert_eq!(retag_db(&db, "OpenAI", &HashSet::new()).unwrap(), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_state_dbs_scans_both_locations_and_skips_backups() {
        let dir = tmp_dir("find");
        std::fs::create_dir_all(dir.join("sqlite")).unwrap();
        make_state_db(&dir.join("state_5.sqlite"), &[]);
        make_state_db(&dir.join("sqlite").join("state_5.sqlite"), &[]);
        std::fs::write(
            dir.join("state_5.sqlite.eb-merge-bak-20260101-000000"),
            b"x",
        )
        .unwrap();

        let found = find_state_dbs(&dir).unwrap();
        assert_eq!(found.len(), 2, "both state dbs, never the backup");
        assert!(found
            .iter()
            .all(|p| !p.to_string_lossy().contains(BACKUP_MARKER)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrite_rollout_meta_changes_provider_and_keeps_body() {
        let dir = tmp_dir("roll");
        let f = dir.join("rollout-x.jsonl");
        std::fs::write(
            &f,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"gemini\",\"cwd\":\"/w\"}}\n\
             {\"type\":\"user_message\",\"payload\":{\"role\":\"user\"}}\n",
        )
        .unwrap();

        assert!(rewrite_rollout_meta(&f, "OpenAI").unwrap());

        let text = std::fs::read_to_string(&f).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let meta: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(meta["payload"]["model_provider"], "OpenAI");
        assert_eq!(meta["payload"]["id"], "s1"); // sibling fields preserved
        assert_eq!(meta["payload"]["cwd"], "/w");
        // body line streamed byte-for-byte
        assert_eq!(
            lines[1],
            "{\"type\":\"user_message\",\"payload\":{\"role\":\"user\"}}"
        );

        // idempotent
        assert!(!rewrite_rollout_meta(&f, "OpenAI").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrite_rollout_meta_ignores_non_session_meta_and_missing_provider() {
        let dir = tmp_dir("roll2");
        let a = dir.join("rollout-a.jsonl");
        std::fs::write(&a, "{\"type\":\"response_item\",\"payload\":{}}\n").unwrap();
        assert!(!rewrite_rollout_meta(&a, "OpenAI").unwrap());

        let b = dir.join("rollout-b.jsonl");
        std::fs::write(
            &b,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s\"}}\n",
        )
        .unwrap();
        assert!(!rewrite_rollout_meta(&b, "OpenAI").unwrap()); // no provider → leave it
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_session_guard_skips_fresh_file() {
        let dir = tmp_dir("live");
        let f = dir.join("rollout-z.jsonl");
        std::fs::write(&f, "{}\n").unwrap();
        assert!(is_live_session(&f), "just-written file is treated as live");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retag_all_end_to_end_merges_db_and_rollouts() {
        let dir = tmp_dir("e2e");
        make_state_db(
            &dir.join("state_5.sqlite"),
            &[("a", "gemini"), ("b", "OpenAI")],
        );
        // an OLD rollout (mtime backdated implicitly is hard; instead drop it
        // straight through rewrite_rollout_meta in its own test). Here we only
        // assert the DB half plus that no rollout dir is fine.
        let report = retag_all(&dir, "OpenAI");
        assert_eq!(report.dbs_seen, 1);
        assert_eq!(report.threads_retagged, 1);
        assert_eq!(provider_count(&dir.join("state_5.sqlite"), "OpenAI"), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn old_rollout(dir: &Path, id: &str) -> PathBuf {
        let sessions = dir.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join(format!("rollout-{id}.jsonl"));
        std::fs::write(
            &path,
            format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"model_provider\":\"openai\"}}}}\n{{\"type\":\"event_msg\",\"payload\":{{\"text\":\"keep me\"}}}}\n"),
        )
        .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        path
    }

    #[test]
    fn state_db_failure_leaves_rollouts_untouched() {
        let dir = tmp_dir("db_failure");
        std::fs::write(dir.join("state_5.sqlite"), b"invalid sqlite database").unwrap();
        let path = old_rollout(&dir, "s1");
        let before = std::fs::read(&path).unwrap();
        let report = retag_all(&dir, "OpenAI");
        assert_eq!(report.rollouts_retagged, 0);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn state_backup_failure_leaves_both_stores_untouched() {
        let dir = tmp_dir("db_backup_failure");
        // The DB fits, but the backup suffix exceeds SQLite's Windows path
        // limit (or the filename component limit on Unix). No ACL setup needed.
        let padding = if cfg!(windows) {
            230usize.saturating_sub(dir.as_os_str().len())
        } else {
            230
        };
        let db = dir.join(format!("state_{}.sqlite", "a".repeat(padding)));
        make_state_db(&db, &[("s1", "openai")]);
        let path = old_rollout(&dir, "s1");
        let before = std::fs::read(&path).unwrap();
        let report = retag_all(&dir, "OpenAI");
        assert_eq!(report.threads_retagged, 0);
        assert_eq!(report.rollouts_retagged, 0);
        assert_eq!(provider_count(&db, "openai"), 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rollout_original_metadata_survives_repeated_merges() {
        let dir = tmp_dir("original_meta");
        let path = old_rollout(&dir, "s1");
        let original = std::fs::read_to_string(&path).unwrap();
        assert!(rewrite_rollout_meta(&path, "OpenAI").unwrap());
        assert!(rewrite_rollout_meta(&path, "custom").unwrap());
        let backup = path.with_extension("jsonl.eb-merge-original-meta");
        assert_eq!(
            std::fs::read_to_string(backup).unwrap(),
            original.split_inclusive('\n').next().unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn coordination_lock_is_released_with_a_duplicated_handle() {
        let dir = tmp_dir("coordination_release");
        let (coordination, _) = lock_history_writers(&dir).unwrap();
        let inherited = coordination.0.try_clone().unwrap();
        drop(coordination);

        let (next, _) = lock_history_writers(&dir).unwrap();
        drop(next);
        drop(inherited);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn active_writer_is_skipped_in_both_stores_then_merged_after_release() {
        use fs2::FileExt;
        let dir = tmp_dir("active_writer");
        let id = uuid::Uuid::new_v4().to_string();
        let path = old_rollout(&dir, &id);
        let before = std::fs::read(&path).unwrap();
        let db = dir.join("state_5.sqlite");
        make_state_db(&db, &[(&id, "openai"), ("idle", "openai")]);
        old_rollout(&dir, "idle");
        let locks = dir.join("thread-writer-locks");
        std::fs::create_dir_all(&locks).unwrap();
        let writer = std::fs::File::create(locks.join(format!("{id}.lock"))).unwrap();
        writer.try_lock_exclusive().unwrap();

        let report = retag_all(&dir, "OpenAI");
        assert_eq!(report.threads_retagged, 1);
        assert_eq!(report.rollouts_retagged, 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(provider_count(&db, "openai"), 1);

        fs2::FileExt::unlock(&writer).unwrap();
        drop(writer);
        let report = retag_all(&dir, "OpenAI");
        assert_eq!(report.threads_retagged, 1);
        assert_eq!(report.rollouts_retagged, 1);
        assert_eq!(provider_count(&db, "OpenAI"), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rollout_backup_failure_preserves_original() {
        let dir = tmp_dir("backup_failure");
        let path = old_rollout(&dir, "s1");
        let before = std::fs::read(&path).unwrap();
        // A directory at the backup destination forces an I/O failure on all
        // platforms, including Windows where read-only directory bits differ.
        std::fs::create_dir(path.with_extension("jsonl.eb-merge-original-meta")).unwrap();
        assert!(rewrite_rollout_meta(&path, "OpenAI").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn launch_reports_partial_history_and_retries_the_remaining_rollout() {
        let dir = tmp_dir("partial_launch");
        let db = dir.join("state_5.sqlite");
        make_state_db(&db, &[("s1", "openai")]);
        std::fs::write(dir.join("config.toml"), "model_provider = 'custom'").unwrap();
        let path = old_rollout(&dir, "s1");
        let before = std::fs::read_to_string(&path).unwrap();
        let backup = path.with_extension("jsonl.eb-merge-original-meta");
        std::fs::create_dir(&backup).unwrap();

        assert_eq!(merge_codex_history(&dir), Some(HistoryMergeNotice::Partial));
        assert_eq!(provider_count(&db, "custom"), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        std::fs::remove_dir(&backup).unwrap();
        assert_eq!(merge_codex_history(&dir), None);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            rollout_header(&path).unwrap().unwrap()["payload"]["model_provider"],
            "custom"
        );
        assert_eq!(
            before.split_once('\n').unwrap().1,
            after.split_once('\n').unwrap().1
        );
        assert_eq!(merge_codex_history(&dir), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn appended_body_is_not_replaced_with_stale_copy() {
        use std::io::Write;
        let dir = tmp_dir("append_race");
        let path = old_rollout(&dir, "s1");
        let before = std::fs::metadata(&path).unwrap();
        let tmp = dir.join("prepared.eb-tmp");
        std::fs::copy(&path, &tmp).unwrap();
        let appended = b"{\"type\":\"event_msg\",\"payload\":{\"text\":\"new message\"}}\n";
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(appended)
            .unwrap();

        assert!(replace_unchanged_rollout(&path, &tmp, &before).is_err());
        assert!(std::fs::read(&path).unwrap().ends_with(appended));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn busy_coordination_leaves_both_stores_untouched() {
        use fs2::FileExt;
        let dir = tmp_dir("busy_coordination");
        let path = old_rollout(&dir, "s1");
        let before = std::fs::read(&path).unwrap();
        let db = dir.join("state_5.sqlite");
        make_state_db(&db, &[("s1", "openai")]);
        let locks = dir.join("thread-writer-locks");
        std::fs::create_dir_all(&locks).unwrap();
        let guard = File::create(locks.join(".coordination.lock")).unwrap();
        guard.try_lock_exclusive().unwrap();
        assert_eq!(
            retag_all(&dir, "OpenAI").notice(),
            Some(HistoryMergeNotice::Deferred)
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(provider_count(&db, "openai"), 1);
        drop(guard);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fresh_rollout_keeps_index_provider_until_next_launch() {
        let dir = tmp_dir("fresh_index");
        let path = old_rollout(&dir, "s1");
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now())
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let db = dir.join("state_5.sqlite");
        make_state_db(&db, &[("s1", "openai")]);
        let report = retag_all(&dir, "OpenAI");
        assert_eq!(report.threads_retagged, 0);
        assert_eq!(report.rollouts_retagged, 0);
        assert_eq!(report.notice(), Some(HistoryMergeNotice::Deferred));
        assert_eq!(retag_all(&dir, "openai").notice(), None);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(provider_count(&db, "openai"), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
