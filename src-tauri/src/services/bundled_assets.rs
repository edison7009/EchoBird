// Bundled install/script assets — embedded at compile time via include_str!.
// Lets the smart-install flow work fully offline; no network round-trips for
// system prompt, hints, install references, or task scripts.
//
// Source of truth lives in repo-root `docs/api/...` (also served by the
// website at echobird.ai/api/...). Bundling and remote stay in sync because
// `include_str!` is resolved at compile time.

pub const MOTHER_SYSTEM_PROMPT: &str = include_str!("../../../docs/api/mother/system_prompt.md");
pub const MOTHER_HINTS_JSON: &str = include_str!("../../../docs/api/mother/hints.json");
pub const INSTALL_INDEX_JSON: &str = include_str!("../../../docs/api/tools/install/index.json");

pub fn get_install_ref(tool_id: &str) -> Option<&'static str> {
    match tool_id {
        "claudecode" => Some(include_str!("../../../docs/api/tools/install/claudecode.json")),
        "codex" => Some(include_str!("../../../docs/api/tools/install/codex.json")),
        "qwencode" => Some(include_str!("../../../docs/api/tools/install/qwencode.json")),
        "aider" => Some(include_str!("../../../docs/api/tools/install/aider.json")),
        "hermes" => Some(include_str!("../../../docs/api/tools/install/hermes.json")),
        "nanobot" => Some(include_str!("../../../docs/api/tools/install/nanobot.json")),
        "openclaw" => Some(include_str!("../../../docs/api/tools/install/openclaw.json")),
        "opencode" => Some(include_str!("../../../docs/api/tools/install/opencode.json")),
        "openfang" => Some(include_str!("../../../docs/api/tools/install/openfang.json")),
        "picoclaw" => Some(include_str!("../../../docs/api/tools/install/picoclaw.json")),
        "zeroclaw" => Some(include_str!("../../../docs/api/tools/install/zeroclaw.json")),
        "claudedesktop" => Some(include_str!("../../../docs/api/tools/install/claudedesktop.json")),
        "codexdesktop" => Some(include_str!("../../../docs/api/tools/install/codexdesktop.json")),
        "geminidesktop" => Some(include_str!("../../../docs/api/tools/install/geminidesktop.json")),
        "coffeecli" => Some(include_str!("../../../docs/api/tools/install/coffeecli.json")),
        "vscode" => Some(include_str!("../../../docs/api/tools/install/vscode.json")),
        "cursor" => Some(include_str!("../../../docs/api/tools/install/cursor.json")),
        "windsurf" => Some(include_str!("../../../docs/api/tools/install/windsurf.json")),
        "trae" => Some(include_str!("../../../docs/api/tools/install/trae.json")),
        "traecn" => Some(include_str!("../../../docs/api/tools/install/traecn.json")),
        _ => None,
    }
}

pub fn get_tool_script(name: &str) -> Option<&'static str> {
    match name {
        "network-info" => Some(include_str!("../../../docs/api/tools/network-info.md")),
        "security-audit" => Some(include_str!("../../../docs/api/tools/security-audit.md")),
        _ => None,
    }
}

/// IDs of every tool with a bundled install reference. Mirrors the keys of
/// `get_install_ref` so the system prompt and AppManager stay in sync.
pub const INSTALLABLE_TOOL_IDS: &[&str] = &[
    "claudecode", "codex", "qwencode", "aider", "hermes",
    "nanobot", "openclaw", "opencode", "openfang", "picoclaw", "zeroclaw",
    "claudedesktop", "codexdesktop", "geminidesktop", "coffeecli",
    "vscode", "cursor", "windsurf", "trae", "traecn",
];

/// Build the full embedded-references block to append to the system prompt.
/// The agent reads from this instead of `web_fetch`-ing echobird.ai.
pub fn build_embedded_refs_section() -> String {
    let mut out = String::with_capacity(48 * 1024);
    out.push_str("\n\n---\n\n## OFFLINE-FIRST: Embedded Install References\n\n");
    out.push_str(
        "The references below are bundled with the EchoBird app. **PREFER \
         them over `web_fetch`** — many users choose smart-install precisely \
         because their network is unreliable. Only fall back to `web_fetch` \
         for tools not in this list.\n\n",
    );

    out.push_str("### Tool Install JSONs\n\n");
    for tool_id in INSTALLABLE_TOOL_IDS {
        if let Some(json) = get_install_ref(tool_id) {
            out.push_str(&format!(
                "#### `{}` install reference\n```json\n{}\n```\n\n",
                tool_id,
                json.trim()
            ));
        }
    }

    out.push_str("### Quick-Action Task Scripts\n\n");
    for name in &["network-info", "security-audit"] {
        if let Some(md) = get_tool_script(name) {
            out.push_str(&format!(
                "#### `{}.md` (use this when the matching Quick Action runs)\n{}\n\n",
                name,
                md.trim()
            ));
        }
    }

    out
}
