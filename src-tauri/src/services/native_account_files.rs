use serde_json::Value;
use sha2::{Digest, Sha256};
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
    let target = private_file_target(path)?;
    write_synced(&target, bytes, fs::File::sync_all)
}

fn private_file_target(path: &Path) -> Result<PathBuf, String> {
    // Preserve an existing native configuration's symbolic-link destination.
    let mut target = path.to_path_buf();
    for _ in 0..40 {
        if !fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Ok(target);
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
    removed_links: Vec<(PathBuf, PathBuf)>,
    published: Vec<Option<Vec<u8>>>,
    guarded: bool,
    committed: bool,
    journal: Option<FileMutationJournal>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FileMutation {
    path: PathBuf,
    parent: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<String>,
}

impl FileMutation {
    fn still_regular_in_original_directory(&self) -> Result<bool, String> {
        Ok(regular_or_missing(&self.path)?
            && self
                .path
                .parent()
                .and_then(|parent| fs::canonicalize(parent).ok())
                .as_ref()
                == Some(&self.parent))
    }
}

struct FileMutationJournal {
    path: PathBuf,
    allowed: Vec<PathBuf>,
    files: Vec<FileMutation>,
}

const RECOVERY_CONFLICT: &str = "accountError.configConflict";

fn regular_or_missing(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err("accountError.read".into()),
    }
}

fn fingerprint(bytes: Option<&[u8]>) -> Option<String> {
    bytes.map(|bytes| hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn has_file_mutations(journal: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(journal) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(RECOVERY_CONFLICT.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("accountError.read".into()),
    }
}

/// Call under the same selection lock used by the original transaction.
pub(crate) fn recover_file_mutations(journal: &Path, allowed: &[&Path]) -> Result<(), String> {
    if !has_file_mutations(journal)? {
        return Ok(());
    }
    let bytes = fs::read(journal).map_err(|_| "accountError.read")?;
    let files: Vec<FileMutation> = serde_json::from_slice(&bytes).map_err(|_| RECOVERY_CONFLICT)?;
    for file in &files {
        if !allowed.contains(&file.path.as_path()) || !file.still_regular_in_original_directory()? {
            return Err(RECOVERY_CONFLICT.into());
        }
    }
    let mut conflict = false;
    for file in files.iter().rev() {
        let current = read_optional(&file.path)?;
        if current == file.before {
            continue;
        }
        if !file.still_regular_in_original_directory()? {
            return Err(RECOVERY_CONFLICT.into());
        }
        if fingerprint(current.as_deref()) != file.after {
            conflict = true;
            continue;
        }
        match &file.before {
            Some(bytes) => write_private_file(&file.path, bytes)?,
            None => fs::remove_file(&file.path).map_err(|_| "accountError.write")?,
        }
    }
    fs::remove_file(journal).map_err(|_| "accountError.write")?;
    if conflict {
        return Err(RECOVERY_CONFLICT.into());
    }
    Ok(())
}

impl Transaction {
    pub(crate) fn new() -> Self {
        Self {
            original: Vec::new(),
            removed_links: Vec::new(),
            published: Vec::new(),
            guarded: false,
            committed: false,
            journal: None,
        }
    }
    /// Leave externally edited files alone when rolling back this operation.
    pub(crate) fn guarded() -> Self {
        let mut transaction = Self::new();
        transaction.guarded = true;
        transaction
    }

    /// Persistent rollback for a file-only update. Keep the existing in-process
    /// rollback for native configurations whose files are symbolic links.
    pub(crate) fn recoverable(journal: &Path, paths: &[&Path]) -> Result<Self, String> {
        let mut transaction = Self::guarded();
        for path in paths {
            if !regular_or_missing(path)? {
                return Ok(transaction);
            }
        }
        transaction.journal = Some(FileMutationJournal {
            path: journal.to_path_buf(),
            allowed: paths.iter().map(|path| path.to_path_buf()).collect(),
            files: Vec::new(),
        });
        Ok(transaction)
    }

    fn record(
        &mut self,
        path: &Path,
        before: Option<&[u8]>,
        after: Option<&[u8]>,
    ) -> Result<(), String> {
        let Some(journal) = &mut self.journal else {
            return Ok(());
        };
        if !journal.allowed.iter().any(|allowed| allowed == path)
            || !regular_or_missing(path)?
            || journal.files.iter().any(|file| file.path == path)
        {
            return Err(RECOVERY_CONFLICT.into());
        }
        let parent = path.parent().ok_or("accountError.home")?;
        fs::create_dir_all(parent).map_err(|_| "accountError.write")?;
        journal.files.push(FileMutation {
            path: path.to_path_buf(),
            parent: fs::canonicalize(parent).map_err(|_| "accountError.home")?,
            before: before.map(<[u8]>::to_vec),
            after: fingerprint(after),
        });
        let bytes = serde_json::to_vec(&journal.files).map_err(|_| "accountError.format")?;
        write(&journal.path, &bytes)?;
        if !journal
            .files
            .last()
            .expect("just recorded")
            .still_regular_in_original_directory()?
            || read_optional(path)?.as_deref() != before
        {
            return Err(RECOVERY_CONFLICT.into());
        }
        Ok(())
    }
    pub(crate) fn replace(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.replace_with(path, bytes, false)
    }
    pub(crate) fn replace_private_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.replace_with(path, bytes, true)
    }
    pub(crate) fn remove(&mut self, path: &Path) -> Result<(), String> {
        self.remove_with(path, false)
    }
    pub(crate) fn remove_private_file(&mut self, path: &Path) -> Result<(), String> {
        self.remove_with(path, true)
    }
    fn remove_with(&mut self, path: &Path, preserve_link: bool) -> Result<(), String> {
        let old = read_optional(path)?;
        if old.is_some() {
            self.record(path, old.as_deref(), None)?;
            if preserve_link
                && fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                if self.journal.is_some() {
                    return Err(RECOVERY_CONFLICT.into());
                }
                // Keep the link itself in the original directory so a relative
                // destination stays valid and rollback needs no symlink privileges.
                let saved = path.with_extension(format!("{}.rollback-link", uuid::Uuid::new_v4()));
                fs::rename(path, &saved).map_err(|_| "accountError.write")?;
                self.removed_links.push((path.to_path_buf(), saved));
                return Ok(());
            }
            fs::remove_file(path).map_err(|_| "accountError.write")?;
            self.original.push((path.to_path_buf(), old, preserve_link));
            self.published.push(None);
        }
        Ok(())
    }
    fn replace_with(
        &mut self,
        path: &Path,
        bytes: &[u8],
        preserve_link: bool,
    ) -> Result<(), String> {
        let target = if preserve_link {
            private_file_target(path)?
        } else {
            path.to_path_buf()
        };
        if self.journal.is_some() && target != path {
            return Err(RECOVERY_CONFLICT.into());
        }
        let old = read_optional(&target)?;
        self.record(path, old.as_deref(), Some(bytes))?;
        if preserve_link {
            write_synced(&target, bytes, fs::File::sync_all)?;
        } else {
            write(path, bytes)?;
        }
        self.original.push((target, old, preserve_link));
        self.published.push(Some(bytes.to_vec()));
        Ok(())
    }
    pub(crate) fn commit(mut self) {
        debug_assert!(self.journal.is_none());
        self.committed = true;
    }
    pub(crate) fn commit_files(mut self) -> Result<(), String> {
        if let Some(journal) = &self.journal {
            if !journal.files.is_empty() {
                fs::remove_file(&journal.path).map_err(|_| "accountError.write")?;
            }
        }
        self.committed = true;
        Ok(())
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        for (path, saved) in self.removed_links.iter().rev() {
            let result = if self.committed {
                fs::remove_file(saved)
            } else {
                match fs::symlink_metadata(path) {
                    Ok(_) => fs::remove_file(saved),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        fs::rename(saved, path)
                    }
                    Err(error) => Err(error),
                }
            };
            if result.is_err() {
                log::error!("Failed to finalize native configuration link removal");
            }
        }
        if self.committed {
            return;
        }
        let mut rolled_back = true;
        for (index, (path, original, preserve_link)) in self.original.iter().enumerate().rev() {
            if self.journal.as_ref().is_some_and(|journal| {
                !journal.files[index]
                    .still_regular_in_original_directory()
                    .unwrap_or(false)
            }) {
                continue;
            }
            if self.guarded
                && ((*preserve_link && !regular_or_missing(path).unwrap_or(false))
                    || read_optional(path).ok().as_ref() != Some(&self.published[index]))
            {
                log::warn!(
                    "Native configuration changed externally; skipping rollback for that file"
                );
                continue;
            }
            let restored = match original {
                Some(bytes) if *preserve_link => write_synced(path, bytes, fs::File::sync_all),
                Some(bytes) => write(path, bytes),
                None => fs::remove_file(path).map_err(|_| "accountError.write".into()),
            };
            if restored.is_err() {
                rolled_back = false;
                log::error!("Failed to restore native account configuration");
            }
        }
        if rolled_back {
            if let Some(journal) = &self.journal {
                if !journal.files.is_empty() {
                    let _ = fs::remove_file(&journal.path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_file_selection_fixture() {
        let Some(root) = std::env::var_os("ECHOBIRD_FILE_SELECTION_FIXTURE") else {
            return;
        };
        let root = PathBuf::from(root);
        let config = root.join("config");
        let catalog = root.join("catalog");
        let auth = root.join("auth");
        let journal = root.join("pending.json");
        let _lock = DirectoryLock::acquire_existing(&config).unwrap();
        let mut selection =
            Transaction::recoverable(&journal, &[&config, &catalog, &auth]).unwrap();
        selection
            .replace_private_file(&config, b"new-config")
            .unwrap();
        selection
            .replace_private_file(&catalog, b"new-catalog")
            .unwrap();
        selection.remove_private_file(&auth).unwrap();
        std::process::exit(77);
    }

    #[test]
    fn recovery_after_process_exit_preserves_external_edits_and_can_retry() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let config = root.join("config");
        let catalog = root.join("catalog");
        let auth = root.join("auth");
        let journal = root.join("pending.json");
        let allowed = [config.as_path(), catalog.as_path(), auth.as_path()];
        fs::write(&config, b"old-config").unwrap();
        fs::write(&auth, b"saved-auth").unwrap();
        let status = crate::utils::process::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "services::native_account_files::tests::interrupted_file_selection_fixture",
            ])
            .env("ECHOBIRD_FILE_SELECTION_FIXTURE", root)
            .output()
            .unwrap()
            .status;
        assert_eq!(status.code(), Some(77));
        assert!(journal.is_file());
        assert_eq!(fs::read(&config).unwrap(), b"new-config");
        assert!(!auth.exists());

        let _lock = DirectoryLock::acquire_existing(&config).unwrap();
        assert!(recover_file_mutations(&journal, &[&config, &catalog]).is_err());
        fs::write(&config, b"external edit").unwrap();
        assert_eq!(
            recover_file_mutations(&journal, &allowed).unwrap_err(),
            RECOVERY_CONFLICT
        );
        assert_eq!(fs::read(&config).unwrap(), b"external edit");
        assert_eq!(fs::read(&auth).unwrap(), b"saved-auth");
        assert!(!catalog.exists());
        assert!(!journal.exists());
        recover_file_mutations(&journal, &allowed).unwrap();
    }

    #[test]
    fn guarded_rollback_preserves_external_edits_and_restores_untouched_files() {
        let root = std::env::temp_dir().join(format!("native-guard-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let changed = root.join("changed");
        let untouched = root.join("untouched");
        let removed = root.join("removed");
        fs::write(&changed, b"old").unwrap();
        fs::write(&untouched, b"old").unwrap();
        fs::write(&removed, b"old").unwrap();
        {
            let mut transaction = Transaction::guarded();
            transaction.replace_private_file(&changed, b"ours").unwrap();
            transaction
                .replace_private_file(&untouched, b"ours")
                .unwrap();
            transaction.remove(&removed).unwrap();
            fs::write(&changed, b"external").unwrap();
            fs::write(&removed, b"external").unwrap();
        }
        assert_eq!(fs::read(&changed).unwrap(), b"external");
        assert_eq!(fs::read(&untouched).unwrap(), b"old");
        assert_eq!(fs::read(&removed).unwrap(), b"external");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn removing_native_symlinks_rolls_back_the_link_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("auth.json");
        let target = Path::new("shared-auth.json");
        fs::write(root.path().join(target), b"shared account").unwrap();
        symlink(target, &config).unwrap();
        {
            let mut transaction = Transaction::guarded();
            transaction.remove_private_file(&config).unwrap();
            assert!(fs::symlink_metadata(&config).is_err());
        }
        assert_eq!(fs::read_link(&config).unwrap(), target);
        {
            let mut transaction = Transaction::guarded();
            transaction.remove_private_file(&config).unwrap();
            symlink("external-missing-auth", &config).unwrap();
        }
        assert_eq!(
            fs::read_link(&config).unwrap(),
            Path::new("external-missing-auth")
        );
        fs::remove_file(&config).unwrap();
        symlink(target, &config).unwrap();
        let mut transaction = Transaction::guarded();
        transaction.remove_private_file(&config).unwrap();
        transaction.commit();
        assert!(fs::symlink_metadata(&config).is_err());
        assert_eq!(
            fs::read(root.path().join(target)).unwrap(),
            b"shared account"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn write_through_dangling_native_symlink_rolls_back_only_the_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("auth.json");
        let target = Path::new("shared-auth.json");
        symlink(target, &config).unwrap();
        {
            let mut transaction = Transaction::guarded();
            transaction.replace_private_file(&config, b"auth").unwrap();
            assert_eq!(fs::read(root.path().join(target)).unwrap(), b"auth");
        }
        assert_eq!(fs::read_link(&config).unwrap(), target);
        assert!(!root.path().join(target).exists());
    }

    #[cfg(unix)]
    #[test]
    fn rolling_back_a_native_file_removal_preserves_parent_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("native-remove-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o750)).unwrap();
        let config = root.join("config");
        fs::write(&config, b"original").unwrap();
        {
            let mut transaction = Transaction::guarded();
            transaction.remove_private_file(&config).unwrap();
            assert!(!config.exists());
        }
        assert_eq!(fs::read(config).unwrap(), b"original");
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o750
        );
        fs::remove_dir_all(root).unwrap();
    }

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
