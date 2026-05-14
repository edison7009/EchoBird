// Auto-skip Codex onboarding/login by patching .codex-global-state.json.
// Port of tools/codex/lib/onboarding-bypass.cjs.
//
// Codex checks several flags in its global state file to decide whether
// to show the welcome / login flow. EchoBird wants Codex to launch
// straight into the main UI on every spawn — no login screen, no
// projectless walkthrough, no first-run wizard. We patch a handful of
// flags inside the `electron-persisted-atom-state` blob to make Codex
// believe onboarding has already been completed.
//
// Writes are atomic (write-to-tmp + rename) and create a `.bak` backup
// before touching the original, so a SIGKILL between operations can
// never leave Codex with a half-written global state.
//
// All path I/O is parameterized by `codex_dir: &Path` so the unit
// tests run against a temp directory without touching the real
// `~/.codex/.codex-global-state.json`.

use serde_json::{json, Value};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// `bypass_onboarding` is called by `process_manager.rs::start_codex_native`
// just before spawning Codex; the rest of this module exists to make
// that call testable in isolation. The function is intentionally
// idempotent — it returns `AlreadyBypassed` without writing when every
// flag we care about is already set, so calling it on every spawn is
// cheap and we don't need a separate "is_complete" pre-check.

/// Filename inside the Codex dir.
const GLOBAL_STATE_FILE: &str = ".codex-global-state.json";

/// Outcome tag for `bypass_onboarding`. The variant tells the caller
/// whether anything actually changed on disk (useful for logging).
#[derive(Debug, PartialEq, Eq)]
pub enum BypassOutcome {
    /// File was missing or out-of-date; we wrote a fresh patched
    /// version. The user's old file (if any) is preserved at `<file>.bak`.
    Patched,
    /// File already had every flag we care about; no write performed.
    AlreadyBypassed,
}

