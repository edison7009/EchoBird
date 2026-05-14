// Codex binary resolution — port of tools/codex/lib/binary-resolver.cjs.
//
// Two flavors:
//
//   CLI (Codex CLI / `codex` command):
//     Codex v0.107+ ships as a Rust binary inside a platform-specific
//     npm package (`@openai/codex-<triple>`). Spawning the .cmd shim
//     directly drops TTY-ness inside the cmd /d /s /c wrapper and the
//     Rust TUI aborts with "stdin is not a terminal". We find the
//     bundled native exe under the npm install root and spawn it
//     directly; if the npm install isn't where we expect, we fall back
//     to the .cmd shim (which works in most non-TTY cases).
//
//   Desktop (Codex Desktop app):
//     Independent of npm. Standalone installer drops a .exe at the
//     usual Programs path; Microsoft Store install exposes an alias
//     under %LOCALAPPDATA%\Microsoft\WindowsApps. macOS ships as a .app
//     bundle under /Applications. Linux: no desktop build as of 2026-05.
//
// All public functions return Option<PathBuf> with `None` meaning
// "not found via the standard search path" — caller decides whether to
// fall back to a shell-resolved PATH lookup or to error out.

use std::path::PathBuf;
use std::time::Duration;

/// Resolve the standalone Codex Desktop binary path. Returns None on
/// Linux (no desktop build) or when the standard install locations
/// don't exist.
pub fn resolve_desktop_binary() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            // 1. Standalone installer default location.
            candidates.push(
                PathBuf::from(&local_app_data)
                    .join("Programs")
                    .join("Codex")
                    .join("Codex.exe"),
            );
            // 2. Microsoft Store executable alias — Windows 10+ exposes
            //    a shim here that resolves to the Store package.
            candidates.push(
                PathBuf::from(&local_app_data)
                    .join("Microsoft")
                    .join("WindowsApps")
                    .join("Codex.exe"),
            );
        }
        // 3. PATH lookup via `where` as a last resort.
        if let Some(p) = which_first("Codex.exe") {
            candidates.push(p);
        }
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Codex.app/Contents/MacOS/Codex",
        ));
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("Applications")
                    .join("Codex.app")
                    .join("Contents")
                    .join("MacOS")
                    .join("Codex"),
            );
        }
    }

    candidates.into_iter().find(|c| c.exists())
}

