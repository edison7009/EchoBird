//! EchoBird thin shell.
//!
//! All business logic lives in the private `echobird_core` crate. This
//! file is intentionally minimal because three things MUST be expanded
//! at compile time inside the public binary crate, where the
//! CARGO_MANIFEST_DIR points at this directory:
//!
//!   * `tauri::generate_context!()` resolves `tauri.conf.json`.
//!   * `include_bytes!("../icons/tray-icon.png")` reads the tray icon.
//!   * The `include_str!` calls that pull every prompt / install /
//!     script asset out of `../../docs/api/...` (these files also feed
//!     echobird.ai/api/..., so they intentionally live in the public
//!     repository).
//!
//! Once those compile-time expansions happen here we register the
//! bundled-asset table with `echobird_core` and call `echobird_core::run`,
//! which contains every other line of EchoBird's Rust code.

use echobird_core::services::bundled_assets::BundledAssets;

static BUNDLED: BundledAssets = BundledAssets {
    mother_system_prompt: include_str!("../../docs/api/mother/system_prompt.md"),
    mother_hints_json: include_str!("../../docs/api/mother/hints.json"),
    install_index_json: include_str!("../../docs/api/tools/install/index.json"),
    install_refs: &[
        (
            "claudecode",
            include_str!("../../docs/api/tools/install/claudecode.json"),
        ),
        (
            "codex",
            include_str!("../../docs/api/tools/install/codex.json"),
        ),
        (
            "qwencode",
            include_str!("../../docs/api/tools/install/qwencode.json"),
        ),
        (
            "aider",
            include_str!("../../docs/api/tools/install/aider.json"),
        ),
        ("pi", include_str!("../../docs/api/tools/install/pi.json")),
        (
            "hermes",
            include_str!("../../docs/api/tools/install/hermes.json"),
        ),
        (
            "nanobot",
            include_str!("../../docs/api/tools/install/nanobot.json"),
        ),
        (
            "openclaw",
            include_str!("../../docs/api/tools/install/openclaw.json"),
        ),
        (
            "opencode",
            include_str!("../../docs/api/tools/install/opencode.json"),
        ),
        (
            "openfang",
            include_str!("../../docs/api/tools/install/openfang.json"),
        ),
        (
            "picoclaw",
            include_str!("../../docs/api/tools/install/picoclaw.json"),
        ),
        (
            "zeroclaw",
            include_str!("../../docs/api/tools/install/zeroclaw.json"),
        ),
        (
            "claudedesktop",
            include_str!("../../docs/api/tools/install/claudedesktop.json"),
        ),
        (
            "codexdesktop",
            include_str!("../../docs/api/tools/install/codexdesktop.json"),
        ),
        (
            "geminidesktop",
            include_str!("../../docs/api/tools/install/geminidesktop.json"),
        ),
        (
            "coffeecli",
            include_str!("../../docs/api/tools/install/coffeecli.json"),
        ),
        (
            "vscode",
            include_str!("../../docs/api/tools/install/vscode.json"),
        ),
        (
            "cursor",
            include_str!("../../docs/api/tools/install/cursor.json"),
        ),
        (
            "windsurf",
            include_str!("../../docs/api/tools/install/windsurf.json"),
        ),
        (
            "trae",
            include_str!("../../docs/api/tools/install/trae.json"),
        ),
        (
            "traecn",
            include_str!("../../docs/api/tools/install/traecn.json"),
        ),
    ],
    tool_scripts: &[
        (
            "network-info",
            include_str!("../../docs/api/tools/network-info.md"),
        ),
        (
            "security-audit",
            include_str!("../../docs/api/tools/security-audit.md"),
        ),
    ],
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    let tray_icon_bytes: &'static [u8] = include_bytes!("../icons/tray-icon.png");

    echobird_core::services::bundled_assets::register(&BUNDLED);
    echobird_core::run(context, tray_icon_bytes);
}
