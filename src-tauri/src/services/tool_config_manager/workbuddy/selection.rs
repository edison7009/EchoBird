use crate::services::native_account_files;
use rusty_leveldb::{Options, DB};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MISSING: &str = "Open WorkBuddy once before configuring its model in EchoBird";
const INVALID: &str = "Invalid WorkBuddy model selection";

pub(super) fn native_id(model_id: &str) -> String {
    if model_id.starts_with("custom-local:") {
        model_id.to_owned()
    } else {
        format!("custom-local:{model_id}")
    }
}

fn key(user_id: Option<&str>) -> Result<Vec<u8>, String> {
    let mut key = b"_file://\0\x01cb-newtask:model".to_vec();
    if let Some(user_id) = user_id.filter(|id| !id.is_empty()) {
        if !user_id.is_ascii() {
            return Err(INVALID.into());
        }
        key.push(b':');
        key.extend_from_slice(user_id.as_bytes());
    }
    Ok(key)
}

fn open(storage: &Path) -> Result<DB, String> {
    if !storage.join("CURRENT").is_file() {
        return Err(MISSING.into());
    }
    DB::open(
        storage,
        Options {
            create_if_missing: false,
            paranoid_checks: true,
            ..Default::default()
        },
    )
    .map_err(|_| "Cannot open WorkBuddy model selection; fully quit WorkBuddy and try again".into())
}

fn decode(bytes: &[u8]) -> Result<Value, String> {
    let text = match bytes.first() {
        Some(1) => bytes[1..].iter().map(|byte| char::from(*byte)).collect(),
        Some(0) if (bytes.len() - 1).is_multiple_of(2) => String::from_utf16(
            &bytes[1..]
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| INVALID)?,
        _ => return Err(INVALID.into()),
    };
    let value: Value = serde_json::from_str(&text).map_err(|_| INVALID)?;
    if !value.is_object() || !value["id"].is_string() {
        return Err(INVALID.into());
    }
    Ok(value)
}

fn encode(value: &Value) -> Result<Vec<u8>, String> {
    let text = serde_json::to_string(value).map_err(|_| INVALID)?;
    if text.is_ascii() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(text.as_bytes());
        Ok(bytes)
    } else {
        let mut bytes = vec![0];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        Ok(bytes)
    }
}

fn get(db: &mut DB, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let snapshot = db.get_snapshot();
    db.get_at(&snapshot, key)
        .map(|value| value.map(|bytes| bytes.to_vec()))
        .map_err(|_| "Cannot read WorkBuddy model selection".into())
}

pub(super) struct SelectionUpdate {
    db: DB,
    key: Vec<u8>,
    previous: Option<Vec<u8>>,
    changed: bool,
}

impl SelectionUpdate {
    pub(super) fn commit(mut self) {
        self.changed = false;
    }
}

impl Drop for SelectionUpdate {
    fn drop(&mut self) {
        if !self.changed {
            return;
        }
        let result = match &self.previous {
            Some(previous) => self.db.put(&self.key, previous),
            None => self.db.delete(&self.key),
        };
        if result.and_then(|_| self.db.flush()).is_err() {
            log::error!("Failed to restore WorkBuddy model selection");
        }
    }
}

pub(super) fn select(
    storage: &Path,
    user_id: Option<&str>,
    model_id: &str,
    is_thinking: bool,
) -> Result<SelectionUpdate, String> {
    if model_id.trim().is_empty() {
        return Err(INVALID.into());
    }
    let key = key(user_id)?;
    let model_id = native_id(model_id);
    let mut db = open(storage)?;
    let previous = get(&mut db, &key)?;
    let current = previous.as_deref().map(decode).transpose()?;
    let mut update = SelectionUpdate {
        db,
        key,
        previous,
        changed: false,
    };
    let mut value = json!({ "id": model_id, "isThinking": is_thinking });
    if let Some(current) = current {
        if current["id"].as_str() == Some(model_id.as_str()) {
            if !is_thinking || current["isThinking"] == true {
                return Ok(update);
            }
            value = current;
            value["isThinking"] = json!(true);
        }
    }
    let value = encode(&value)?;
    update.changed = true;
    update
        .db
        .put(&update.key, &value)
        .and_then(|_| update.db.flush())
        .map_err(|_| "Cannot save WorkBuddy model selection")?;
    Ok(update)
}

