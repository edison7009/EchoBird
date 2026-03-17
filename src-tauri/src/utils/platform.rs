// Platform detection and command utilities �?mirrors old utils.ts

use std::process::Command;

/// Supported platforms
#[derive(Debug, Clone, PartialEq)]
pub enum Platform {
    Windows,
    Mac,
    Linux,
}

/// Detect the current platform
pub fn get_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Mac
    } else {
        Platform::Linux
    }
}

/// Check if a command exists on PATH
pub async fn command_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// Get the full path of a command
pub async fn get_command_path(cmd: &str) -> Option<String> {
    which::which(cmd)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
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
        format!("{}\n{}", stdout, stderr)
    };

    // Scan all lines for a version pattern (digits.digits...)
    for line in combined.lines() {
        if let Some(ver) = line
            .split_whitespace()
            .find(|s| s.chars().next().map_or(false, |c| c.is_ascii_digit()) && s.contains('.'))
        {
            return Some(ver.trim().to_string());
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
pub fn echobird_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
}
