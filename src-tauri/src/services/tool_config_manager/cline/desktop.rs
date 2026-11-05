//! Only Cline's remembered model selection is changed. Session records are separate.
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(any(not(windows), test))]
const KEY: &str = "cline.code.model-selection.v1";
#[cfg(windows)]
const LEVEL_KEY: &[u8] = b"_http://tauri.localhost\0\x01cline.code.model-selection.v1";
const MISSING: &str = "Open Cline Desktop once before configuring its model in EchoBird";

pub(super) fn select(mut selection: Value, provider: &str, model: &str) -> Result<Value, String> {
    if !selection.is_object() || !selection["lastModelByProvider"].is_object() {
        return Err("Unsupported Cline Desktop model selection format".into());
    }
    selection["lastProvider"] = json!(provider);
    selection["lastModelByProvider"][provider] = json!(model);
    Ok(selection)
}

fn decode_utf16(bytes: &[u8]) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("Invalid Cline Desktop model selection encoding".into());
    }
    String::from_utf16(
        &bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| "Invalid Cline Desktop model selection encoding".into())
}

pub(crate) enum Storage {
    #[cfg(windows)]
    Level(Box<rusty_leveldb::DB>),
    #[cfg(any(not(windows), test))]
    Sqlite(rusqlite::Connection),
}

impl Storage {
    pub(crate) fn open() -> Result<Self, String> {
        Self::open_at(&path()?)
    }

    pub(crate) fn open_at(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        if path.is_dir() {
            let db = rusty_leveldb::DB::open(
                path,
                rusty_leveldb::Options {
                    create_if_missing: false,
                    paranoid_checks: true,
                    ..Default::default()
                },
            )
            .map_err(|_| {
                "Cannot open Cline Desktop model storage; fully quit Cline and try again"
            })?;
            return Ok(Self::Level(Box::new(db)));
        }
        #[cfg(any(not(windows), test))]
        {
            let db = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
            )
            .map_err(|_| "Cannot open Cline Desktop model storage")?;
            db.busy_timeout(std::time::Duration::from_secs(2))
                .map_err(|_| "Cannot access Cline Desktop model storage")?;
            Ok(Self::Sqlite(db))
        }
        #[cfg(all(windows, not(test)))]
        Err(MISSING.into())
    }

    pub(crate) fn read(&mut self) -> Result<Value, String> {
        let text = match self {
            #[cfg(windows)]
            Self::Level(db) => {
                let bytes = db.get(LEVEL_KEY).ok_or(MISSING)?;
                match bytes.first() {
                    Some(1) => bytes[1..].iter().map(|b| char::from(*b)).collect(),
                    Some(0) => decode_utf16(&bytes[1..])?,
                    _ => return Err("Unsupported Cline Desktop model selection encoding".into()),
                }
            }
            #[cfg(any(not(windows), test))]
            Self::Sqlite(db) => {
                let bytes: Vec<u8> = db
                    .query_row("SELECT value FROM ItemTable WHERE key = ?1", [KEY], |row| {
                        row.get(0)
                    })
                    .map_err(|_| MISSING)?;
                decode_utf16(&bytes)?
            }
        };
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| "Invalid Cline Desktop model selection JSON")?;
        if !value.is_object() || !value["lastModelByProvider"].is_object() {
            return Err("Unsupported Cline Desktop model selection format".into());
        }
        Ok(value)
    }

    pub(crate) fn write(&mut self, selection: &Value) -> Result<(), String> {
        let text = serde_json::to_string(selection)
            .map_err(|_| "Cannot serialize Cline Desktop model selection")?;
        let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        match self {
            #[cfg(windows)]
            Self::Level(db) => {
                // Chromium localStorage's byte 0 denotes UTF-16LE; byte 1 denotes Latin-1.
                let mut bytes = vec![0];
                bytes.extend(utf16);
                db.put(LEVEL_KEY, &bytes)
                    .and_then(|_| db.flush())
                    .map_err(|_| "Cannot save Cline Desktop model selection")?;
            }
            #[cfg(any(not(windows), test))]
            Self::Sqlite(db) => {
                let changed = db
                    .execute(
                        "UPDATE ItemTable SET value = ?1 WHERE key = ?2",
                        rusqlite::params![utf16, KEY],
                    )
                    .map_err(|_| "Cannot save Cline Desktop model selection")?;
                if changed != 1 {
                    return Err(MISSING.into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn path() -> Result<PathBuf, String> {
    let path = dirs::data_local_dir()
        .ok_or(MISSING)?
        .join("bot.cline.app/EBWebView/Default/Local Storage/leveldb");
    if !path.join("CURRENT").is_file() {
        return Err(MISSING.into());
    }
    Ok(path)
}

#[cfg(not(windows))]
fn path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    let root = dirs::home_dir()
        .ok_or(MISSING)?
        .join("Library/WebKit/bot.cline.app/WebsiteData");
    #[cfg(not(target_os = "macos"))]
    let root = dirs::data_local_dir().ok_or(MISSING)?.join("bot.cline.app");
    let mut candidates = Vec::new();
    find_sqlite(&root, 0, &mut candidates);
    let mut matches = Vec::new();
    for candidate in candidates {
        if read_at(&candidate).is_ok() {
            matches.push(candidate);
        }
    }
    if matches.len() != 1 {
        return Err(MISSING.into());
    }
    Ok(matches.remove(0))
}

#[cfg(not(windows))]
fn find_sqlite(root: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            find_sqlite(&entry.path(), depth + 1, found);
        } else if kind.is_file() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "localstorage.sqlite3" || name.ends_with(".localstorage") {
                found.push(entry.path());
            }
        }
    }
}

struct Snapshot(PathBuf);
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Opening LevelDB recovers logs and may write. Scan a disposable snapshot so passive
// navigation never locks or modifies Cline's native database. SQLite WAL travels with it.
fn read_at(source: &Path) -> Result<Value, String> {
    let snapshot =
        Snapshot(std::env::temp_dir().join(format!("echobird-cline-{}", uuid::Uuid::new_v4())));
    fs::create_dir(&snapshot.0).map_err(|_| "Cannot read Cline Desktop model storage")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&snapshot.0, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot protect Cline Desktop model snapshot")?;
    }
    let target = if source.is_dir() {
        for entry in fs::read_dir(source).map_err(|_| MISSING)?.flatten() {
            let name = entry.file_name();
            if name == "LOCK" || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            fs::copy(entry.path(), snapshot.0.join(name))
                .map_err(|_| "Cannot snapshot Cline Desktop model storage")?;
        }
        snapshot.0.clone()
    } else {
        let target = snapshot.0.join("localstorage.sqlite3");
        fs::copy(source, &target).map_err(|_| "Cannot snapshot Cline Desktop model storage")?;
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", source.display()));
            if sidecar.exists() {
                fs::copy(
                    sidecar,
                    snapshot.0.join(format!("localstorage.sqlite3{suffix}")),
                )
                .map_err(|_| "Cannot snapshot Cline Desktop model storage")?;
            }
        }
        target
    };
    Storage::open_at(&target)?.read()
}