struct Snapshot(PathBuf);

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn read(storage: &Path, user_id: Option<&str>) -> Result<Option<String>, String> {
    let key = key(user_id)?;
    if !storage.join("CURRENT").is_file() {
        return Ok(None);
    }
    // Opening LevelDB can recover logs and write; only open a disposable copy.
    let snapshot = Snapshot(std::env::temp_dir().join(format!(
        "echobird-workbuddy-selection-{}",
        uuid::Uuid::new_v4()
    )));
    native_account_files::private_dir(&snapshot.0)?;
    for entry in fs::read_dir(storage).map_err(|_| "Cannot read WorkBuddy model selection")? {
        let entry = entry.map_err(|_| "Cannot read WorkBuddy model selection")?;
        if entry.file_name() != "LOCK" && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            fs::copy(entry.path(), snapshot.0.join(entry.file_name()))
                .map_err(|_| "Cannot snapshot WorkBuddy model selection")?;
        }
    }
    let mut db = open(&snapshot.0)?;
    get(&mut db, &key)?
        .as_deref()
        .map(decode)
        .transpose()
        .map(|value| value.and_then(|value| value["id"].as_str().map(str::to_owned)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "echobird-workbuddy-selection-test-{}",
                uuid::Uuid::new_v4()
            ));
            let mut db = DB::open(&path, Default::default()).unwrap();
            db.flush().unwrap();
            Self(path)
        }

        fn put(&self, user_id: Option<&str>, value: &Value) {
            let mut db = open(&self.0).unwrap();
            db.put(&key(user_id).unwrap(), &encode(value).unwrap())
                .unwrap();
            db.flush().unwrap();
        }

        fn raw(&self, user_id: Option<&str>) -> Option<Vec<u8>> {
            get(&mut open(&self.0).unwrap(), &key(user_id).unwrap()).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn files(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fs::read_dir(path)
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| entry.file_name() != "LOCK")
            .map(|entry| (entry.file_name().into(), fs::read(entry.path()).unwrap()))
            .collect()
    }

    #[test]
    fn selecting_one_user_preserves_anonymous_other_users_templates_and_origins() {
        let fixture = Fixture::new();
        for user_id in [None, Some("first"), Some("second"), Some("first:template")] {
            fixture.put(user_id, &json!({"id": "fast-model", "isThinking": true}));
        }
        let other_origin = b"_https://example.test\0\x01cb-newtask:model:first";
        let unrelated = b"_file://\0\x01unrelated-preference";
        {
            let mut db = open(&fixture.0).unwrap();
            db.put(other_origin, b"other-origin-value").unwrap();
            db.put(unrelated, b"unrelated-value").unwrap();
            db.flush().unwrap();
        }
        select(&fixture.0, Some("first"), "model-one", false)
            .unwrap()
            .commit();
        assert_eq!(
            read(&fixture.0, Some("first")).unwrap().as_deref(),
            Some("custom-local:model-one")
        );
        for user_id in [None, Some("second"), Some("first:template")] {
            assert_eq!(
                read(&fixture.0, user_id).unwrap().as_deref(),
                Some("fast-model")
            );
        }
        let mut db = open(&fixture.0).unwrap();
        assert_eq!(
            db.get(other_origin).unwrap().as_ref(),
            b"other-origin-value"
        );
        assert_eq!(db.get(unrelated).unwrap().as_ref(), b"unrelated-value");
    }

    #[test]
    fn anonymous_selection_does_not_change_user_preference_or_use_user_fallback() {
        let fixture = Fixture::new();
        fixture.put(Some("first"), &json!({"id": "fast-model"}));
        select(&fixture.0, None, "anonymous-model", false)
            .unwrap()
            .commit();
        assert_eq!(
            read(&fixture.0, None).unwrap().as_deref(),
            Some("custom-local:anonymous-model")
        );
        assert_eq!(
            read(&fixture.0, Some("first")).unwrap().as_deref(),
            Some("fast-model")
        );
        assert_eq!(read(&fixture.0, Some("missing")).unwrap(), None);
    }

    #[test]
    fn same_model_keeps_thinking_effort_context_and_unknown_bytes() {
        let fixture = Fixture::new();
        fixture.put(
            Some("first"),
            &json!({
                "id": "custom-local:model-one", "isThinking": true,
                "reasoningEffort": "high", "contextWindow": 200000, "unknown": "keep"
            }),
        );
        let before = fixture.raw(Some("first"));
        select(&fixture.0, Some("first"), "model-one", false)
            .unwrap()
            .commit();
        assert_eq!(fixture.raw(Some("first")), before);
    }

    #[test]
    fn different_model_resets_old_preferences_and_preserves_the_native_prefix() {
        let fixture = Fixture::new();
        fixture.put(
            None,
            &json!({
                "id": "custom-local:old-model", "isThinking": true,
                "reasoningEffort": "high", "contextWindow": 200000, "unknown": "old"
            }),
        );
        select(&fixture.0, None, "custom-local:new-model", false)
            .unwrap()
            .commit();
        let raw = fixture.raw(None).unwrap();
        assert_eq!(raw[0], 1);
        assert_eq!(
            decode(&raw).unwrap(),
            json!({"id":"custom-local:new-model", "isThinking":false})
        );
    }

    #[test]
    fn required_thinking_updates_the_same_model_without_losing_other_preferences() {
        let fixture = Fixture::new();
        fixture.put(
            None,
            &json!({"id":"custom-local:model", "isThinking":false, "unknown":"keep"}),
        );
        select(&fixture.0, None, "model", true).unwrap().commit();
        assert_eq!(
            decode(&fixture.raw(None).unwrap()).unwrap(),
            json!({"id":"custom-local:model", "isThinking":true, "unknown":"keep"})
        );
    }

    #[test]
    fn unicode_model_id_uses_native_utf16_encoding_and_round_trips() {
        let fixture = Fixture::new();
        select(&fixture.0, None, "模型", true).unwrap().commit();
        let raw = fixture.raw(None).unwrap();
        assert_eq!(raw[0], 0);
        assert_eq!(
            decode(&raw).unwrap(),
            json!({"id":"custom-local:模型", "isThinking":true})
        );
        assert_eq!(
            read(&fixture.0, None).unwrap().as_deref(),
            Some("custom-local:模型")
        );
    }

    #[test]
    fn malformed_related_selection_is_not_overwritten() {
        for malformed in [
            vec![1, b'{'],
            vec![0, 1],
            vec![2],
            encode(&json!({"id": 42})).unwrap(),
            encode(&json!([])).unwrap(),
        ] {
            let fixture = Fixture::new();
            {
                let mut db = open(&fixture.0).unwrap();
                db.put(&key(None).unwrap(), &malformed).unwrap();
                db.flush().unwrap();
            }
            assert!(select(&fixture.0, None, "new-model", false).is_err());
            assert!(read(&fixture.0, None).is_err());
            assert_eq!(fixture.raw(None).unwrap(), malformed);
        }
    }

    #[test]
    fn dropped_update_restores_previous_raw_bytes_or_missing_key() {
        for previous in [None, Some(json!({"id":"fast-model", "unknown":"keep"}))] {
            let fixture = Fixture::new();
            if let Some(value) = previous {
                fixture.put(Some("first"), &value);
            }
            let before = fixture.raw(Some("first"));
            let update = select(&fixture.0, Some("first"), "new-model", false).unwrap();
            drop(update);
            assert_eq!(fixture.raw(Some("first")), before);
        }
    }

    #[test]
    fn committed_selection_survives_update_drop() {
        let fixture = Fixture::new();
        select(&fixture.0, Some("first"), "new-model", false)
            .unwrap()
            .commit();
        assert_eq!(
            read(&fixture.0, Some("first")).unwrap().as_deref(),
            Some("custom-local:new-model")
        );
    }

    #[test]
    fn missing_database_is_never_created_and_missing_current_is_not_repaired() {
        let fixture = Fixture::new();
        let missing = fixture.0.join("missing");
        assert!(select(&missing, None, "model", false).is_err());
        assert_eq!(read(&missing, None).unwrap(), None);
        assert!(!missing.exists());
        let current = fixture.0.join("CURRENT");
        fs::remove_file(&current).unwrap();
        let before = files(&fixture.0);
        assert!(select(&fixture.0, None, "model", false).is_err());
        assert_eq!(read(&fixture.0, None).unwrap(), None);
        assert_eq!(files(&fixture.0), before);
    }

    #[test]
    fn native_database_lock_blocks_writes_and_snapshot_reads_leave_source_untouched() {
        let fixture = Fixture::new();
        fixture.put(
            Some("first"),
            &json!({"id":"fast-model", "isThinking":true}),
        );
        let db = open(&fixture.0).unwrap();
        let before = files(&fixture.0);
        assert_eq!(
            read(&fixture.0, Some("first")).unwrap().as_deref(),
            Some("fast-model")
        );
        assert_eq!(files(&fixture.0), before);
        assert!(select(&fixture.0, Some("first"), "model", false).is_err());
        drop(db);
        assert_eq!(
            fixture
                .raw(Some("first"))
                .map(|bytes| decode(&bytes).unwrap()["id"].clone()),
            Some(json!("fast-model"))
        );
    }
}
