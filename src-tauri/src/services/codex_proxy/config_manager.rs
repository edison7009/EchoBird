// Codex config + relay file management — port of tools/codex/lib/config-manager.cjs.
//
// Two filesystem locations:
//
//   ~/.codex/config.toml        ← Codex's own config. We own its shape
//                                 end-to-end (canonical 13-line template
//                                 with base_url = http://127.0.0.1:53682/v1
//                                 and wire_api = "responses"). `apply_codex`
//                                 in tool_config_manager.rs writes this
//                                 whenever Codex is selected; this module
//                                 provides a defensive read-and-rewrite-if-
//                                 drifted helper for the proxy's startup
//                                 path (Phase 6).
//
//   ~/.echobird/codex.json      ← The relay file. EchoBird writes the
//                                 currently-selected model / API key /
//                                 upstream base_url here, and the proxy
//                                 reads it FRESH on every incoming
//                                 request so model switches take effect
//                                 without restarting Codex or the proxy.
//
// Both paths can be overridden via env vars for tests:
//   ECHOBIRD_CODEX_CONFIG_DIR  → overrides ~/.codex
//   ECHOBIRD_RELAY_DIR         → overrides ~/.echobird
//
// The path-derivation helpers are split from the IO helpers so the IO
// layer is testable with explicit paths.

use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::CODEX_PROXY_PORT;

/// File name (under the relay dir) where EchoBird writes the
/// currently-selected Codex upstream config.
pub const RELAY_FILENAME: &str = "codex.json";

/// File name (under the Codex dir) Codex reads at startup.
//
// Phase 7 will call `ensure_canonical_config` from process_manager.rs
// just before spawning Codex, replacing the equivalent check inside
// codex-launcher.cjs. Until then these are reachable only from tests.
#[allow(dead_code)]
pub const CODEX_CONFIG_FILENAME: &str = "config.toml";

/// The base_url Codex sees. The same value is baked into
/// `apply_codex` over in `tool_config_manager.rs` — keep them in sync.
#[allow(dead_code)]
pub fn codex_proxy_url() -> String {
    format!("http://127.0.0.1:{CODEX_PROXY_PORT}/v1")
}

/// The exact 13-line config.toml shape we own. Codex must see this
/// verbatim — any drift (different model id, missing review_model,
/// changed wire_api) breaks the protocol bridge. `apply_codex` writes
/// the same template, and `ensure_canonical_config` rewrites if drift
/// is detected.
#[allow(dead_code)]
pub fn canonical_config_toml() -> String {
    format!(
        "model_provider = \"OpenAI\"\n\
         model = \"gpt-5.4\"\n\
         review_model = \"gpt-5.4\"\n\
         model_reasoning_effort = \"xhigh\"\n\
         disable_response_storage = true\n\
         network_access = \"enabled\"\n\
         model_context_window = 1000000\n\
         model_auto_compact_token_limit = 900000\n\
         \n\
         [model_providers.OpenAI]\n\
         name = \"OpenAI\"\n\
         base_url = \"{url}\"\n\
         wire_api = \"responses\"\n\
         requires_openai_auth = true\n",
        url = codex_proxy_url()
    )
}

/// Default Codex config directory: env override → `~/.codex`.
#[allow(dead_code)]
pub fn default_codex_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ECHOBIRD_CODEX_CONFIG_DIR") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    dirs::home_dir().map(|h| h.join(".codex"))
}

/// Default relay directory: env override → `~/.echobird`.
pub fn default_relay_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ECHOBIRD_RELAY_DIR") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    dirs::home_dir().map(|h| h.join(".echobird"))
}

/// Outcome of `ensure_canonical_config`. The `reason` field is a stable
/// tag suitable for logging / tests.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub struct EnsureOutcome {
    pub wrote: bool,
    pub reason: &'static str,
}

/// Verify config.toml at `codex_config_path` points Codex at our proxy.
/// If missing or drifted, rewrite it to the canonical template.
/// Idempotent: cheap when already correct, self-healing when not.
#[allow(dead_code)]
pub fn ensure_canonical_config(codex_config_path: &Path) -> io::Result<EnsureOutcome> {
    let template = canonical_config_toml();
    let proxy_url = codex_proxy_url();

    let current = fs::read_to_string(codex_config_path);
    match current {
        Ok(content) => {
            // Quick check: does the file mention our exact proxy URL?
            // If yes, it's compatible (Codex hits our proxy). If not,
            // replace the whole file — we don't merge, we own the shape.
            if content.contains(&proxy_url) {
                Ok(EnsureOutcome {
                    wrote: false,
                    reason: "already-canonical",
                })
            } else {
                if let Some(parent) = codex_config_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(codex_config_path, template)?;
                Ok(EnsureOutcome {
                    wrote: true,
                    reason: "drifted",
                })
            }
        }
        Err(_) => {
            if let Some(parent) = codex_config_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(codex_config_path, template)?;
            Ok(EnsureOutcome {
                wrote: true,
                reason: "missing",
            })
        }
    }
}

