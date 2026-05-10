// Platform detection and command utilities — mirrors old utils.ts

use std::process::Command;

/// Check if a command exists on PATH
pub async fn command_exists(cmd: &str) -> bool {
    #[cfg(not(target_os = "android"))]
    {
        if which::which(cmd).is_ok() { return true; }
        shell_command_path(cmd).is_some()
    }
    #[cfg(target_os = "android")]
    { let _ = cmd; false }
}

/// Get the full path of a command
pub async fn get_command_path(cmd: &str) -> Option<String> {
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(p) = which::which(cmd) {
            return Some(p.to_string_lossy().to_string());
        }
        shell_command_path(cmd)
    }
    #[cfg(target_os = "android")]
    { let _ = cmd; None }
}

/// Linux/macOS fallback: query the user's login-shell PATH.
/// Tauri GUI processes inherit a stripped PATH (no rc-file additions like nvm, asdf,
/// conda, ~/.local/bin extensions). Spawning `$SHELL -lc 'command -v <cmd>'` runs the
/// user's login shell, sources their rc files, and reports the real PATH resolution.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn shell_command_path(cmd: &str) -> Option<String> {
    // Defense-in-depth: only forward simple command names into the shell
    if cmd.is_empty()
        || !cmd
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let output = Command::new(&shell)
        .args(["-lc", &format!("command -v {} 2>/dev/null", cmd)])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    if line.is_empty() || !std::path::Path::new(line).exists() {
        return None;
    }
    log::info!(
        "[platform] '{}' resolved via login shell: {}",
        cmd, line
    );
    Some(line.to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[allow(dead_code)]
fn shell_command_path(_cmd: &str) -> Option<String> {
    // Windows: GUI apps inherit the system PATH via the registry, so `which` is
    // already authoritative. No fallback needed.
    None
}

/// Check if a Python module is installed (pip-installed tools like nanobot)
/// Runs `python -m {module} --version` and checks exit code
pub async fn python_module_exists(module: &str) -> bool {
    #[cfg(windows)]
    let result = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("python")
            .args(["-m", module, "--version"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };
    #[cfg(not(windows))]
    let result = Command::new("python3")
        .args(["-m", module, "--version"])
        .output();

    match result {
        Ok(output) => output.status.success() || !output.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Get command version by running `cmd --version`
pub async fn get_version(cmd: &str) -> Option<String> {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // Use cmd.exe /C to properly resolve .cmd batch wrappers (npm-installed tools)
        Command::new("cmd")
            .args(["/C", cmd, "--version"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?
    };
    #[cfg(not(windows))]
    let output = Command::new(cmd)
        .arg("--version")
        .output()
        .ok()?;

    // Check both stdout and stderr — some tools print version to stderr
    // Don't require success exit code — some tools return non-zero with --version
    let combined = {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let raw = format!("{}\n{}", stdout, stderr);
        // Strip ANSI escape codes (PicoClaw etc. output colored banners)
        let mut stripped = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() { break; }
                    }
                    continue;
                }
                chars.next();
                continue;
            }
            stripped.push(ch);
        }
        stripped
    };

    // Scan all lines for a version pattern (digits.digits... or v-prefixed)
    for line in combined.lines() {
        // Strategy 1: find token starting with digit and containing '.' (e.g. "0.4.9")
        if let Some(ver) = line
            .split_whitespace()
            .find(|s| s.chars().next().map_or(false, |c| c.is_ascii_digit()) && s.contains('.'))
        {
            return Some(ver.trim().to_string());
        }
        // Strategy 2: find v-prefixed token (e.g. "v0.1.4.post5") → strip the 'v'
        if let Some(ver) = line
            .split_whitespace()
            .find(|s| s.starts_with('v') && s.len() > 1 && s.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) && s.contains('.'))
        {
            return Some(ver[1..].trim().to_string());
        }
    }
    None
}

/// Get user home directory
pub fn home_dir() -> String {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Get Echobird config directory (~/.echobird/)
/// On Android, dirs::home_dir() returns None, so we use the app's internal data dir.
pub fn echobird_dir() -> std::path::PathBuf {
    // Try standard home dir first (Windows / macOS / Linux desktop)
    if let Some(home) = dirs::home_dir() {
        return home.join(".echobird");
    }
    // Android fallback: use app internal storage
    // Tauri sets TMPDIR to the app's cache dir; derive files dir from it
    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        // TMPDIR is typically /data/data/com.echobird.ai/cache
        // We want /data/data/com.echobird.ai/files/.echobird
        let path = std::path::Path::new(&tmpdir);
        if let Some(parent) = path.parent() {
            return parent.join("files").join(".echobird");
        }
    }
    // Last resort: use /data/local/tmp (unlikely to reach here)
    std::path::PathBuf::from("/data/local/tmp/.echobird")
}
