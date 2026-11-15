//! Claude Desktop Code has account/org-scoped pointers to shared CLI JSONL.
//! Populate missing pointers after exit; never rewrite transcripts or credentials.
mod layout;
mod pointers;
mod transcripts;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) struct SourceSession {
    cli_id: String,
    cwd: String,
    title: String,
    created_at: i64,
    last_activity_at: i64,
    is_archived: bool,
}

/// Explicit apply can defer until the existing restart path has stopped Claude.
/// Passive account loading/navigation never calls this service.
pub(crate) async fn prepare(extra_target: Option<PathBuf>) -> Result<(), String> {
    if super::process_manager::desktop_tool_is_running("claudedesktop")
        .await
        .map_err(|_| "accountError.stopClaudeDesktop")?
    {
        return Err("accountError.stopClaudeDesktop".into());
    }
    let cli_home = super::claude_code_accounts::config_dir()?;
    let cli_home = if cli_home == Path::new("~") {
        dirs::home_dir().ok_or("accountError.home")?
    } else {
        super::tool_manager::expand_path(&cli_home.to_string_lossy())
    };
    let state = dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/claude-desktop-history");
    let mut native = layout::resolve()?;
    log::debug!(
        "[ClaudeHistory] inspecting {} native data roots",
        native.roots.len()
    );
    // Recheck before writing in case Claude was relaunched during discovery.
    if super::process_manager::desktop_tool_is_running("claudedesktop")
        .await
        .map_err(|_| "accountError.stopClaudeDesktop")?
    {
        return Err("accountError.stopClaudeDesktop".into());
    }
    if let Some(root) = native.threep_root.as_ref() {
        ensure_installation_id(root)?;
        native = layout::resolve()?;
    }
    if let Some(target) = extra_target {
        if !native.targets.contains(&target) {
            native.targets.push(target);
        }
    }
    tokio::task::spawn_blocking(move || synchronize(&native.targets, &cli_home, &state))
        .await
        .map_err(|_| "Claude history preparation was interrupted.".to_string())??;
    Ok(())
}

/// Model config is read at the next launch. If it is already stopped, make
/// manual launches work too; otherwise the normal prelaunch hook does the work.
pub(crate) async fn prepare_if_stopped() -> Result<(), String> {
    if !super::process_manager::desktop_tool_is_running("claudedesktop").await? {
        prepare(None).await?;
    }
    Ok(())
}

fn ensure_installation_id(root: &Path) -> Result<(), String> {
    let path = root.join("ant-did");
    if path.exists() {
        return Ok(());
    }
    // Same native K3t/G3t format: base64 UUID, mode0600, exclusive creation.
    // Desktop otherwise generates this identity only after its first launch.
    let encoded = STANDARD.encode(uuid::Uuid::new_v4().to_string());
    publish(&path, encoded.as_bytes())?;
    Ok(())
}

pub(super) fn entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    match fs::read_dir(path) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .map(|e| e.path())
                    .map_err(|_| "Cannot read Claude history directory.".into())
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("Cannot read Claude history directory.".into()),
    }
}

pub(super) fn regular_directory(path: &Path) -> bool {
    regular(path, true)
}
pub(super) fn regular_file(path: &Path) -> bool {
    regular(path, false)
}
fn regular(path: &Path, directory: bool) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return false;
            }
        }
        !m.file_type().is_symlink() && if directory { m.is_dir() } else { m.is_file() }
    })
}

