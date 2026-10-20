use super::llm_client::{ContentBlock, Message, MessageContent};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_STORE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SUMMARY_BYTES: usize = 12_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistorySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_session_id: Option<String>,
    pub messages: Vec<Message>,
    pub summary: Option<HistorySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySummary {
    pub through: usize,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedHistory {
    version: u8,
    revision: u64,
    snapshot: HistorySnapshot,
}

struct FileLock(File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

pub struct HistoryStore {
    root: PathBuf,
}

impl HistoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn standard() -> Self {
        Self::new(
            crate::utils::platform::echobird_dir()
                .join("config")
                .join("agent_sessions"),
        )
    }

    pub fn load(&self, scope: &str) -> Result<HistorySnapshot, String> {
        if !self.check_directory()? {
            return Ok(HistorySnapshot::default());
        }
        let _guard = self.read_lock(scope)?;
        Ok(self.load_saved(scope)?.snapshot)
    }

    pub fn save(&self, scope: &str, snapshot: &HistorySnapshot) -> Result<(), String> {
        validate_snapshot(snapshot)?;
        self.check_directory()?;
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Cannot create conversation storage: {error}"))?;
        self.check_directory()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
                .map_err(|error| format!("Cannot protect conversation storage: {error}"))?;
        }
        let _guard = self.write_lock(scope)?;
        let previous = self.load_saved(scope)?;
        let saved = SavedHistory {
            version: 1,
            revision: previous
                .revision
                .checked_add(1)
                .ok_or("Conversation revision limit reached")?,
            snapshot: snapshot.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&saved)
            .map_err(|error| format!("Cannot encode conversation: {error}"))?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err("Conversation exceeds the 64 MiB storage limit".into());
        }
        self.atomic_write(&self.primary_path(scope), &bytes)?;
        self.atomic_write(&self.backup_path(scope), &bytes)
            .map_err(|error| format!("Conversation was saved, but its backup failed: {error}"))
    }

    pub fn clear(&self, scope: &str) -> Result<(), String> {
        if !self.check_directory()? {
            return Ok(());
        }
        let primary = self.primary_path(scope);
        let backup = self.backup_path(scope);
        let lock = self.lock_path(scope);
        for path in [&primary, &backup, &lock] {
            drop(open_regular_file(path, false)?);
        }
        if !primary.exists() && !backup.exists() && !lock.exists() {
            return Ok(());
        }
        let guard = self.write_lock(scope)?;
        remove_owned_file(&primary)?;
        remove_owned_file(&backup)?;
        drop(guard);
        remove_owned_file(&lock)
    }

    fn load_saved(&self, scope: &str) -> Result<SavedHistory, String> {
        let primary = self.read_saved(&self.primary_path(scope));
        let backup = self.read_saved(&self.backup_path(scope));
        match (primary, backup) {
            (Ok(Some(primary)), Ok(Some(backup))) => Ok(if backup.revision > primary.revision {
                backup
            } else {
                primary
            }),
            (Ok(Some(saved)), Ok(None)) | (Ok(None), Ok(Some(saved))) => Ok(saved),
            (Ok(Some(saved)), Err(error)) | (Err(error), Ok(Some(saved))) => {
                log::warn!("[MotherHistory] using intact conversation copy: {error}");
                Ok(saved)
            }
            (Ok(None), Ok(None)) => Ok(SavedHistory {
                version: 1,
                revision: 0,
                snapshot: HistorySnapshot::default(),
            }),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    fn read_saved(&self, path: &Path) -> Result<Option<SavedHistory>, String> {
        let Some(file) = open_regular_file(path, false)? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        file.take((MAX_STORE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot read conversation: {error}"))?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(
                "Saved conversation exceeds the 64 MiB storage limit; file was preserved".into(),
            );
        }
        let saved = if bytes
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b'[')
        {
            SavedHistory {
                version: 1,
                revision: 0,
                snapshot: HistorySnapshot {
                    search_session_id: None,
                    messages: serde_json::from_slice(&bytes).map_err(|error| {
                        format!("Saved conversation is damaged; file was preserved: {error}")
                    })?,
                    summary: None,
                },
            }
        } else {
            let saved: SavedHistory = serde_json::from_slice(&bytes).map_err(|error| {
                format!("Saved conversation is damaged; file was preserved: {error}")
            })?;
            if saved.version != 1 {
                return Err(
                    "Saved conversation has an unsupported format; file was preserved".into(),
                );
            }
            saved
        };
        validate_snapshot(&saved.snapshot)?;
        Ok(Some(saved))
    }

    fn primary_path(&self, scope: &str) -> PathBuf {
        self.root.join(format!("{}.json", safe_key(scope)))
    }

    fn backup_path(&self, scope: &str) -> PathBuf {
        self.root.join(format!("{}.backup.json", safe_key(scope)))
    }

    fn lock_path(&self, scope: &str) -> PathBuf {
        self.root.join(format!("{}.writer.lock", safe_key(scope)))
    }

    fn atomic_write(&self, target: &Path, bytes: &[u8]) -> Result<(), String> {
        drop(open_regular_file(target, false)?);
        let temporary = self.root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_private_options(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("Cannot save conversation: {error}"))?;
        let write = file.write_all(bytes).and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = write.and_then(|_| fs::rename(&temporary, target)) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Cannot save conversation: {error}"));
        }
        #[cfg(unix)]
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("Cannot sync conversation storage: {error}"))?;
        Ok(())
    }

    fn read_lock(&self, scope: &str) -> Result<Option<FileLock>, String> {
        let Some(file) = open_regular_file(&self.lock_path(scope), false)? else {
            return Ok(None);
        };
        fs2::FileExt::try_lock_shared(&file)
            .map_err(|error| format!("Conversation storage is busy: {error}"))?;
        Ok(Some(FileLock(file)))
    }

    fn write_lock(&self, scope: &str) -> Result<FileLock, String> {
        let path = self.lock_path(scope);
        let file = match open_regular_file(&path, true)? {
            Some(file) => file,
            None => {
                let mut options = OpenOptions::new();
                options.read(true).write(true).create_new(true);
                set_private_options(&mut options);
                match options.open(&path) {
                    Ok(file) => file,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        open_regular_file(&path, true)?.ok_or("Conversation lock is unavailable")?
                    }
                    Err(error) => return Err(format!("Cannot lock conversation storage: {error}")),
                }
            }
        };
        fs2::FileExt::try_lock_exclusive(&file)
            .map_err(|error| format!("Conversation storage is busy: {error}"))?;
        Ok(FileLock(file))
    }

    fn check_directory(&self) -> Result<bool, String> {
        let mut exists = true;
        for path in self.root.ancestors() {
            if path.as_os_str().is_empty() {
                continue;
            }
            match fs::symlink_metadata(path) {
                Ok(metadata) if is_link(&metadata) || !metadata.is_dir() => {
                    return Err(
                        "Conversation storage cannot use linked or non-directory paths".into(),
                    );
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => exists = false,
                Err(error) => return Err(format!("Cannot inspect conversation storage: {error}")),
            }
        }
        Ok(exists)
    }
}

