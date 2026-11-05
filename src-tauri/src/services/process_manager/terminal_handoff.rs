//! Private, single-use launch files for terminals that cannot inherit our environment.
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

const EXPIRY: Duration = Duration::from_secs(10 * 60);

pub(super) struct Handoff {
    directory: PathBuf,
    retained: bool,
}

impl Handoff {
    pub(super) fn create(cache: &Path, body: &str) -> Result<Self, String> {
        let root = cache.join("terminal-handoffs");
        crate::services::native_account_files::private_dir(&root)?;
        cleanup_expired(&root);
        let directory = root.join(uuid::Uuid::new_v4().to_string());
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|e| e.to_string())?;
        let handoff = Self {
            directory,
            retained: false,
        };
        let payload = handoff.directory.join("payload.sh");
        let launcher = handoff.launcher();
        let quote = |path: &Path| super::shell_single_quote(&path.to_string_lossy());
        // The shell keeps the payload's open descriptor after unlinking these paths.
        // Remove credentials before the tool starts, including failed tool launches.
        let script = format!(
            "#!/bin/sh\nrm -f -- {} {}\nrmdir -- {}\n{}",
            quote(&payload),
            quote(&launcher),
            quote(&handoff.directory),
            body
        );
        write_new(&payload, script.as_bytes(), 0o600)?;
        // Terminal.app requires an executable command file; it contains no credentials.
        write_new(
            &launcher,
            format!("#!/bin/sh\nexec sh {}\n", quote(&payload)).as_bytes(),
            0o700,
        )?;
        Ok(handoff)
    }

    pub(super) fn launcher(&self) -> PathBuf {
        self.directory.join("launch.command")
    }

    pub(super) fn retain(mut self) {
        self.retained = true;
        let directory = self.directory.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(EXPIRY).await;
            cleanup(&directory);
        });
    }
}

impl Drop for Handoff {
    fn drop(&mut self) {
        if !self.retained {
            cleanup(&self.directory);
        }
    }
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

fn cleanup(directory: &Path) {
    for name in ["payload.sh", "launch.command"] {
        let _ = fs::remove_file(directory.join(name));
    }
    let _ = fs::remove_dir(directory);
}

fn cleanup_expired(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err() {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if metadata.is_dir()
            && metadata
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age >= EXPIRY)
        {
            cleanup(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn delayed_launches_are_private_isolated_and_self_remove_before_executing() {
        let root = std::env::temp_dir().join(format!("echobird-handoff-{}", uuid::Uuid::new_v4()));
        let first = Handoff::create(&root, "printf '%s' 'first secret'\n").unwrap();
        let second = Handoff::create(&root, "printf '%s' 'second secret'\n").unwrap();
        assert_ne!(first.launcher(), second.launcher());
        for (handoff, expected) in [(&second, "second secret"), (&first, "first secret")] {
            let payload = handoff.directory.join("payload.sh");
            assert_eq!(
                fs::metadata(&handoff.directory)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&payload).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert!(!fs::read_to_string(handoff.launcher())
                .unwrap()
                .contains(expected));
            let output = crate::utils::process::command("sh")
                .arg(handoff.launcher())
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
            assert!(!handoff.directory.exists());
        }
        let abandoned = Handoff::create(&root, "exit 1\n").unwrap();
        let path = abandoned.directory.clone();
        drop(abandoned);
        assert!(!path.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
