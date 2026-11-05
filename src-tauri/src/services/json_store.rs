//! Serialized read/modify/write for EchoBird-owned JSON documents.
use serde::{de::DeserializeOwned, Serialize};
use std::{fs, path::Path};

pub(crate) struct FileLock(fs::File);

impl FileLock {
    pub(crate) fn acquire(path: &Path, wait: bool) -> Result<Self, String> {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Refusing a symbolic-link lock file".into());
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(path)
            .map_err(|e| format!("Cannot open configuration lock: {e}"))?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Invalid configuration lock file".into());
        }
        if wait {
            fs2::FileExt::lock_exclusive(&file)
        } else {
            fs2::FileExt::try_lock_exclusive(&file)
        }
        .map_err(|e| format!("Cannot lock configuration: {e}"))?;
        Ok(Self(file))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
        // Keep the sidecar: unlinking it could let another writer lock a new inode.
    }
}

pub(crate) fn read<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid configuration {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("Cannot read configuration {}: {e}", path.display())),
    }
}

pub(crate) fn mutate<T: DeserializeOwned + Default + Serialize, R>(
    path: &Path,
    operation: impl FnOnce(&mut T) -> Result<(R, bool), String>,
) -> Result<R, String> {
    fs::create_dir_all(path.parent().ok_or("Configuration has no parent")?)
        .map_err(|e| e.to_string())?;
    let mut name = path.as_os_str().to_os_string();
    name.push(".write-lock");
    let _lock = FileLock::acquire(Path::new(&name), true)?;
    let mut value = read(path)?;
    let (result, changed) = operation(&mut value)?;
    if changed {
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        super::native_account_files::write_private_file(path, &bytes)
            .map_err(|error| format!("Cannot save configuration {}: {error}", path.display()))?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_updates_preserve_every_write_and_corrupt_data_is_never_replaced() {
        let root = std::env::temp_dir().join(format!("echobird-json-{}", uuid::Uuid::new_v4()));
        let path = root.join("data.json");
        let threads: Vec<_> = (0..12)
            .map(|id| {
                let path = path.clone();
                std::thread::spawn(move || {
                    mutate(&path, |rows: &mut Vec<u32>| {
                        rows.push(id);
                        Ok(((), true))
                    })
                    .unwrap()
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let mut rows: Vec<u32> = read(&path).unwrap();
        rows.sort();
        assert_eq!(rows, (0..12).collect::<Vec<_>>());
        fs::write(&path, b"truncated {").unwrap();
        assert!(mutate(&path, |_: &mut Vec<u32>| Ok(((), true))).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"truncated {");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(mutate(&path, |_: &mut Vec<u32>| Ok(((), true))).is_err());
        assert!(path.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unavailable_parent_fails_and_noop_does_not_create_document() {
        let root = std::env::temp_dir().join(format!("echobird-json-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("data.json");
        mutate(&path, |_: &mut Vec<u32>| Ok(((), false))).unwrap();
        assert!(!path.exists());
        fs::write(&path, b"blocker").unwrap();
        assert!(mutate(&path.join("data.json"), |_: &mut Vec<u32>| Ok(((), true))).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"blocker");
        fs::remove_dir_all(root).unwrap();
    }
}