/// Resolve the `shell:AppsFolder\<AUMID>` launch URI for Codex Desktop
/// on Windows Store installs. Reads from `tools/codexdesktop/paths.json`
/// — the file shipped in the EchoBird tools directory.
#[cfg(windows)]
pub fn resolve_desktop_launch_uri(tools_dir: &std::path::Path) -> Option<String> {
    let path = tools_dir.join("codexdesktop").join("paths.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let cfg: serde_json::Value = serde_json::from_str(&content).ok()?;
    cfg.get("launchUri")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(not(windows))]
pub fn resolve_desktop_launch_uri(_tools_dir: &std::path::Path) -> Option<String> {
    None
}

/// Resolve the native Codex CLI binary (the platform-specific Rust exe
/// shipped inside `@openai/codex-<triple>`). Returns None if neither
/// the global npm install nor any well-known fallback location holds
/// the expected file.
pub fn resolve_codex_cli_binary() -> Option<PathBuf> {
    let (plat_pkg, triple, exe_name) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => (
            "@openai/codex-win32-arm64",
            "aarch64-pc-windows-msvc",
            "codex.exe",
        ),
        ("windows", _) => (
            "@openai/codex-win32-x64",
            "x86_64-pc-windows-msvc",
            "codex.exe",
        ),
        ("macos", "aarch64") => (
            "@openai/codex-darwin-arm64",
            "aarch64-apple-darwin",
            "codex",
        ),
        ("macos", _) => ("@openai/codex-darwin-x64", "x86_64-apple-darwin", "codex"),
        ("linux", "aarch64") => (
            "@openai/codex-linux-arm64",
            "aarch64-unknown-linux-musl",
            "codex",
        ),
        ("linux", _) => (
            "@openai/codex-linux-x64",
            "x86_64-unknown-linux-musl",
            "codex",
        ),
        _ => return None,
    };

    let mut codex_pkg_roots: Vec<PathBuf> = Vec::new();

    // npm prefix dance: ask `which`/`where` for the codex shim, then
    // climb to its sibling `node_modules\@openai\codex` folder.
    let find_arg = if cfg!(windows) { "codex.cmd" } else { "codex" };
    if let Some(stub) = which_first(find_arg) {
        if let Some(npm_dir) = stub.parent() {
            codex_pkg_roots.push(npm_dir.join("node_modules").join("@openai").join("codex"));
            // Linux-style global install: /usr/bin/codex →
            // /usr/lib/node_modules/@openai/codex
            if let Some(parent) = npm_dir.parent() {
                codex_pkg_roots.push(
                    parent
                        .join("lib")
                        .join("node_modules")
                        .join("@openai")
                        .join("codex"),
                );
            }
        }
    }

    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA")
            .or_else(|_| std::env::var("LOCALAPPDATA"))
            .ok();
        if let Some(appdata) = appdata {
            if appdata.len() > 2 {
                codex_pkg_roots.push(
                    PathBuf::from(appdata)
                        .join("npm")
                        .join("node_modules")
                        .join("@openai")
                        .join("codex"),
                );
            }
        }
    }

    #[cfg(not(windows))]
    {
        codex_pkg_roots.push(PathBuf::from("/usr/local/lib/node_modules/@openai/codex"));
        codex_pkg_roots.push(PathBuf::from("/usr/lib/node_modules/@openai/codex"));
        if let Some(home) = dirs::home_dir() {
            codex_pkg_roots.push(
                home.join(".npm-global")
                    .join("lib")
                    .join("node_modules")
                    .join("@openai")
                    .join("codex"),
            );
        }
    }

    for pkg_root in &codex_pkg_roots {
        let candidate = pkg_root
            .join("node_modules")
            .join(plat_pkg)
            .join("vendor")
            .join(triple)
            .join("codex")
            .join(exe_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Last-resort CLI fallback: locate the `codex.cmd` / `codex` shim
/// itself. Spawning via the shim works for non-TTY contexts and is
/// strictly better than failing to launch at all.
pub fn resolve_codex_cli_shim() -> Option<PathBuf> {
    let shim = if cfg!(windows) { "codex.cmd" } else { "codex" };

    // Direct file existence in the most common install locations first
    // (cheaper than spawning `where` / `which`).
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA")
            .or_else(|_| std::env::var("LOCALAPPDATA"))
            .ok();
        if let Some(appdata) = appdata {
            if appdata.len() > 2 {
                let candidate = PathBuf::from(appdata).join("npm").join(shim);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let candidate = PathBuf::from("/usr/local/bin").join(shim);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // PATH lookup.
    which_first(shim)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolve the first PATH hit for `program` via the OS's `where` /
/// `which` command. We use the same approach as the .cjs version
/// (spawn the OS utility) rather than the `which` crate so we get
/// identical resolution semantics across the port.
fn which_first(program: &str) -> Option<PathBuf> {
    let (cmd, arg_to_find) = if cfg!(windows) {
        ("where", program)
    } else {
        ("which", program)
    };

    use std::process::{Command, Stdio};
    let mut command = Command::new(cmd);
    command
        .arg(arg_to_find)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // Hide console window on Windows (this gets called from background
    // tasks, including the Tauri main process).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Bound the wait — `which` should be near-instant; `where` on
    // Windows occasionally stalls on broken PATH entries.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let output = loop {
        match child.try_wait() {
            Ok(Some(_status)) => break child.wait_with_output().ok()?,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    };

    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_first_finds_a_universal_command() {
        // Pick the OS's most-portable command. Skip if not found
        // (some sandboxed CI runners strip everything).
        let target = if cfg!(windows) { "cmd.exe" } else { "ls" };
        if let Some(p) = which_first(target) {
            assert!(
                p.to_string_lossy()
                    .to_lowercase()
                    .contains(target.trim_end_matches(".exe")),
                "got: {p:?}"
            );
        }
    }

    #[test]
    fn which_first_returns_none_for_nonexistent_program() {
        let result = which_first("this-program-definitely-does-not-exist-xyzzy-2026");
        assert!(result.is_none(), "got: {result:?}");
    }

    #[test]
    fn resolve_desktop_launch_uri_returns_none_for_missing_file() {
        let result =
            resolve_desktop_launch_uri(std::path::Path::new("/this/path/does/not/exist/anywhere"));
        assert!(result.is_none());
    }

    #[test]
    fn resolve_desktop_launch_uri_reads_uri_from_paths_json() {
        // Build a fake tools dir with codexdesktop/paths.json holding a
        // launchUri. Verify we extract it.
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "echobird_cbr_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let cd_dir = dir.join("codexdesktop");
        std::fs::create_dir_all(&cd_dir).unwrap();
        std::fs::write(
            cd_dir.join("paths.json"),
            r#"{ "launchUri": "shell:AppsFolder\\OpenAI.Codex_xxx!App" }"#,
        )
        .unwrap();

        let got = resolve_desktop_launch_uri(&dir);
        #[cfg(windows)]
        assert_eq!(
            got.as_deref(),
            Some("shell:AppsFolder\\OpenAI.Codex_xxx!App")
        );
        // On non-Windows the function is hardcoded to None.
        #[cfg(not(windows))]
        assert!(got.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolvers_are_callable_without_panicking() {
        // Smoke test: just make sure the resolvers don't panic when
        // Codex isn't installed. They legitimately may return None.
        let _ = resolve_desktop_binary();
        let _ = resolve_codex_cli_binary();
        let _ = resolve_codex_cli_shim();
    }
}
