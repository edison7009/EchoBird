// Platform detection and command utilities.
mod probe;
use probe::version_output;

/// Check if a command exists on PATH
pub async fn command_exists(cmd: &str) -> bool {
    get_command_path(cmd).await.is_some()
}

/// Resolve once instead of probing existence and starting the shell again for its path.
pub async fn get_command_path(cmd: &str) -> Option<String> {
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(path) = which::which(cmd) {
            return Some(path.to_string_lossy().into_owned());
        }
        shell_command_path(cmd).await
    }
    #[cfg(target_os = "android")]
    {
        let _ = cmd;
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn shell_command_path(cmd: &str) -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    shell_command_path_with(cmd, &shell, std::time::Duration::from_secs(2)).await
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn shell_command_path_with(
    cmd: &str,
    shell: &str,
    timeout: std::time::Duration,
) -> Option<String> {
    if cmd.is_empty()
        || !cmd
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return None;
    }
    // A scan may contain many missing tools. Waiting here does not occupy Tokio workers.
    static PROBES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let _permit = PROBES.acquire().await.ok()?;
    let mut command = crate::utils::process::command(shell);
    command.args(["-lc", &format!("command -v {cmd} 2>/dev/null")]);
    let output = match version_output(command, timeout).await {
        Ok(output) => output,
        Err(error) => {
            log::warn!("[platform] Shell resolution failed for '{cmd}': {error}");
            return None;
        }
    };
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    (!line.is_empty() && std::path::Path::new(line).is_file()).then(|| line.to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[allow(dead_code)]
async fn shell_command_path(_cmd: &str) -> Option<String> {
    None
}

/// Check a pip-installed CLI without leaving a timed-out Python process behind.
pub async fn python_module_exists(module: &str) -> bool {
    #[cfg(windows)]
    let python = "python";
    #[cfg(not(windows))]
    let python = "python3";
    let mut command = crate::utils::process::command(python);
    command.args(["-m", module, "--version"]);
    version_output(command, std::time::Duration::from_secs(3))
        .await
        .is_ok_and(|output| output.status.success() || !output.stdout.is_empty())
}

/// Get command version by running `cmd --version`
/// Has a 3-second timeout to prevent hanging if the command is slow or broken.
pub async fn get_version(cmd: &str) -> Option<String> {
    use std::time::Duration;
    let output = match version_output(version_command(cmd), Duration::from_secs(3)).await {
        Ok(output) => output,
        Err(e) => {
            log::warn!("[platform] Version check failed for '{}': {}", cmd, e);
            return None;
        }
    };

    // Check both stdout and stderr — some tools print version to stderr
    // Don't require success exit code — some tools return non-zero with --version
    let combined = {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let raw = format!("{}\n{}", stdout, stderr);
        // Strip ANSI escape codes (some CLIs output colored banners)
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

fn version_command(cmd: &str) -> std::process::Command {
    #[cfg(windows)]
    let mut command = {
        let mut command = crate::utils::process::command("cmd");
        use std::os::windows::process::CommandExt;
        command.args(["/D", "/S", "/C"]);
        command.raw_arg(format!("\"\"{cmd}\" --version\""));
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = crate::utils::process::command(cmd);
        command.arg("--version");
        command
    };
    if std::path::Path::new(cmd)
        .file_stem()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("cline"))
    {
        command.env("CLINE_NO_AUTO_UPDATE", "1");
    }
    command
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

#[cfg(test)]
mod version_tests {
    use std::{fs, path::Path, time::Duration};

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn shell_resolution_is_nonblocking_bounded_and_kills_timed_out_children() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("echobird-shell-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script = root.join("shell");
        fs::write(&script, "#!/bin/sh\nprintf '/bin/sh\\n'\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            super::shell_command_path_with(
                "fixture",
                script.to_str().unwrap(),
                Duration::from_secs(1)
            )
            .await
            .as_deref(),
            Some("/bin/sh")
        );
        assert!(super::shell_command_path_with(
            "fixture;exit",
            script.to_str().unwrap(),
            Duration::from_secs(1)
        )
        .await
        .is_none());
        let pid = root.join("child.pid");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
                pid.display()
            ),
        )
        .unwrap();
        let (result, tick) = tokio::join!(
            super::shell_command_path_with(
                "fixture",
                script.to_str().unwrap(),
                Duration::from_secs(1)
            ),
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                std::time::Instant::now()
            }
        );
        assert!(result.is_none());
        assert!(
            tick.elapsed() >= Duration::from_millis(50),
            "runtime timer must run while the shell is pending"
        );
        let child: u32 = fs::read_to_string(pid).unwrap().trim().parse().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while running(child) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!running(child));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cline_version_probe_disables_automatic_updates_without_changing_other_tools() {
        use std::ffi::OsStr;
        for command in ["cline", "cline.cmd", "cline.exe", "/fixture/bin/cline"] {
            let probe = super::version_command(command);
            let value = probe.get_envs().find_map(|(name, value)| {
                (name == OsStr::new("CLINE_NO_AUTO_UPDATE")).then_some(value)
            });
            assert_eq!(value, Some(Some(OsStr::new("1"))), "{command}");
        }
        assert!(!super::version_command("codex")
            .get_envs()
            .any(|(name, _)| name == OsStr::new("CLINE_NO_AUTO_UPDATE")));
    }

    #[tokio::test]
    async fn cline_version_probe_runs_only_its_isolated_wrapper_with_spaces() {
        let root = std::env::temp_dir().join(format!("echobird version {}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        #[cfg(windows)]
        let executable = {
            let path = root.join("cline.cmd");
            fs::write(&path, "@echo off\r\nif not \"%~1\"==\"--version\" exit /b 2\r\nif \"%CLINE_NO_AUTO_UPDATE%\"==\"1\" (echo cline v9.8.7) else (echo 0.0.0)\r\n").unwrap();
            path
        };
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            let path = root.join("cline");
            fs::write(&path, "#!/bin/sh\nif [ \"$1\" = --version ] && [ \"$CLINE_NO_AUTO_UPDATE\" = 1 ]; then echo 'cline v9.8.7'; else echo '0.0.0'; fi\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let version = super::get_version(executable.to_str().unwrap()).await;
        fs::remove_dir_all(root).unwrap();
        assert_eq!(version.as_deref(), Some("9.8.7"));
    }

    fn fixture_command(root: &Path, role: &str) -> std::process::Command {
        let mut command = crate::utils::process::command(std::env::current_exe().unwrap());
        command
            .args([
                "utils::platform::version_tests::version_probe_fixture_process",
                "--exact",
                "--nocapture",
            ])
            .env("ECHOBIRD_VERSION_FIXTURE_DIR", root)
            .env("ECHOBIRD_VERSION_FIXTURE_ROLE", role);
        command
    }

    #[test]
    fn version_probe_fixture_process() {
        let Some(root) = std::env::var_os("ECHOBIRD_VERSION_FIXTURE_DIR") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let role = std::env::var("ECHOBIRD_VERSION_FIXTURE_ROLE").unwrap();
        let _child = matches!(role.as_str(), "parent" | "exiting-parent")
            .then(|| fixture_command(&root, "child").spawn().unwrap());
        let marker = if role == "exiting-parent" {
            "parent"
        } else {
            &role
        };
        fs::write(
            root.join(format!("{marker}.pid")),
            std::process::id().to_string(),
        )
        .unwrap();
        if role == "exiting-parent" {
            while !root.join("parent.exit").exists() {
                std::thread::sleep(Duration::from_millis(10));
            }
            return;
        }
        loop {
            std::thread::park();
        }
    }

    fn running(pid: u32) -> bool {
        #[cfg(windows)]
        {
            let output = crate::utils::process::command("tasklist.exe")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).contains(&format!("\",\"{pid}\","))
        }
        #[cfg(unix)]
        {
            let output = crate::utils::process::command("/bin/ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8_lossy(&output.stdout);
            !state.trim().is_empty() && !state.trim_start().starts_with('Z')
        }
    }

    struct FixtureChild(std::process::Child);

    impl Drop for FixtureChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[tokio::test]
    async fn version_probe_timeout_and_cancellation_stop_the_owned_process_tree() {
        for (cancel, parent_exits) in [(false, false), (true, false), (false, true), (true, true)] {
            let root = std::env::temp_dir()
                .join(format!("echobird-version-owned-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let mut unowned = FixtureChild(
                fixture_command(&root, "unowned")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let probe = tokio::spawn(super::version_output(
                fixture_command(
                    &root,
                    if parent_exits {
                        "exiting-parent"
                    } else {
                        "parent"
                    },
                ),
                Duration::from_secs(if cancel {
                    30
                } else if parent_exits {
                    5
                } else {
                    3
                }),
            ));
            let ready = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let parent = fs::read_to_string(root.join("parent.pid"))
                        .ok()
                        .and_then(|s| s.parse::<u32>().ok());
                    let child = fs::read_to_string(root.join("child.pid"))
                        .ok()
                        .and_then(|s| s.parse::<u32>().ok());
                    if let (Some(parent), Some(child)) = (parent, child) {
                        break (parent, child);
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
            if ready.is_err() {
                probe.abort();
                let _ = probe.await;
                panic!("isolated version fixture did not start");
            }
            let (parent, child) = ready.unwrap();
            assert!(running(parent), "parent positive control did not match");
            assert!(running(child), "child positive control did not match");
            assert!(unowned.0.try_wait().unwrap().is_none());
            if parent_exits {
                fs::write(root.join("parent.exit"), "exit").unwrap();
                tokio::time::timeout(Duration::from_secs(2), async {
                    while running(parent) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("fixture parent did not exit before cancellation/timeout");
                assert!(
                    running(child),
                    "inherited pipe holder positive control did not match"
                );
            }
            if cancel {
                probe.abort();
                assert!(probe.await.unwrap_err().is_cancelled());
            } else {
                assert_eq!(
                    probe.await.unwrap().unwrap_err().kind(),
                    std::io::ErrorKind::TimedOut
                );
            }
            let stopped = tokio::time::timeout(Duration::from_secs(3), async {
                while running(parent) || running(child) {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            assert!(
                unowned.0.try_wait().unwrap().is_none(),
                "cleanup stopped an unowned process"
            );
            drop(unowned);
            fs::remove_dir_all(root).unwrap();
            assert!(
                stopped.is_ok(),
                "owned fixture survived timeout/cancellation"
            );
        }
    }
}
