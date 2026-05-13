// Platform detection and command utilities — mirrors old utils.ts

use std::process::Command;
use std::path::Path;

/// Check if a command exists on PATH
pub async fn command_exists(cmd: &str) -> bool {
    #[cfg(not(target_os = "android"))]
    {
        if which::which(cmd).is_ok() {
            return true;
        }
        shell_command_path(cmd).is_some()
    }
    #[cfg(target_os = "android")]
    {
        let _ = cmd;
        false
    }
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
    {
        let _ = cmd;
        None
    }
}

/// Linux/macOS fallback: query the user's login-shell PATH.
/// Tauri GUI processes inherit a stripped PATH (no rc-file additions like nvm, asdf,
/// conda, ~/.local/bin extensions). Spawning `$SHELL -lc 'command -v <cmd>'` runs the
/// user's login shell, sources their rc files, and reports the real PATH resolution.
///
/// IMPORTANT: This function has a 2-second timeout to prevent hanging the UI if the
/// user's shell config is slow or broken. If the timeout is hit, we return None and
/// fall back to the system PATH.
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

    // Spawn the shell command with a timeout to prevent hanging
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let cmd_clone = cmd.to_string();
    let shell_clone = shell.clone();

    thread::spawn(move || {
        let output = Command::new(&shell_clone)
            .args(["-lc", &format!("command -v {} 2>/dev/null", cmd_clone)])
            .output();
        let _ = tx.send(output);
    });

    // Wait up to 2 seconds for the shell command to complete
    let output = match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            log::warn!("[platform] Shell command failed for '{}': {}", cmd, e);
            return None;
        }
        Err(_) => {
            log::warn!("[platform] Shell command timed out for '{}' (>2s) - user's shell config may be slow", cmd);
            return None;
        }
    };

    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    if line.is_empty() || !std::path::Path::new(line).exists() {
        return None;
    }
    log::info!("[platform] '{}' resolved via login shell: {}", cmd, line);
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
/// Has a 3-second timeout to prevent hanging.
pub async fn python_module_exists(module: &str) -> bool {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let module_clone = module.to_string();

    thread::spawn(move || {
        #[cfg(windows)]
        let result = {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            Command::new("python")
                .args(["-m", &module_clone, "--version"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        };
        #[cfg(not(windows))]
        let result = Command::new("python3")
            .args(["-m", &module_clone, "--version"])
            .output();

        let _ = tx.send(result);
    });

    // Wait up to 3 seconds for the Python module check to complete
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(output)) => output.status.success() || !output.stdout.is_empty(),
        Ok(Err(_)) => false,
        Err(_) => {
            log::warn!(
                "[platform] Python module check timed out for '{}' (>3s)",
                module
            );
            false
        }
    }
}

/// Get command version by running `cmd --version`
/// Has a 3-second timeout to prevent hanging if the command is slow or broken.
pub async fn get_version(cmd: &str) -> Option<String> {
    let resolved_path = get_command_path(cmd).await;

    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let cmd_clone = resolved_path.unwrap_or_else(|| cmd.to_string());

    thread::spawn(move || {
        #[cfg(windows)]
        let output = {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            // Use cmd.exe /C to properly resolve .cmd batch wrappers (npm-installed tools)
            Command::new("cmd")
                .args(["/C", &cmd_clone, "--version"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        };
        #[cfg(not(windows))]
        let output = Command::new(&cmd_clone).arg("--version").output();

        let _ = tx.send(output);
    });

    // Wait up to 3 seconds for the version command to complete
    let output = match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            log::warn!("[platform] Version check failed for '{}': {}", cmd, e);
            return None;
        }
        Err(_) => {
            log::warn!("[platform] Version check timed out for '{}' (>3s)", cmd);
            return None;
        }
    };

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
                        if c.is_ascii_alphabetic() {
                            break;
                        }
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
            .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()) && s.contains('.'))
        {
            return Some(ver.trim().to_string());
        }
        // Strategy 2: find v-prefixed token (e.g. "v0.1.4.post5") → strip the 'v'
        if let Some(ver) = line.split_whitespace().find(|s| {
            s.starts_with('v')
                && s.len() > 1
                && s.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
                && s.contains('.')
        }) {
            return Some(ver[1..].trim().to_string());
        }
    }
    None
}

/// Get version by running an explicit executable path with `--version`.
/// Useful when the GUI process cannot resolve the tool on PATH but detection
/// already found a concrete binary location.
pub async fn get_version_from_path(exe_path: &Path) -> Option<String> {
    if !exe_path.exists() || !exe_path.is_file() {
        return None;
    }

    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let exe = exe_path.to_path_buf();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        #[cfg(windows)]
        let output = {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            Command::new(&exe)
                .arg("--version")
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        };
        #[cfg(not(windows))]
        let output = Command::new(&exe).arg("--version").output();

        let _ = tx.send(output);
    });

    let output = match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            log::warn!(
                "[platform] Version check failed for path '{}': {}",
                exe_path.display(),
                e
            );
            return None;
        }
        Err(_) => {
            log::warn!(
                "[platform] Version check timed out for path '{}' (>3s)",
                exe_path.display()
            );
            return None;
        }
    };

    let combined = {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let raw = format!("{}\n{}", stdout, stderr);
        let mut stripped = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() {
                            break;
                        }
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

    for line in combined.lines() {
        if let Some(ver) = line
            .split_whitespace()
            .find(|s| s.chars().next().is_some_and(|c| c.is_ascii_digit()) && s.contains('.'))
        {
            return Some(ver.trim().to_string());
        }
        if let Some(ver) = line.split_whitespace().find(|s| {
            s.starts_with('v')
                && s.len() > 1
                && s.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
                && s.contains('.')
        }) {
            return Some(ver[1..].trim().to_string());
        }
    }

    None
}

/// Read a macOS app bundle version from the executable path's enclosing app bundle.
/// Prefers CFBundleShortVersionString and falls back to CFBundleVersion.
#[cfg(target_os = "macos")]
pub fn get_macos_bundle_version(exe_path: &Path) -> Option<String> {
    let mut current = exe_path.to_path_buf();

    loop {
        let file_name = current.file_name()?.to_string_lossy();
        if file_name == "MacOS" {
            let contents_dir = current.parent()?;
            let plist_path = contents_dir.join("Info.plist");
            if !plist_path.exists() {
                return None;
            }

            let value = plist::Value::from_file(&plist_path).ok()?;
            let dict = value.as_dictionary()?;

            if let Some(version) = dict
                .get("CFBundleShortVersionString")
                .and_then(|v| v.as_string())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                return Some(version.to_string());
            }

            return dict
                .get("CFBundleVersion")
                .and_then(|v| v.as_string())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }

        if !current.pop() {
            return None;
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_macos_bundle_version(_exe_path: &Path) -> Option<String> {
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