pub(crate) fn read() -> Result<Value, String> {
    read_at(&path()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Set ECHOBIRD_CLINE_FIXTURE to a disposable directory containing
    /// .echobird-cline-fixture. Launch Cline with CLINE_DATA_DIR=<root>/data and
    /// WEBVIEW2_USER_DATA_FOLDER=<root>/browser after this test, against a local
    /// HTTP fixture on port 18769. Never point this at the user's native profile.
    #[cfg(windows)]
    #[test]
    #[ignore = "manual native-client fixture; requires an explicitly marked isolated directory"]
    fn prepare_native_cline_fixture() {
        let root = PathBuf::from(
            std::env::var("ECHOBIRD_CLINE_FIXTURE").expect("isolated fixture directory"),
        );
        assert!(root.join(".echobird-cline-fixture").is_file());
        let path = root.join("browser/EBWebView/Default/Local Storage/leveldb");
        if !path.join("CURRENT").exists() {
            fs::create_dir_all(&path).unwrap();
            let mut db = rusty_leveldb::DB::open(&path, Default::default()).unwrap();
            db.put(
                LEVEL_KEY,
                b"\x01{\"lastProvider\":\"\",\"lastModelByProvider\":{}}",
            )
            .unwrap();
            db.flush().unwrap();
        }
        let protocol = std::env::var("ECHOBIRD_CLINE_PROTOCOL").unwrap_or_else(|_| "openai".into());
        let config = root.join("data/settings/providers.json");
        let info = serde_json::from_value(json!({"model":"fixture-shared","baseUrl":"http://127.0.0.1:18769/v1","apiKey":"fixture","protocol":protocol})).unwrap();
        let settings = super::super::provider_settings(&info).unwrap();
        let provider = settings["provider"].clone();
        let mut storage = Storage::open_at(&path).unwrap();
        super::super::apply_at(&config, settings, Some(&mut storage)).unwrap();
        assert_eq!(
            super::super::load(&config).unwrap()["lastUsedProvider"],
            provider
        );
        assert_eq!(storage.read().unwrap()["lastProvider"], provider);
    }

    #[test]
    fn desktop_apply_rolls_back_providers_on_selection_failure() {
        let dir = Snapshot(
            std::env::temp_dir().join(format!("echobird-cline-rollback-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir(&dir.0).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE ItemTable(key TEXT UNIQUE, value BLOB NOT NULL);")
            .unwrap();
        let selection = json!({"lastProvider":"native","lastModelByProvider":{"native":"old"}});
        let bytes: Vec<u8> = selection
            .to_string()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        db.execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            rusqlite::params![KEY, bytes],
        )
        .unwrap();
        db.execute_batch("CREATE TRIGGER reject_change BEFORE UPDATE ON ItemTable BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        let mut storage = Storage::Sqlite(db);
        let path = dir.0.join("providers.json");
        let info = serde_json::from_value(
            json!({"model":"test","baseUrl":"https://example.test/v1","apiKey":"fixture"}),
        )
        .unwrap();
        let settings = super::super::provider_settings(&info).unwrap();
        assert!(super::super::apply_at(&path, settings.clone(), Some(&mut storage)).is_err());
        assert!(!path.exists());
        assert_eq!(storage.read().unwrap(), selection);
        let original = json!({"version":1,"providers":{"native":{"settings":{"apiKey":"keep"}}},"lastUsedProvider":"native"});
        fs::write(&path, original.to_string()).unwrap();
        assert!(super::super::apply_at(&path, settings.clone(), Some(&mut storage)).is_err());
        assert_eq!(super::super::load(&path).unwrap(), original);
        #[cfg(windows)]
        let db = match &storage {
            Storage::Sqlite(db) => db,
            Storage::Level(_) => panic!("expected SQLite"),
        };
        #[cfg(not(windows))]
        let Storage::Sqlite(db) = &storage;
        db.execute_batch("DROP TRIGGER reject_change;").unwrap();
        super::super::apply_at(&path, settings, Some(&mut storage)).unwrap();
        let state = super::super::load(&path).unwrap();
        assert_eq!(state["lastUsedProvider"], "openai-compatible");
        assert_eq!(
            state["providers"]["native"],
            original["providers"]["native"]
        );
        assert_eq!(storage.read().unwrap()["lastProvider"], "openai-compatible");
    }
    #[test]
    fn webkit_selection_roundtrip_preserves_other_preferences_and_read_is_passive() {
        let dir = Snapshot(
            std::env::temp_dir().join(format!("echobird-cline-sqlite-{}", uuid::Uuid::new_v4())),
        );
        fs::create_dir(&dir.0).unwrap();
        let path = dir.0.join("tauri_localhost_0.localstorage");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE ItemTable(key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL ON CONFLICT FAIL);").unwrap();
        let original =
            json!({"lastProvider":"native","lastModelByProvider":{"native":"before"},"keep":true});
        let bytes: Vec<u8> = original
            .to_string()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        db.execute(
            "INSERT INTO ItemTable VALUES (?1, ?2)",
            rusqlite::params![KEY, bytes],
        )
        .unwrap();
        db.execute("INSERT INTO ItemTable VALUES ('theme', X'010203')", [])
            .unwrap();
        drop(db);
        let before = fs::read(&path).unwrap();
        assert_eq!(read_at(&path).unwrap(), original);
        assert_eq!(fs::read(&path).unwrap(), before);
        let mut storage = Storage::open_at(&path).unwrap();
        storage
            .write(&select(original, "echobird-desktop", "模型/test").unwrap())
            .unwrap();
        assert_eq!(
            storage.read().unwrap()["lastModelByProvider"]["echobird-desktop"],
            "模型/test"
        );
        drop(storage);
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row::<Vec<u8>, _, _>(
                "SELECT value FROM ItemTable WHERE key='theme'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
            vec![1, 2, 3]
        );
    }

    #[cfg(windows)]
    #[test]
    fn webview2_selection_roundtrip_preserves_other_keys_and_read_is_passive() {
        let dir = Snapshot(
            std::env::temp_dir().join(format!("echobird-cline-level-{}", uuid::Uuid::new_v4())),
        );
        let mut db = rusty_leveldb::DB::open(&dir.0, Default::default()).unwrap();
        db.put(
            LEVEL_KEY,
            b"\x01{\"lastProvider\":\"native\",\"lastModelByProvider\":{\"native\":\"old\"}}",
        )
        .unwrap();
        db.put(b"unrelated", b"preserve").unwrap();
        db.flush().unwrap();
        drop(db);
        let current = fs::read(dir.0.join("CURRENT")).unwrap();
        let selection = read_at(&dir.0).unwrap();
        assert_eq!(fs::read(dir.0.join("CURRENT")).unwrap(), current);
        let mut storage = Storage::open_at(&dir.0).unwrap();
        storage
            .write(&select(selection, "echobird-desktop", "模型/test").unwrap())
            .unwrap();
        assert_eq!(
            storage.read().unwrap()["lastModelByProvider"]["native"],
            "old"
        );
        assert_eq!(
            storage.read().unwrap()["lastModelByProvider"]["echobird-desktop"],
            "模型/test"
        );
        let Storage::Level(db) = &mut storage else {
            panic!("expected LevelDB")
        };
        assert_eq!(&*db.get(b"unrelated").unwrap(), b"preserve");
    }
}