fn stable_id(key: &str) -> String {
    let hash = Sha256::digest(format!("echobird:claude-desktop-history:{key}"));
    let mut bytes: [u8; 16] = hash[..16].try_into().unwrap();
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn claimed_ids(pointer: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for field in [
        "cliSessionId",
        "unarchivedCliSessionId",
        "preClearCliSessionId",
    ] {
        if let Some(id) = pointer[field].as_str() {
            ids.push(id.into());
        }
    }
    if let Some(prior) = pointer["priorCliSessionIds"].as_array() {
        ids.extend(prior.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    ids
}

fn deleted_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.strip_prefix("deleted_")?;
    uuid::Uuid::parse_str(name).ok().map(|id| id.to_string())
}

fn blocked(pointer: &Value, deleted: &HashSet<String>) -> bool {
    pointer["sessionId"]
        .as_str()
        .and_then(|id| id.strip_prefix("local_"))
        .is_some_and(|id| deleted.contains(id))
        || pointer["cliSessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| pointer["unarchivedCliSessionId"].as_str())
            .is_some_and(|id| deleted.contains(id))
}

fn synchronize(targets: &[PathBuf], cli_home: &Path, state: &Path) -> Result<usize, String> {
    if targets.is_empty() {
        return Ok(0);
    }
    super::native_account_files::private_dir(state)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state.join("merge.lock"))
        .map_err(|_| "Cannot lock Claude history.")?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| "Claude history preparation is already running.")?;
    let mut deleted = HashSet::new();
    let mut claimed = HashSet::new();
    let mut records = Vec::new();
    for scope in targets {
        for file in entries(scope)? {
            if !regular_file(&file) {
                continue;
            }
            if let Some(id) = deleted_id(&file) {
                deleted.insert(id);
                continue;
            }
            if file.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(value) = fs::read(&file).and_then(|bytes| {
                serde_json::from_slice::<Value>(&bytes).map_err(std::io::Error::other)
            }) else {
                continue;
            };
            // Only pointers whose identity matches their filename are native records.
            if value["sessionId"].as_str() != file.file_stem().and_then(|s| s.to_str()) {
                continue;
            }
            claimed.extend(claimed_ids(&value));
            if let Some(portable) = pointers::portable_pointer(&value) {
                records.push((scope.clone(), value, portable));
            }
        }
    }
    let host_ids: HashSet<_> = transcripts::files(cli_home)?
        .into_iter()
        .filter_map(|file| file.file_stem()?.to_str().map(str::to_owned))
        .collect();
    let mut newest: BTreeMap<String, Value> = BTreeMap::new();
    for (scope, original, portable) in records {
        if blocked(&original, &deleted) {
            continue;
        }
        let cli_id = portable["cliSessionId"].as_str().unwrap();
        let cwd = portable["cwd"].as_str().unwrap();
        if !host_ids.contains(cli_id) {
            // Native's confirmed external import moves a staged JSONL into projects.
            // Copy an unconfirmed stage rather than moving or altering its original.
            let expected = scope
                .join("imported-staging")
                .join(format!("{cli_id}.jsonl"));
            if original["stagedTranscriptPath"].as_str().map(Path::new) != Some(expected.as_path())
                || !regular_directory(&scope.join("imported-staging"))
                || !regular_file(&expected)
                || transcripts::read(&expected)?.is_none()
            {
                continue;
            }
            let Some(host) = transcripts::path(cli_home, cwd, cli_id) else {
                continue;
            };
            publish(
                &host,
                &fs::read(&expected).map_err(|_| "Cannot read staged Claude history.")?,
            )?;
        }
        let entry = newest
            .entry(portable["sessionId"].as_str().unwrap().into())
            .or_insert_with(|| portable.clone());
        if portable["lastActivityAt"].as_i64() > entry["lastActivityAt"].as_i64() {
            *entry = portable;
        }
    }
    // A native session can acquire a new CLI handle after resume/clear.
    // Select its latest pointer before considering separate CLI transcripts.
    let mut candidates: BTreeMap<String, Value> = BTreeMap::new();
    for portable in newest.into_values() {
        let entry = candidates
            .entry(portable["cliSessionId"].as_str().unwrap().into())
            .or_insert_with(|| portable.clone());
        if portable["lastActivityAt"].as_i64() > entry["lastActivityAt"].as_i64() {
            *entry = portable;
        }
    }
    for transcript in transcripts::scan(cli_home)? {
        let source = &transcript.source;
        if let Some(pointer) = candidates.get_mut(&source.cli_id) {
            // CLI and Desktop share the body; use its actual time for new scopes.
            if source.last_activity_at > pointer["lastActivityAt"].as_i64().unwrap_or(0) {
                pointer["lastActivityAt"] = source.last_activity_at.into();
            }
        }
        if transcript.desktop
            || claimed.contains(&source.cli_id)
            || deleted.contains(&source.cli_id)
        {
            continue;
        }
        let id = format!("local_{}", stable_id(&source.cli_id));
        if deleted.contains(id.strip_prefix("local_").unwrap()) {
            continue;
        }
        // Native diskTranscript scans all project directories by CLI UUID and
        // performs its own resume bring-home. Keep the single original body.
        candidates
            .entry(source.cli_id.clone())
            .or_insert_with(|| pointers::native_pointer(source, &id, &source.cli_id));
    }
    let mut created = 0;
    for scope in targets {
        let mut present = HashSet::new();
        for file in entries(scope)? {
            if !regular_file(&file) || file.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(&file).map_err(|_| "Cannot read Claude history pointer.")?;
            if let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) {
                if value["sessionId"].as_str() != file.file_stem().and_then(|s| s.to_str()) {
                    continue;
                }
                present.extend(claimed_ids(&value));
                if let Some(portable) = pointers::portable_pointer(&value) {
                    if let Some(latest) = candidates
                        .values()
                        .find(|p| p["sessionId"] == portable["sessionId"])
                        .or_else(|| candidates.get(portable["cliSessionId"].as_str().unwrap()))
                    {
                        if latest["lastActivityAt"].as_i64() > value["lastActivityAt"].as_i64() {
                            // Back up the original before advancing its shared body.
                            let backup = state
                                .join("backups")
                                .join(stable_id(&scope.to_string_lossy()))
                                .join(file.file_name().unwrap());
                            publish(&backup, &bytes)?;
                            if fs::read(&file)
                                .map_err(|_| "Cannot recheck Claude history pointer.")?
                                != bytes
                            {
                                return Err("Claude history changed during preparation.".into());
                            }
                            if latest["cliSessionId"] != portable["cliSessionId"] {
                                // Follow the newer native resume handle without
                                // carrying stale execution/model/leaf state.
                                value = latest.clone();
                                value["isArchived"] = portable["isArchived"].clone();
                            } else {
                                value["lastActivityAt"] = latest["lastActivityAt"].clone();
                            }
                            super::native_account_files::write_private_file(
                                &file,
                                &serde_json::to_vec_pretty(&value)
                                    .map_err(|_| "Cannot encode Claude history pointer.")?,
                            )?;
                            present.extend(claimed_ids(&value));
                        }
                    }
                }
            }
        }
        for (cli_id, pointer) in &candidates {
            if present.contains(cli_id) || blocked(pointer, &deleted) {
                continue;
            }
            let id = pointer["sessionId"].as_str().unwrap();
            if scope.join(format!("deleted_{}", &id[6..])).exists()
                || scope.join(format!("deleted_{cli_id}")).exists()
            {
                continue;
            }
            let path = scope.join(format!("{id}.json"));
            if publish(
                &path,
                &serde_json::to_vec_pretty(pointer)
                    .map_err(|_| "Cannot encode Claude history pointer.")?,
            )? {
                created += 1;
                present.insert(cli_id.clone());
            }
        }
    }
    if created > 0 {
        log::info!("[ClaudeHistory] added {created} missing Code session pointers");
    }
    Ok(created)
}

