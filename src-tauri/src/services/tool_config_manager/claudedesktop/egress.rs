use crate::services::native_account_files;
use serde_json::{json, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const ENTRY_ID: &str = "c737ba50-eea6-47d3-babe-e7f460a85838";
const ENTRY_FILE: &str = "c737ba50-eea6-47d3-babe-e7f460a85838.json";
const MANIFEST: &str = "_meta.json";
const CONFLICT: &str = "Claude Desktop's Cowork proxy profile contains other settings. Review the profile before retrying.";

fn manifest() -> Value {
    json!({
        "appliedId": ENTRY_ID,
        "entries": [{"id": ENTRY_ID, "name": "EchoBird"}],
        "echobirdCoworkProxy": true,
    })
}

fn owned(library: &Path) -> Result<bool, String> {
    let metadata = match fs::symlink_metadata(library) {
        Ok(metadata) if metadata.is_dir() => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Ok(false);
        }
    }
    #[cfg(not(windows))]
    let _ = metadata;

    let meta = match native_account_files::object(&library.join(MANIFEST)) {
        Ok(meta) => meta,
        Err(error) if library.join(ENTRY_FILE).exists() => return Err(error),
        Err(_) => return Ok(false),
    };
    if meta["echobirdCoworkProxy"] != true {
        return Ok(false);
    }
    // Only retire the profile if both files still match what we own.
    if meta != manifest() {
        return Err(CONFLICT.into());
    }
    for file in fs::read_dir(library).map_err(|error| error.to_string())? {
        let file = file.map_err(|error| error.to_string())?;
        if !matches!(file.file_name().to_str(), Some(MANIFEST | ENTRY_FILE))
            || !file
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
        {
            return Err(CONFLICT.into());
        }
    }
    let entry = native_account_files::object(&library.join(ENTRY_FILE))?;
    let port = entry["egressProxyUrl"]
        .as_str()
        .and_then(|value| value.strip_prefix("http://127.0.0.1:"))
        .and_then(|port| port.parse::<u16>().ok());
    if entry.as_object().is_none_or(|object| object.len() != 1) || port.is_none_or(|port| port == 0)
    {
        return Err(CONFLICT.into());
    }
    Ok(true)
}

pub(super) fn write_proxy(library: &Path, port: u16) -> Result<(), String> {
    if port == 0 {
        return Err("Cowork proxy is not listening.".into());
    }
    let entry = native_account_files::json(&json!({
        "egressProxyUrl": format!("http://127.0.0.1:{port}")
    }))?;
    if owned(library)? {
        return native_account_files::write_private_file(&library.join(ENTRY_FILE), &entry);
    }
    if !matches!(fs::symlink_metadata(library), Err(e) if e.kind() == io::ErrorKind::NotFound) {
        return Err(CONFLICT.into());
    }
    let staged = library.with_file_name(format!(".echobird-cowork-{}", uuid::Uuid::new_v4()));
    native_account_files::private_dir(&staged)?;
    let result = (|| {
        native_account_files::write_private_file(&staged.join(ENTRY_FILE), &entry)?;
        native_account_files::write_private_file(
            &staged.join(MANIFEST),
            &native_account_files::json(&manifest())?,
        )?;
        fs::rename(&staged, library)
            .map_err(|error| format!("Cannot prepare Cowork proxy profile: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(staged.join(ENTRY_FILE));
        let _ = fs::remove_file(staged.join(MANIFEST));
        let _ = fs::remove_dir(staged);
    }
    result
}

pub(super) struct RetiredProfile {
    library: PathBuf,
    parked: PathBuf,
    committed: bool,
}

pub(super) fn retire(library: &Path) -> Result<Option<RetiredProfile>, String> {
    if !owned(library)? {
        return Ok(None);
    }
    let parked =
        library.with_file_name(format!(".echobird-cowork-retired-{}", uuid::Uuid::new_v4()));
    fs::rename(library, &parked)
        .map_err(|error| format!("Cannot retire Cowork proxy profile: {error}"))?;
    Ok(Some(RetiredProfile {
        library: library.to_path_buf(),
        parked,
        committed: false,
    }))
}

impl RetiredProfile {
    pub(super) fn commit(mut self) {
        self.committed = true;
        for name in [MANIFEST, ENTRY_FILE] {
            let _ = fs::remove_file(self.parked.join(name));
        }
        if let Err(error) = fs::remove_dir(&self.parked) {
            log::warn!("[ClaudeLocal] Could not remove retired Cowork proxy profile: {error}");
        }
    }
}

impl Drop for RetiredProfile {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if !matches!(fs::symlink_metadata(&self.library), Err(e) if e.kind() == io::ErrorKind::NotFound)
        {
            log::warn!("[ClaudeLocal] Kept the parked Cowork proxy profile because the client profile changed");
            return;
        }
        if let Err(error) = fs::rename(&self.parked, &self.library) {
            log::warn!("[ClaudeLocal] Could not restore the parked Cowork proxy profile: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_the_port_and_only_retires_the_owned_profile() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Claude-3p/configLibrary");
        write_proxy(&path, 35000).unwrap();
        write_proxy(&path, 35001).unwrap();
        assert_eq!(
            native_account_files::object(&path.join(ENTRY_FILE)).unwrap()["egressProxyUrl"],
            "http://127.0.0.1:35001"
        );
        fs::write(path.join("user.json"), b"custom profile").unwrap();
        assert!(write_proxy(&path, 35002).is_err());
        assert!(retire(&path).is_err());
        assert_eq!(fs::read(path.join("user.json")).unwrap(), b"custom profile");
        fs::remove_file(path.join("user.json")).unwrap();
        let retired = retire(&path).unwrap().unwrap();
        assert!(!path.exists());
        drop(retired);
        assert!(owned(&path).unwrap());
        retire(&path).unwrap().unwrap().commit();
        assert!(!path.exists());
    }

    #[test]
    fn a_parked_profile_cannot_block_the_next_launch() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("configLibrary");
        write_proxy(&library, 35000).unwrap();
        let retired = retire(&library).unwrap().unwrap();
        assert!(owned(&retired.parked).unwrap());
        write_proxy(&library, 35001).unwrap();
        let previous = retired.parked.clone();
        drop(retired);
        assert!(owned(&previous).unwrap());
        assert_eq!(
            native_account_files::object(&library.join(ENTRY_FILE)).unwrap()["egressProxyUrl"],
            "http://127.0.0.1:35001"
        );
    }
}
