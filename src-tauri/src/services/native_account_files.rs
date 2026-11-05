use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("accountError.home".into());
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            component => result.push(component.as_os_str()),
        }
    }
    Ok(result)
}

pub(crate) fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|_| "accountError.write")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "accountError.write")?;
    }
    Ok(())
}

pub(crate) struct DirectoryLock {
    _lock: super::json_store::FileLock,
}
impl DirectoryLock {
    pub(crate) fn acquire(anchor: &Path) -> Result<Self, String> {
        private_dir(anchor.parent().ok_or("accountError.home")?)?;
        Self::acquire_existing(anchor)
    }
    pub(crate) fn acquire_existing(anchor: &Path) -> Result<Self, String> {
        if !anchor.parent().is_some_and(Path::is_dir) {
            return Err("accountError.home".into());
        }
        // Respect legacy locks without unlinking an older client's lock directory.
        if fs::symlink_metadata(PathBuf::from(format!("{}.lock", anchor.display()))).is_ok() {
            return Err("accountError.busy".into());
        }
        // Native clients may use their own .lock sidecars; keep our inode distinct.
        let lock = PathBuf::from(format!("{}.echobird-lock", anchor.display()));
        let lock =
            super::json_store::FileLock::acquire(&lock, false).map_err(|_| "accountError.busy")?;
        Ok(Self { _lock: lock })
    }
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("accountError.read".into()),
    }
}
pub(crate) fn object(path: &Path) -> Result<Value, String> {
    let value: Value = match read_optional(path)? {
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| "accountError.format")?,
        None => serde_json::json!({}),
    };
    value
        .is_object()
        .then_some(value)
        .ok_or_else(|| "accountError.format".into())
}
pub(crate) fn json(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value).map_err(|_| "accountError.format".into())
}
pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    private_dir(path.parent().ok_or("accountError.home")?)?;
    write_synced(path, bytes, fs::File::sync_all)
}

pub(crate) fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    // Preserve an existing native configuration's symbolic-link destination.
    let mut target = path.to_path_buf();
    for _ in 0..40 {
        if !fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return write_synced(&target, bytes, fs::File::sync_all);
        }
        let link = fs::read_link(&target).map_err(|_| "accountError.write")?;
        target = if link.is_absolute() {
            link
        } else {
            target.parent().ok_or("accountError.write")?.join(link)
        };
    }
    Err("accountError.write|Too many symbolic links".into())
}

fn write_synced(
    path: &Path,
    bytes: &[u8],
    sync: impl FnOnce(&fs::File) -> std::io::Result<()>,
) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("accountError.home")?)
        .map_err(|_| "accountError.write")?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|_| "accountError.write")?;
        file.write_all(bytes).map_err(|_| "accountError.write")?;
        sync(&file).map_err(|_| "accountError.write")?;
        drop(file);
        replace(&temporary, path).map_err(|_| "accountError.write".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn replace(temporary: &Path, path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match fs::rename(temporary, path) {
                Err(error)
                    if matches!(error.raw_os_error(), Some(5 | 32 | 33))
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => return result,
            }
        }
    }
    #[cfg(not(windows))]
    fs::rename(temporary, path)
}