/// Read the relay file fresh. Called by the proxy on EVERY incoming
/// request so model switches take effect without restarting anything:
/// EchoBird's `apply_codex` rewrites this JSON, and the next request
/// the proxy sees uses the new model / key / upstream URL.
///
/// Returns None if the file is missing or malformed — caller should
/// respond with a clear error to Codex.
pub fn read_echobird_relay(relay_config_path: &Path) -> Option<Value> {
    let content = fs::read_to_string(relay_config_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Detect the official OpenAI host. Used by the proxy to skip the
/// model-id rewrite for real OpenAI calls (OpenAI's `/responses`
/// endpoint already accepts Codex's request shape verbatim).
pub fn is_openai(url: &str) -> bool {
    !url.is_empty() && url.contains("api.openai.com")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmpdir(label: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("echobird_cfg_{label}_{pid}_{n}"));
        fs::create_dir_all(&dir).expect("tmpdir create");
        dir
    }

    // ---- canonical_config_toml ----

    #[test]
    fn canonical_template_contains_proxy_url() {
        let t = canonical_config_toml();
        assert!(t.contains("127.0.0.1:53682"), "got: {t}");
        assert!(t.contains("wire_api = \"responses\""), "got: {t}");
        assert!(t.contains("model = \"gpt-5.4\""), "got: {t}");
        assert!(t.contains("[model_providers.OpenAI]"), "got: {t}");
    }

    #[test]
    fn canonical_template_is_13_content_lines() {
        // The template was historically described as "13 lines". Verify
        // the line count stays stable so accidental edits get caught.
        let t = canonical_config_toml();
        let lines: Vec<&str> = t.lines().collect();
        // 8 top-level + 1 blank + 5 provider block = 14 lines, plus
        // the trailing newline. Match the JS template byte-for-byte.
        assert_eq!(lines.len(), 14, "got {} lines: {t}", lines.len());
    }

    // ---- ensure_canonical_config ----

    #[test]
    fn ensure_writes_when_file_missing() {
        let dir = unique_tmpdir("missing");
        let cfg = dir.join(CODEX_CONFIG_FILENAME);
        assert!(!cfg.exists());

        let out = ensure_canonical_config(&cfg).expect("ok");
        assert_eq!(out.reason, "missing");
        assert!(out.wrote);
        let written = fs::read_to_string(&cfg).unwrap();
        assert!(written.contains("127.0.0.1:53682"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_no_op_when_already_canonical() {
        let dir = unique_tmpdir("canonical");
        let cfg = dir.join(CODEX_CONFIG_FILENAME);
        fs::write(&cfg, canonical_config_toml()).unwrap();

        let out = ensure_canonical_config(&cfg).expect("ok");
        assert_eq!(out.reason, "already-canonical");
        assert!(!out.wrote);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_rewrites_when_drifted() {
        let dir = unique_tmpdir("drifted");
        let cfg = dir.join(CODEX_CONFIG_FILENAME);
        // Drifted shape: points at api.openai.com instead of our proxy.
        fs::write(
            &cfg,
            "model_provider = \"OpenAI\"\nbase_url = \"https://api.openai.com/v1\"\n",
        )
        .unwrap();

        let out = ensure_canonical_config(&cfg).expect("ok");
        assert_eq!(out.reason, "drifted");
        assert!(out.wrote);
        let written = fs::read_to_string(&cfg).unwrap();
        assert!(written.contains("127.0.0.1:53682"));
        // Drifted line must be gone.
        assert!(!written.contains("api.openai.com"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_creates_parent_dir_if_missing() {
        let dir = unique_tmpdir("nested");
        // Two levels of non-existent subdirs.
        let cfg = dir.join("sub1").join("sub2").join(CODEX_CONFIG_FILENAME);
        let out = ensure_canonical_config(&cfg).expect("ok");
        assert_eq!(out.reason, "missing");
        assert!(cfg.exists());

        fs::remove_dir_all(&dir).ok();
    }

    // ---- read_echobird_relay ----

    #[test]
    fn relay_returns_none_when_file_missing() {
        let dir = unique_tmpdir("relaymiss");
        let p = dir.join(RELAY_FILENAME);
        assert_eq!(read_echobird_relay(&p), None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn relay_returns_parsed_json_when_present() {
        let dir = unique_tmpdir("relayok");
        let p = dir.join(RELAY_FILENAME);
        let payload = json!({
            "model": "deepseek-chat",
            "base_url": "https://api.deepseek.com/v1",
            "api_key": "sk-test",
        });
        fs::write(&p, serde_json::to_string(&payload).unwrap()).unwrap();

        let out = read_echobird_relay(&p).expect("some");
        assert_eq!(out["model"], "deepseek-chat");
        assert_eq!(out["base_url"], "https://api.deepseek.com/v1");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn relay_returns_none_when_malformed_json() {
        let dir = unique_tmpdir("relaybad");
        let p = dir.join(RELAY_FILENAME);
        fs::write(&p, "not-json-at-all{").unwrap();
        assert_eq!(read_echobird_relay(&p), None);
        fs::remove_dir_all(&dir).ok();
    }

    // ---- is_openai ----

    #[test]
    fn is_openai_matches_official_host() {
        assert!(is_openai("https://api.openai.com/v1"));
        assert!(is_openai("https://api.openai.com/v1/chat/completions"));
    }

    #[test]
    fn is_openai_rejects_third_party_hosts() {
        assert!(!is_openai("https://api.deepseek.com/v1"));
        assert!(!is_openai("https://api.minimax.io/v1"));
        assert!(!is_openai("http://127.0.0.1:53682/v1"));
    }

    #[test]
    fn is_openai_rejects_empty_string() {
        assert!(!is_openai(""));
    }
}
