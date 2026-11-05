//! Bounded background process output, including owned-tree cleanup on cancellation.
struct CommandProbe {
    child: tokio::process::Child,
    pid: u32,
    completed: bool,
}

impl Drop for CommandProbe {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        #[cfg(windows)]
        if let Some(handle) = self.child.raw_handle() {
            let handle = unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(handle) };
            if let Ok((created, exited)) =
                crate::services::process_manager::owned_windows_process_times(&handle)
            {
                let _ = crate::services::process_manager::stop_owned_windows_tree(
                    self.pid, created, exited,
                );
            }
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGKILL);
        }
        let _ = self.child.start_kill();
    }
}

pub(super) async fn version_output(
    command: std::process::Command,
    timeout: std::time::Duration,
) -> std::io::Result<std::process::Output> {
    use std::process::Stdio;
    #[cfg(unix)]
    let command = {
        use std::os::unix::process::CommandExt;
        let mut command = command;
        command.process_group(0);
        command
    };
    let mut command = tokio::process::Command::from(command);
    let child = command
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child = CommandProbe {
        pid: child.id().unwrap(),
        child,
        completed: false,
    };
    let stdout = child.child.stdout.take().unwrap();
    let stderr = child.child.stderr.take().unwrap();
    let result = tokio::time::timeout(timeout, async {
        // Keep the leader's identity until its inherited pipes are closed too.
        let (out, err) = tokio::try_join!(read_limited(stdout), read_limited(stderr))?;
        let status = child.child.wait().await?;
        Ok(std::process::Output {
            status,
            stdout: out,
            stderr: err,
        })
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "command probe timed out"))?;
    child.completed = result.is_ok();
    result
}

// Profiles and version wrappers are user-controlled processes. Bound both pipes
// independently so a noisy probe cannot allocate indefinitely before its timeout.
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

async fn read_limited(reader: impl tokio::io::AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    reader
        .take((MAX_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "command probe output exceeded limit",
        ));
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    #[tokio::test]
    async fn rejects_excessive_output_and_accepts_normal_probes() {
        let mut normal = std::process::Command::new("sh");
        normal.args(["-c", "printf fixture"]);
        let output = super::version_output(normal, std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"fixture");
        for redirect in ["", " >&2"] {
            let mut noisy = std::process::Command::new("sh");
            noisy.args([
                "-c",
                &format!(
                    "head -c {} /dev/zero{redirect}; sleep 30",
                    super::MAX_OUTPUT_BYTES + 1
                ),
            ]);
            let error = super::version_output(noisy, std::time::Duration::from_secs(5))
                .await
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        }
    }
}