pub(crate) struct Transaction {
    original: Vec<(PathBuf, Option<Vec<u8>>, bool)>,
    committed: bool,
}
impl Transaction {
    pub(crate) fn new() -> Self {
        Self {
            original: Vec::new(),
            committed: false,
        }
    }
    pub(crate) fn replace(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.replace_with(path, bytes, false)
    }
    pub(crate) fn replace_private_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.replace_with(path, bytes, true)
    }
    pub(crate) fn remove(&mut self, path: &Path) -> Result<(), String> {
        if let Some(bytes) = read_optional(path)? {
            fs::remove_file(path).map_err(|_| "accountError.write")?;
            self.original.push((path.to_path_buf(), Some(bytes), false));
        }
        Ok(())
    }
    fn replace_with(
        &mut self,
        path: &Path,
        bytes: &[u8],
        preserve_link: bool,
    ) -> Result<(), String> {
        let old = read_optional(path)?;
        if preserve_link {
            write_private_file(path, bytes)?;
        } else {
            write(path, bytes)?;
        }
        self.original.push((path.to_path_buf(), old, preserve_link));
        Ok(())
    }
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, original, preserve_link) in self.original.iter().rev() {
            let restored = match original {
                Some(bytes) if *preserve_link => write_private_file(path, bytes),
                Some(bytes) => write(path, bytes),
                None => fs::remove_file(path).map_err(|_| "accountError.write".into()),
            };
            if restored.is_err() {
                log::error!("Failed to restore native account configuration");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_process_fixture() {
        let Some(root) = std::env::var_os("ECHOBIRD_NATIVE_LOCK_FIXTURE") else {
            return;
        };
        let root = PathBuf::from(root);
        let _guard = DirectoryLock::acquire(&root.join("config")).unwrap();
        fs::write(root.join("ready"), b"locked").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn process_exit_releases_native_lock_without_unlinking_another_writers_inode() {
        let root = std::env::temp_dir().join(format!("echobird-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            crate::utils::process::command(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "services::native_account_files::tests::lock_process_fixture",
                    "--nocapture",
                ])
                .env("ECHOBIRD_NATIVE_LOCK_FIXTURE", &root)
                .spawn()
                .unwrap(),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !root.join("ready").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            root.join("ready").exists(),
            "lock fixture must acquire the real lock"
        );
        assert!(DirectoryLock::acquire(&root.join("config")).is_err());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let recovered = DirectoryLock::acquire(&root.join("config")).unwrap();
        assert!(DirectoryLock::acquire(&root.join("config")).is_err());
        drop(recovered);
        assert!(root.join("config.echobird-lock").is_file());
        fs::create_dir(root.join("legacy.lock")).unwrap();
        assert!(DirectoryLock::acquire(&root.join("legacy")).is_err());
        assert!(root.join("legacy.lock").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_rollback_preserves_its_link_and_parent_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("echobird-linked-rollback-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o750)).unwrap();
        let target = root.join("actual.json");
        let path = root.join("settings.json");
        fs::write(&target, b"original").unwrap();
        std::os::unix::fs::symlink("actual.json", &path).unwrap();
        {
            let mut transaction = Transaction::new();
            transaction.replace_private_file(&path, b"applied").unwrap();
            assert_eq!(fs::read(&target).unwrap(), b"applied");
            assert!(transaction
                .replace_private_file(&target.join("invalid"), b"fail")
                .is_err());
        }
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o750
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_flush_keeps_the_original_and_removes_only_its_temporary_file() {
        let root = std::env::temp_dir().join(format!("echobird-flush-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        fs::write(&path, b"original").unwrap();
        let result = write_synced(&path, b"replacement", |file| {
            assert_eq!(fs::read(&path).unwrap(), b"original");
            assert_eq!(file.metadata().unwrap().len(), 11);
            assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
            Err(std::io::Error::other("fixture flush failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        write_synced(&path, b"replacement", |file| {
            assert_eq!(fs::read(&path).unwrap(), b"original");
            file.sync_all()
        })
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_paths_normalize_dot_segments_without_resolving_symbolic_links() {
        let home = std::env::temp_dir();
        assert_eq!(
            absolute_path(&home.join("data/../account/./auth")).unwrap(),
            home.join("account/auth")
        );
        assert!(absolute_path(Path::new("relative/accounts")).is_err());
        #[cfg(unix)]
        {
            assert_eq!(
                absolute_path(Path::new("/../../accounts")).unwrap(),
                PathBuf::from("/accounts")
            );
        }
    }
    #[test]
    fn failed_multi_file_update_restores_prior_account_and_releases_lock() {
        let root = std::env::temp_dir().join(format!(
            "echobird-native-transaction-{}",
            uuid::Uuid::new_v4()
        ));
        let first = root.join("user");
        write(&first, b"original").unwrap();
        let guard = DirectoryLock::acquire(&root.join("auth.lock")).unwrap();
        assert!(DirectoryLock::acquire(&root.join("auth.lock")).is_err());
        {
            let mut transaction = Transaction::new();
            transaction.replace(&first, b"new").unwrap();
            transaction.replace(&root.join("new-user"), b"new").unwrap();
            assert!(transaction.replace(&first.join("invalid"), b"new").is_err());
        }
        assert_eq!(fs::read(&first).unwrap(), b"original");
        assert!(!root.join("new-user").exists());
        drop(guard);
        let _guard = DirectoryLock::acquire(&root.join("auth.lock")).unwrap();
        drop(_guard);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn committed_multi_file_update_is_private() {
        let root =
            std::env::temp_dir().join(format!("echobird-native-commit-{}", uuid::Uuid::new_v4()));
        let file = root.join("user");
        let mut transaction = Transaction::new();
        transaction.replace(&file, b"saved").unwrap();
        transaction.commit();
        assert_eq!(fs::read(&file).unwrap(), b"saved");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