fn validate_snapshot(snapshot: &HistorySnapshot) -> Result<(), String> {
    if let Some(summary) = &snapshot.summary {
        if summary.through > snapshot.messages.len() || summary.text.len() > MAX_SUMMARY_BYTES {
            return Err(
                "Conversation summary has an invalid boundary or exceeds 12000 UTF-8 bytes".into(),
            );
        }
    }
    let mut bytes = snapshot
        .summary
        .as_ref()
        .map_or(0, |summary| summary.text.len());
    for message in &snapshot.messages {
        bytes = bytes.saturating_add(message.role.len());
        match &message.content {
            MessageContent::Text(text) => bytes = bytes.saturating_add(text.len()),
            MessageContent::Blocks(blocks) => {
                for block in blocks {
                    let length = match block {
                        ContentBlock::Text { text } => text.len(),
                        ContentBlock::Thinking {
                            thinking,
                            signature,
                            ..
                        } => thinking.len().saturating_add(signature.len()),
                        ContentBlock::RedactedThinking { data, .. } => data.len(),
                        ContentBlock::ToolUse { id, name, .. } => {
                            id.len().saturating_add(name.len())
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                        } => tool_use_id.len().saturating_add(content.len()),
                    };
                    bytes = bytes.saturating_add(length);
                }
            }
        }
        if bytes > MAX_STORE_BYTES {
            return Err("Conversation exceeds the 64 MiB storage limit".into());
        }
    }
    Ok(())
}