/// Atomic, exclusive publication. Existing native files are never replaced.
fn publish(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    let parent = path
        .parent()
        .ok_or("Cannot locate Claude history directory.")?;
    let mut directory = fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory
        .create(parent)
        .map_err(|_| "Cannot create Claude history directory.")?;
    let temporary = parent.join(format!(".echobird-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Cannot prepare Claude history.")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save Claude history.")?;
        drop(file);
        match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(_) => Err("Cannot publish Claude history; original files were preserved.".into()),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ONE: &str = "11111111-1111-4111-8111-111111111111";
    const TWO: &str = "22222222-2222-4222-8222-222222222222";

    struct Fixture {
        root: PathBuf,
        cli: PathBuf,
        targets: Vec<PathBuf>,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("claude-history-{}", uuid::Uuid::new_v4()));
            let cli = root.join("cli");
            let targets = vec![
                root.join("official/account/org"),
                root.join("third-party/account/org"),
            ];
            for target in &targets {
                fs::create_dir_all(target).unwrap();
            }
            Self { root, cli, targets }
        }
        fn transcript(&self, id: &str, entrypoint: &str) -> PathBuf {
            let cwd = self.root.join("project").to_string_lossy().into_owned();
            let path = transcripts::path(&self.cli, &cwd, id).unwrap();
            let rows = [
                json!({"type":"user", "uuid":ONE, "parentUuid":null,"sessionId":id,"cwd":cwd,
                    "entrypoint":entrypoint,"timestamp":"2026-01-01T10:00:00Z",
                    "message":{"role":"user","content":"Continue the fixture"}}),
                json!({"type":"assistant","uuid":TWO,"parentUuid":ONE,"sessionId":id,"cwd":cwd,
                    "timestamp":"2026-01-01T10:01:00Z","message":{"role":"assistant",
                    "content":[{"type":"text","text":"Fixture result"}]}}),
            ];
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                rows.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n",
            )
            .unwrap();
            path
        }
        fn pointer(&self, id: &str, archived: bool) -> Value {
            json!({"sessionId":format!("local_{id}"),"cliSessionId":id,
                "cwd":self.root.join("project"), "createdAt":1767261600000i64,
                "lastActivityAt":1767261660000i64,"isArchived":archived,"title":"Original native title",
                "model":"old-account-model", "permissionMode":"bypassPermissions",
                "remoteMcpServersConfig":[{"secret":"not-copied"}], "pendingFirstStart":true})
        }
        fn run(&self) -> usize {
            synchronize(&self.targets, &self.cli, &self.root.join("state")).unwrap()
        }
        fn pointers(&self, index: usize) -> Vec<Value> {
            entries(&self.targets[index])
                .unwrap()
                .iter()
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
                .map(|p| serde_json::from_slice(&fs::read(p).unwrap()).unwrap())
                .collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn merges_native_and_cli_across_accounts_and_gateway_without_rewriting_bodies() {
        let fixture = Fixture::new();
        let native_body = fixture.transcript(ONE, "claude-desktop-3p");
        let cli_body = fixture.transcript(TWO, "cli");
        let before = [
            fs::read(&native_body).unwrap(),
            fs::read(&cli_body).unwrap(),
        ];
        let native_pointer = fixture.targets[0].join(format!("local_{ONE}.json"));
        let original = serde_json::to_vec(&fixture.pointer(ONE, false)).unwrap();
        fs::write(&native_pointer, &original).unwrap();
        assert_eq!(fixture.run(), 3);
        assert_eq!(fixture.pointers(0).len(), 2);
        assert_eq!(fixture.pointers(1).len(), 2);
        let copied = fixture
            .pointers(1)
            .into_iter()
            .find(|p| p["cliSessionId"] == ONE)
            .unwrap();
        assert_eq!(copied["title"], "Original native title");
        assert_eq!(copied["lastActivityAt"], 1767261660000i64);
        for field in [
            "model",
            "permissionMode",
            "remoteMcpServersConfig",
            "pendingFirstStart",
            "importedFrom",
        ] {
            assert!(copied.get(field).is_none());
        }
        assert_eq!(fixture.run(), 0);
        assert_eq!(fs::read(native_pointer).unwrap(), original);
        assert_eq!(fs::read(native_body).unwrap(), before[0]);
        assert_eq!(fs::read(cli_body).unwrap(), before[1]);
    }

    #[test]
    fn tombstones_archives_orphan_desktop_and_subagents_are_not_revived() {
        let fixture = Fixture::new();
        fixture.transcript(ONE, "cli");
        fixture.transcript(TWO, "claude-desktop"); // deleted legacy Desktop body has no pointer
        fs::write(
            fixture.targets[0].join(format!("deleted_{ONE}")),
            b"1767261600000",
        )
        .unwrap();
        assert_eq!(fixture.run(), 0);
        fs::remove_file(fixture.targets[0].join(format!("deleted_{ONE}"))).unwrap();
        fs::write(
            fixture.targets[0].join(format!("local_{ONE}.json")),
            fixture.pointer(ONE, true).to_string(),
        )
        .unwrap();
        assert_eq!(fixture.run(), 1);
        assert!(fixture.pointers(1).iter().all(|p| p["isArchived"] == true));
        assert_eq!(fixture.run(), 0);
        // A Desktop UUID tombstone blocks copies even when its CLI UUID differs.
        let pointer = fixture.targets[1].join(format!("local_{ONE}.json"));
        fs::remove_file(pointer).unwrap();
        fs::write(fixture.targets[1].join(format!("deleted_{ONE}")), b"1").unwrap();
        assert_eq!(fixture.run(), 0);
    }

    #[test]
    fn malformed_or_remote_records_and_incomplete_transcripts_do_not_create_cards() {
        let fixture = Fixture::new();
        let path = fixture.transcript(ONE, "cli");
        let mut body = fs::read(&path).unwrap();
        body.extend_from_slice(b"{incomplete");
        fs::write(path, body).unwrap();
        let mut remote = fixture.pointer(TWO, false);
        remote["sshConfig"] = json!({"host":"fixture"});
        fs::write(
            fixture.targets[0].join(format!("local_{TWO}.json")),
            remote.to_string(),
        )
        .unwrap();
        assert_eq!(fixture.run(), 0);
        assert!(fixture.pointers(1).is_empty());
    }

    #[test]
    fn html_and_image_only_requests_are_imported_and_moved_cwd_does_not_copy_bodies() {
        let fixture = Fixture::new();
        let html = fixture.transcript(ONE, "cli");
        let mut rows: Vec<Value> = fs::read_to_string(&html)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        rows[0]["message"]["content"] = "<html>Fix this page</html>".into();
        rows[1]["cwd"] = json!(fixture.root.join("another-cwd"));
        fs::write(
            &html,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let image = fixture.transcript(TWO, "cli");
        let mut images: Vec<Value> = fs::read_to_string(&image)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        images[0]["message"]["content"] = json!([{"type":"image", "source":{"type":"base64","media_type":"image/png","data":"fixture"}}]);
        fs::write(
            &image,
            images
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let source_bytes = fs::read(&html).unwrap();
        let files_before = transcripts::files(&fixture.cli).unwrap().len();
        assert_eq!(fixture.run(), 4);
        assert_eq!(fixture.pointers(1).len(), 2);
        assert_eq!(
            transcripts::files(&fixture.cli).unwrap().len(),
            files_before
        );
        assert_eq!(fs::read(html).unwrap(), source_bytes);
        assert_eq!(fixture.run(), 0);
    }

    #[test]
    fn advances_shared_body_activity_with_backup_and_preserves_account_fields() {
        let fixture = Fixture::new();
        fixture.transcript(ONE, "claude-desktop-3p");
        let path = fixture.targets[0].join(format!("local_{ONE}.json"));
        let mut original = fixture.pointer(ONE, false);
        original["lastActivityAt"] = 1.into();
        let bytes = original.to_string();
        fs::write(&path, &bytes).unwrap();
        let unrelated: Vec<_> = [
            format!("local_{ONE}.json.bak"),
            format!("local_{ONE}.tmp"),
            "backup.json".into(),
            format!("local_{ONE}"),
        ]
        .into_iter()
        .map(|name| fixture.targets[0].join(name))
        .collect();
        for file in &unrelated {
            fs::write(file, &bytes).unwrap();
        }
        assert_eq!(fixture.run(), 1);
        for file in unrelated {
            assert_eq!(fs::read_to_string(file).unwrap(), bytes);
        }
        let updated: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(updated["lastActivityAt"], 1767261660000i64);
        assert_eq!(updated["model"], original["model"]);
        assert_eq!(updated["permissionMode"], original["permissionMode"]);
        let backup = fixture
            .root
            .join("state/backups")
            .join(stable_id(&fixture.targets[0].to_string_lossy()))
            .join(path.file_name().unwrap());
        assert_eq!(fs::read_to_string(backup).unwrap(), bytes);
        let after = fs::read(path).unwrap();
        assert_eq!(fixture.run(), 0);
        assert_eq!(
            fs::read(fixture.targets[0].join(format!("local_{ONE}.json"))).unwrap(),
            after
        );
    }

    #[test]
    fn follows_latest_native_resume_handle_across_repeated_account_switches() {
        let mut fixture = Fixture::new();
        let one = fixture.transcript(ONE, "claude-desktop");
        let two = fixture.transcript(TWO, "claude-desktop-3p");
        let bodies = [fs::read(&one).unwrap(), fs::read(&two).unwrap()];
        let old = fixture.pointer(ONE, true);
        fs::write(
            fixture.targets[0].join(format!("local_{ONE}.json")),
            old.to_string(),
        )
        .unwrap();
        let mut current = old.clone();
        current["cliSessionId"] = TWO.into();
        current["isArchived"] = false.into();
        current["priorCliSessionIds"] = json!([ONE]);
        current["lastActivityAt"] = 1767261760000i64.into();
        fs::write(
            fixture.targets[1].join(format!("local_{ONE}.json")),
            current.to_string(),
        )
        .unwrap();
        let third = fixture.root.join("third-account");
        fixture.targets.push(third);
        assert_eq!(fixture.run(), 1);
        for index in 0..3 {
            let pointers = fixture.pointers(index);
            assert_eq!(pointers.len(), 1);
            assert_eq!(pointers[0]["cliSessionId"], TWO);
            assert_eq!(pointers[0]["priorCliSessionIds"], json!([ONE]));
            assert_eq!(pointers[0]["isArchived"], index == 0);
        }
        assert_eq!(fixture.run(), 0);
        assert_eq!(fs::read(one).unwrap(), bodies[0]);
        assert_eq!(fs::read(two).unwrap(), bodies[1]);
    }

    #[test]
    fn deleted_ancestor_does_not_hide_a_live_fork_or_reimport_the_ancestor() {
        let fixture = Fixture::new();
        fixture.transcript(ONE, "cli");
        fixture.transcript(TWO, "claude-desktop");
        let mut child = fixture.pointer(TWO, false);
        child["priorCliSessionIds"] = json!([ONE]);
        fs::write(
            fixture.targets[0].join(format!("local_{TWO}.json")),
            child.to_string(),
        )
        .unwrap();
        fs::write(fixture.targets[0].join(format!("deleted_{ONE}")), b"1").unwrap();
        assert_eq!(fixture.run(), 1);
        assert_eq!(fixture.pointers(1).len(), 1);
        assert_eq!(fixture.pointers(1)[0]["cliSessionId"], TWO);
        assert_eq!(fixture.run(), 0);
    }

    #[test]
    fn staged_import_is_copied_and_registered_without_confirmation_or_source_mutation() {
        let fixture = Fixture::new();
        let body = fixture.transcript(ONE, "cli");
        let bytes = fs::read(&body).unwrap();
        fs::remove_file(&body).unwrap();
        let stage = fixture.targets[0]
            .join("imported-staging")
            .join(format!("{ONE}.jsonl"));
        fs::create_dir_all(stage.parent().unwrap()).unwrap();
        fs::write(&stage, &bytes).unwrap();
        let mut pointer = fixture.pointer(ONE, false);
        pointer["stagedTranscriptPath"] = json!(stage);
        pointer["importedFrom"] = "terminal-cli".into();
        let pointer_path = fixture.targets[0].join(format!("local_{ONE}.json"));
        fs::write(&pointer_path, pointer.to_string()).unwrap();
        assert_eq!(fixture.run(), 1);
        assert_eq!(fs::read(body).unwrap(), bytes);
        assert_eq!(fs::read(stage).unwrap(), bytes);
        assert_eq!(
            fs::read_to_string(pointer_path).unwrap(),
            pointer.to_string()
        );
        assert!(fixture.pointers(1)[0].get("importedFrom").is_none());
    }

    #[test]
    fn publication_is_exclusive_and_failure_keeps_original_files() {
        let fixture = Fixture::new();
        let path = fixture.root.join("already.json");
        fs::write(&path, b"original").unwrap();
        assert!(!publish(&path, b"replacement").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(publish(&path.join("child.json"), b"cannot-write").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(!entries(&fixture.root)
            .unwrap()
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "tmp")));
    }
}