/// Patch the Codex global state file to skip onboarding. Returns the
/// outcome on success, or an I/O error if reading/writing failed.
pub fn bypass_onboarding(codex_dir: &Path) -> io::Result<BypassOutcome> {
    let global_state_path = codex_dir.join(GLOBAL_STATE_FILE);

    // Load existing state or start with an empty object. Parse errors
    // are non-fatal — we just rebuild the state cleanly.
    let mut state: Value = match fs::read_to_string(&global_state_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| json!({})),
        Err(_) => json!({}),
    };

    let modified = apply_patches(&mut state);
    if !modified {
        return Ok(BypassOutcome::AlreadyBypassed);
    }

    // Ensure parent dir exists before any writes.
    fs::create_dir_all(codex_dir)?;

    // Backup the existing file (if any) before we rewrite. Matches the
    // .cjs version's behavior: keep a snapshot at `<file>.bak`.
    if global_state_path.exists() {
        let backup_path: PathBuf = path_with_suffix(&global_state_path, ".bak");
        fs::copy(&global_state_path, &backup_path)?;
    }

    // Atomic write: serialize to a .tmp sibling, then rename onto the
    // target so the global state file is either fully old or fully new.
    let tmp_path: PathBuf = path_with_suffix(&global_state_path, ".tmp");
    let serialized = serde_json::to_string_pretty(&state).map_err(io::Error::other)?;
    fs::write(&tmp_path, serialized)?;
    match fs::rename(&tmp_path, &global_state_path) {
        Ok(()) => Ok(BypassOutcome::Patched),
        Err(e) => {
            // Best-effort cleanup of the orphan .tmp.
            let _ = fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Append a suffix to a path. Used to derive `.bak` / `.tmp` sibling
// paths. We append textually rather than using `with_extension` because
// the global state filename already starts with a dot.
fn path_with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s: std::ffi::OsString = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

// Apply the onboarding-skip flags to `state` in place. Returns true if
// anything was actually changed. The list is kept in lockstep with the
// original JS version so a state file that round-tripped through
// either implementation looks identical.
fn apply_patches(state: &mut Value) -> bool {
    let mut modified = false;

    // Ensure `electron-persisted-atom-state` is an object.
    if !state
        .get("electron-persisted-atom-state")
        .map(|v| v.is_object())
        .unwrap_or(false)
    {
        state["electron-persisted-atom-state"] = json!({});
        modified = true;
    }
    let atom = state["electron-persisted-atom-state"]
        .as_object_mut()
        .expect("just ensured object");

    // Booleans / strings we set unconditionally.
    let patches: [(&str, Value); 4] = [
        ("electron:onboarding-override", Value::String("auto".into())),
        ("electron:onboarding-welcome-pending", Value::Bool(false)),
        (
            "electron:onboarding-projectless-completed",
            Value::Bool(true),
        ),
        ("skip-full-access-confirm", Value::Bool(true)),
    ];
    for (k, v) in patches {
        if atom.get(k) != Some(&v) {
            atom.insert(k.to_string(), v);
            modified = true;
        }
    }

    // Timestamp — only set if missing (preserve original timestamp on
    // re-runs so we don't make Codex think onboarding just happened).
    if !atom
        .get("last_completed_onboarding")
        .map(|v| !v.is_null())
        .unwrap_or(false)
    {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        atom.insert(
            "last_completed_onboarding".into(),
            Value::Number(now_millis.into()),
        );
        modified = true;
    }

    // Agent mode for "local" host. Only set if not already present so
    // the user's explicit choice isn't overwritten.
    let need_init_map = !atom
        .get("agent-mode-by-host-id")
        .map(|v| v.is_object())
        .unwrap_or(false);
    if need_init_map {
        atom.insert("agent-mode-by-host-id".into(), json!({}));
        modified = true;
    }
    let host_map = atom["agent-mode-by-host-id"]
        .as_object_mut()
        .expect("just ensured");
    if !host_map.contains_key("local") {
        host_map.insert("local".into(), Value::String("full-access".into()));
        modified = true;
    }

    modified
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmpdir(label: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("echobird_onb_{label}_{pid}_{n}"));
        fs::create_dir_all(&dir).expect("tmpdir create");
        dir
    }

    fn read_state(path: &Path) -> Value {
        let content = fs::read_to_string(path).expect("read state");
        serde_json::from_str(&content).expect("parse state")
    }

    // ---- bypass_onboarding ----

    #[test]
    fn bypass_creates_file_when_missing() {
        let dir = unique_tmpdir("missing");
        let outcome = bypass_onboarding(&dir).expect("ok");
        assert_eq!(outcome, BypassOutcome::Patched);

        let p = dir.join(GLOBAL_STATE_FILE);
        assert!(p.exists());
        let state = read_state(&p);
        let atom = &state["electron-persisted-atom-state"];
        assert_eq!(atom["electron:onboarding-override"], "auto");
        assert_eq!(atom["electron:onboarding-welcome-pending"], false);
        assert_eq!(atom["electron:onboarding-projectless-completed"], true);
        assert_eq!(atom["skip-full-access-confirm"], true);
        assert!(atom["last_completed_onboarding"].is_number());
        assert_eq!(atom["agent-mode-by-host-id"]["local"], "full-access");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bypass_returns_already_bypassed_on_second_run() {
        let dir = unique_tmpdir("idempotent");
        let first = bypass_onboarding(&dir).expect("ok");
        assert_eq!(first, BypassOutcome::Patched);
        let second = bypass_onboarding(&dir).expect("ok");
        assert_eq!(second, BypassOutcome::AlreadyBypassed);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bypass_preserves_existing_unrelated_keys() {
        let dir = unique_tmpdir("preserve");
        let p = dir.join(GLOBAL_STATE_FILE);
        let initial = json!({
            "some-other-top-level-key": "keep me",
            "electron-persisted-atom-state": {
                "user-preference-x": 42,
            },
        });
        fs::write(&p, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        bypass_onboarding(&dir).expect("ok");
        let state = read_state(&p);
        assert_eq!(state["some-other-top-level-key"], "keep me");
        assert_eq!(
            state["electron-persisted-atom-state"]["user-preference-x"],
            42
        );
        // And the onboarding flags are now set.
        assert_eq!(
            state["electron-persisted-atom-state"]["electron:onboarding-override"],
            "auto"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bypass_creates_backup_when_file_already_existed() {
        let dir = unique_tmpdir("backup");
        let p = dir.join(GLOBAL_STATE_FILE);
        fs::write(&p, "{\"original\":true}").unwrap();

        bypass_onboarding(&dir).expect("ok");
        let backup = path_with_suffix(&p, ".bak");
        assert!(backup.exists(), "backup not created");
        let backup_content = fs::read_to_string(&backup).unwrap();
        assert!(backup_content.contains("\"original\""), "backup mismatch");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bypass_rebuilds_from_malformed_json() {
        let dir = unique_tmpdir("malformed");
        let p = dir.join(GLOBAL_STATE_FILE);
        fs::write(&p, "not-valid-json{").unwrap();

        // Should not error — we just rebuild from {}. The malformed
        // payload survives in the .bak backup.
        let outcome = bypass_onboarding(&dir).expect("ok");
        assert_eq!(outcome, BypassOutcome::Patched);
        let state = read_state(&p);
        assert_eq!(
            state["electron-persisted-atom-state"]["electron:onboarding-override"],
            "auto"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bypass_does_not_overwrite_existing_agent_mode() {
        let dir = unique_tmpdir("agentmode");
        let p = dir.join(GLOBAL_STATE_FILE);
        let initial = json!({
            "electron-persisted-atom-state": {
                "agent-mode-by-host-id": { "local": "read-only" },
            },
        });
        fs::write(&p, serde_json::to_string(&initial).unwrap()).unwrap();

        bypass_onboarding(&dir).expect("ok");
        let state = read_state(&p);
        // User's explicit choice is preserved.
        assert_eq!(
            state["electron-persisted-atom-state"]["agent-mode-by-host-id"]["local"],
            "read-only"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bypass_preserves_existing_onboarding_timestamp() {
        let dir = unique_tmpdir("timestamp");
        let p = dir.join(GLOBAL_STATE_FILE);
        let initial = json!({
            "electron-persisted-atom-state": {
                "last_completed_onboarding": 1_700_000_000_000_u64,
            },
        });
        fs::write(&p, serde_json::to_string(&initial).unwrap()).unwrap();

        bypass_onboarding(&dir).expect("ok");
        let state = read_state(&p);
        // Original timestamp survives — we only set it if missing.
        assert_eq!(
            state["electron-persisted-atom-state"]["last_completed_onboarding"],
            1_700_000_000_000_u64
        );

        fs::remove_dir_all(&dir).ok();
    }

    // ---- path_with_suffix ----

    #[test]
    fn path_with_suffix_appends_textually() {
        let p = Path::new("/tmp/.codex-global-state.json");
        assert_eq!(
            path_with_suffix(p, ".bak"),
            PathBuf::from("/tmp/.codex-global-state.json.bak")
        );
        assert_eq!(
            path_with_suffix(p, ".tmp"),
            PathBuf::from("/tmp/.codex-global-state.json.tmp")
        );
    }
}