fn safe_key(scope: &str) -> String {
    scope.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

fn remove_owned_file(path: &Path) -> Result<(), String> {
    drop(open_regular_file(path, false)?);
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot clear conversation file: {error}")),
    }
}

fn is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

fn set_private_options(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
}

fn open_regular_file(path: &Path, writable: bool) -> Result<Option<File>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => {}
        Ok(_) => return Err("Conversation storage contains a linked or non-regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot inspect conversation storage: {error}")),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(writable);
    set_private_options(&mut options);
    let file = options
        .open(path)
        .map_err(|error| format!("Cannot open conversation: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect conversation: {error}"))?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err("Conversation storage contains a linked or non-regular file".into());
    }
    Ok(Some(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let base = std::env::temp_dir();
            let base = fs::canonicalize(&base).unwrap_or(base);
            Self(base.join(format!("echobird-history-test-{}", uuid::Uuid::new_v4())))
        }

        fn store(&self) -> HistoryStore {
            HistoryStore::new(self.0.clone())
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            if self.0.is_dir() {
                let _ = fs::remove_dir_all(&self.0);
            } else {
                let _ = fs::remove_file(&self.0);
            }
        }
    }

    fn snapshot(text: &str) -> HistorySnapshot {
        HistorySnapshot {
            search_session_id: None,
            messages: vec![
                Message {
                    role: "user".into(),
                    content: MessageContent::Text(text.into()),
                },
                Message {
                    role: "assistant".into(),
                    content: MessageContent::Blocks(vec![
                        ContentBlock::Text {
                            text: "I will inspect the output".into(),
                        },
                        ContentBlock::ToolUse {
                            id: "call-1".into(),
                            name: "shell_exec".into(),
                            input: json!({"command":"echo complete"}),
                        },
                    ]),
                },
                Message {
                    role: "user".into(),
                    content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                        tool_use_id: "call-1".into(),
                        content: "untrusted tool text: password=original-secret".into(),
                    }]),
                },
            ],
            summary: Some(HistorySummary {
                through: 1,
                text: "用户正在诊断安装结果".into(),
            }),
        }
    }

    fn encoded(snapshot: &HistorySnapshot) -> serde_json::Value {
        serde_json::to_value(snapshot).unwrap()
    }

    #[test]
    fn missing_load_and_clear_do_not_create_storage() {
        let dir = TestDir::new();
        assert!(dir
            .store()
            .load("conversation-a")
            .unwrap()
            .messages
            .is_empty());
        dir.store().clear("conversation-a").unwrap();
        assert!(!dir.0.exists());
    }

    #[test]
    fn legacy_array_loads_without_writes_and_migrates_with_complete_tool_history() {
        let dir = TestDir::new();
        fs::create_dir(&dir.0).unwrap();
        let store = dir.store();
        let original = snapshot(&"完整中文🙂".repeat(10_000));
        let bytes = serde_json::to_vec(&original.messages).unwrap();
        fs::write(store.primary_path("server/a:b"), &bytes).unwrap();
        let loaded = store.load("server/a:b").unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.messages).unwrap(),
            serde_json::to_value(&original.messages).unwrap()
        );
        assert!(loaded.summary.is_none());
        assert_eq!(fs::read(store.primary_path("server/a:b")).unwrap(), bytes);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        store.save("server/a:b", &original).unwrap();
        assert_eq!(
            encoded(&store.load("server/a:b").unwrap()),
            encoded(&original)
        );
        let saved: SavedHistory =
            serde_json::from_slice(&fs::read(store.primary_path("server/a:b")).unwrap()).unwrap();
        assert_eq!(saved.version, 1);
        assert_eq!(saved.revision, 1);
        assert!(store.backup_path("server/a:b").exists());
    }

    #[test]
    fn summary_roundtrip_preserves_full_original_messages() {
        let dir = TestDir::new();
        let mut original = snapshot("用户的完整原始消息");
        if let MessageContent::Blocks(blocks) = &mut original.messages[1].content {
            blocks.insert(
                0,
                serde_json::from_value(
                    json!({"type":"redacted_thinking","data":"opaque-native-block"}),
                )
                .unwrap(),
            );
        }
        dir.store().save("conversation-a", &original).unwrap();
        assert_eq!(
            encoded(&dir.store().load("conversation-a").unwrap()),
            encoded(&original)
        );
    }

    #[test]
    fn invalid_summary_and_oversized_text_do_not_create_storage() {
        let dir = TestDir::new();
        let store = dir.store();
        let mut invalid = snapshot("text");
        invalid.summary.as_mut().unwrap().through = invalid.messages.len() + 1;
        assert!(store.save("a", &invalid).is_err());
        invalid.summary.as_mut().unwrap().through = 0;
        invalid.summary.as_mut().unwrap().text = "界".repeat(4_001);
        assert!(store.save("a", &invalid).is_err());
        let large = HistorySnapshot {
            search_session_id: None,
            messages: vec![Message {
                role: "user".into(),
                content: MessageContent::Text("x".repeat(MAX_STORE_BYTES + 1)),
            }],
            summary: None,
        };
        assert!(store.save("a", &large).is_err());
        let opaque = HistorySnapshot {
            search_session_id: None,
            messages: vec![Message {
                role: "assistant".into(),
                content: MessageContent::Blocks(vec![serde_json::from_value(
                    json!({"type":"redacted_thinking","data":"x".repeat(MAX_STORE_BYTES + 1)}),
                )
                .unwrap()]),
            }],
            summary: None,
        };
        assert!(store.save("a", &opaque).is_err());
        assert!(!dir.0.exists());
    }

    #[test]
    fn damaged_primary_recovers_backup_without_passive_writes() {
        let dir = TestDir::new();
        let store = dir.store();
        let original = snapshot("preserve my history");
        store.save("a", &original).unwrap();
        fs::write(store.primary_path("a"), "damaged primary").unwrap();
        assert_eq!(encoded(&store.load("a").unwrap()), encoded(&original));
        assert_eq!(
            fs::read_to_string(store.primary_path("a")).unwrap(),
            "damaged primary"
        );
        let recovered = snapshot("continue safely");
        store.save("a", &recovered).unwrap();
        assert_eq!(encoded(&store.load("a").unwrap()), encoded(&recovered));
    }

    #[test]
    fn damaged_or_unrecognized_copies_do_not_get_overwritten_or_emptied() {
        let dir = TestDir::new();
        let store = dir.store();
        store.save("a", &snapshot("original")).unwrap();
        for path in [store.primary_path("a"), store.backup_path("a")] {
            fs::write(
                path,
                "{\"version\":99,\"revision\":1,\"snapshot\":{\"messages\":[],\"summary\":null}}",
            )
            .unwrap();
        }
        let previous = fs::read(store.primary_path("a")).unwrap();
        assert!(store.load("a").is_err());
        assert!(store.save("a", &snapshot("replacement")).is_err());
        assert_eq!(fs::read(store.primary_path("a")).unwrap(), previous);
        assert_eq!(fs::read(store.backup_path("a")).unwrap(), previous);
    }

    #[test]
    fn newest_valid_revision_survives_interruption_in_either_copy() {
        let dir = TestDir::new();
        let store = dir.store();
        store.save("a", &snapshot("old")).unwrap();
        let old = fs::read(store.primary_path("a")).unwrap();
        let newest = snapshot("new");
        store.save("a", &newest).unwrap();
        let new = fs::read(store.primary_path("a")).unwrap();
        fs::write(store.backup_path("a"), &old).unwrap();
        assert_eq!(encoded(&store.load("a").unwrap()), encoded(&newest));
        fs::write(store.primary_path("a"), old).unwrap();
        fs::write(store.backup_path("a"), new).unwrap();
        assert_eq!(encoded(&store.load("a").unwrap()), encoded(&newest));
    }

    #[test]
    fn conversations_and_owned_cleanup_remain_separate() {
        let dir = TestDir::new();
        let store = dir.store();
        store.save("a", &snapshot("a")).unwrap();
        let b = snapshot("b");
        store.save("b", &b).unwrap();
        fs::write(dir.0.join("unrelated.json"), "keep").unwrap();
        store.clear("a").unwrap();
        assert!(store.load("a").unwrap().messages.is_empty());
        assert_eq!(encoded(&store.load("b").unwrap()), encoded(&b));
        assert_eq!(
            fs::read_to_string(dir.0.join("unrelated.json")).unwrap(),
            "keep"
        );
        assert!(!store.primary_path("a").exists());
        assert!(!store.backup_path("a").exists());
        assert!(!store.lock_path("a").exists());
    }

    #[test]
    fn failed_primary_clear_preserves_raw_backup_and_other_conversations() {
        let dir = TestDir::new();
        let store = dir.store();
        let original = snapshot("a");
        store.save("a", &original).unwrap();
        let backup = fs::read(store.backup_path("a")).unwrap();
        store.save("b", &snapshot("b")).unwrap();
        fs::remove_file(store.primary_path("a")).unwrap();
        fs::create_dir(store.primary_path("a")).unwrap();
        assert!(store.clear("a").is_err());
        assert_eq!(fs::read(store.backup_path("a")).unwrap(), backup);
        assert_eq!(encoded(&store.load("a").unwrap()), encoded(&original));
        assert_eq!(encoded(&store.load("b").unwrap()), encoded(&snapshot("b")));
    }

    #[test]
    fn busy_scope_rejects_operations_and_does_not_block_other_scopes() {
        let dir = TestDir::new();
        let store = dir.store();
        store.save("a", &snapshot("a")).unwrap();
        let bytes = fs::read(store.primary_path("a")).unwrap();
        let guard = store.write_lock("a").unwrap();
        assert!(store.load("a").is_err());
        assert!(store.save("a", &snapshot("new")).is_err());
        assert!(store.clear("a").is_err());
        store.save("b", &snapshot("b")).unwrap();
        assert_eq!(fs::read(store.primary_path("a")).unwrap(), bytes);
        drop(guard);
        assert_eq!(encoded(&store.load("a").unwrap()), encoded(&snapshot("a")));
    }

    #[test]
    fn filesystem_failure_preserves_existing_target() {
        let dir = TestDir::new();
        fs::write(&dir.0, "not a directory").unwrap();
        let store = dir.store();
        assert!(store.load("a").is_err());
        assert!(store.save("a", &snapshot("a")).is_err());
        assert!(store.clear("a").is_err());
        assert_eq!(fs::read_to_string(&dir.0).unwrap(), "not a directory");
    }

    #[cfg(unix)]
    #[test]
    fn linked_paths_are_not_followed_and_new_files_are_private() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = TestDir::new();
        let store = dir.store();
        store.save("a", &snapshot("private")).unwrap();
        assert_eq!(
            fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for path in [
            store.primary_path("a"),
            store.backup_path("a"),
            store.lock_path("a"),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let outside = dir.0.join("outside");
        fs::write(&outside, "private file").unwrap();
        fs::remove_file(store.primary_path("a")).unwrap();
        symlink(&outside, store.primary_path("a")).unwrap();
        assert!(store.save("a", &snapshot("new")).is_err());
        assert!(store.clear("a").is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "private file");
        let linked = dir.0.join("linked");
        symlink(&dir.0, &linked).unwrap();
        assert!(HistoryStore::new(linked).load("a").is_err());
    }
}
