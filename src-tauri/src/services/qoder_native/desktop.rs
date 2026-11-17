use crate::services::{
    electron_storage,
    native_account_files::DirectoryLock,
    qoder_accounts::{Credentials, Edition},
};
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const USER: &str = "secret://aicoding.auth.userInfo";
const KEYS: [&str; 4] = [
    USER,
    "secret://aicoding.auth.userPlan",
    "secret://aicoding.auth.creditUsage",
    "secret://aicoding.auth.enterpriseUsage",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProductKind {
    Ide,
    App,
}

fn product(executable: &Path) -> Option<Value> {
    let resolved = std::fs::canonicalize(executable).ok();
    let executable = resolved.as_deref().unwrap_or(executable);
    let directory = if cfg!(target_os = "macos") {
        executable.parent()?.parent()?.join("Resources")
    } else {
        executable.parent()?.join("resources")
    };
    let bytes = std::fs::read(directory.join("product.json"))
        .ok()
        .or_else(|| std::fs::read(directory.join("app/product.json")).ok())
        .or_else(|| {
            std::fs::read(
                executable
                    .parent()?
                    .parent()?
                    .join("resources/app/product.json"),
            )
            .ok()
        })?;
    serde_json::from_slice(&bytes).ok()
}
pub(crate) fn identity(executable: &Path) -> Option<(Edition, ProductKind)> {
    let metadata = product(executable)?;
    if metadata.get("productId").is_some() {
        return match (
            metadata["productId"].as_str()?,
            metadata["appId"].as_str()?,
            metadata["dataDirectoryName"].as_str()?,
            metadata["protocolScheme"].as_str()?,
        ) {
            ("qoder", "com.qoder.app", "com.qoder.app.stable", "qoder") => {
                Some((Edition::Qoder, ProductKind::App))
            }
            ("qoder-cn", "com.qodercn.app", "com.qodercn.app.stable", "qoder-cn") => {
                Some((Edition::QoderCn, ProductKind::App))
            }
            _ => None,
        };
    }
    match (
        metadata["nameShort"].as_str()?,
        metadata["dataFolderName"].as_str()?,
        metadata["urlProtocol"].as_str()?,
    ) {
        ("Qoder", ".qoder", "qoder") => Some((Edition::Qoder, ProductKind::Ide)),
        ("QoderCN", ".qoder-cn", "qoder-cn") => Some((Edition::QoderCn, ProductKind::Ide)),
        _ => None,
    }
}
pub(crate) fn edition(executable: &Path) -> Option<Edition> {
    identity(executable).map(|(edition, _)| edition)
}
fn data_dir(edition: Edition) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("accountError.home")?;
    let appdata = dirs::config_dir().ok_or("accountError.home")?;
    Ok(resolve_dir(std::env::consts::OS, &home, &appdata, edition))
}
fn resolve_dir(platform: &str, home: &Path, config: &Path, edition: Edition) -> PathBuf {
    let name = if edition == Edition::Qoder {
        "Qoder"
    } else {
        "QoderCN"
    };
    if platform == "macos" {
        home.join("Library/Application Support").join(name)
    } else {
        config.join(name)
    }
}
fn profile(c: &Credentials, status: &Value, policy: &Value) -> Result<Value, String> {
    if status["id"].as_str() != Some(c.user_id.as_str()) {
        return Err("accountError.invalidAccount".into());
    }
    let whitelist = match status["whitelistStatus"].as_str() {
        Some("PASS") => 3,
        Some("NoLicense") => 5,
        Some("NoQuota" | "EXPIRED") => 6,
        _ => return Err("accountError.loginRequired".into()),
    };
    let mut value = json!({"id":c.user_id,"token":c.device_token,"refreshToken":c.device_refresh,"expireTime":c.device_expires_at.to_string(),"name":c.name,"email":c.email,"status":2,"whitelist":whitelist});
    for key in [
        "name",
        "email",
        "avatarUrl",
        "userType",
        "userTag",
        "isSubAccount",
        "quota",
        "isQuotaExceeded",
        "orgId",
        "orgName",
        "yxUid",
        "staffId",
        "cloudType",
        "isPrivacyPolicyModifiable",
        "isPrivacyPolicyVisible",
        "featureSwitches",
    ] {
        if let Some(v) = status.get(key) {
            value[key] = v.clone();
        }
    }
    if policy["success"] == true {
        if let Some(agreed) = policy["result"]["status"].as_str() {
            value["privacyPolicyAgreed"] = json!(agreed == "AGREE");
        }
    }
    if c.edition == Edition::QoderCn {
        value["login_source"] = json!("qodercn");
    }
    Ok(value)
}
async fn load_profile(c: &Credentials) -> Result<Value, String> {
    load_profile_at(c, c.edition.openapi()).await
}
async fn load_profile_at(c: &Credentials, base: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "accountError.network")?;
    let fetch = |path: &str| {
        client
            .get(format!("{base}{path}"))
            .bearer_auth(&c.device_token)
            .header("Accept", "application/json")
            .header("Cosy-ClientType", "0")
    };
    let response = fetch("/api/v3/user/status")
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    let status = response
        .json::<Value>()
        .await
        .map_err(|_| "accountError.format")?;
    let response = fetch("/api/v2/config/getDataPolicy")
        .query(&[("requestId", uuid::Uuid::new_v4().to_string())])
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    let policy = response
        .json::<Value>()
        .await
        .map_err(|_| "accountError.format")?;
    profile(c, &status, &policy)
}

pub(crate) struct NativeDesktopTransaction {
    db: Connection,
    original: Vec<(String, Option<SqlValue>)>,
    committed: bool,
    _lock: DirectoryLock,
}
impl NativeDesktopTransaction {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for NativeDesktopTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let result = (|| {
            let tx = self.db.transaction().map_err(|_| ())?;
            for (key, value) in &self.original {
                match value {
                    Some(value) => {
                        tx.execute(
                            "INSERT OR REPLACE INTO ItemTable(key,value) VALUES (?1,?2)",
                            rusqlite::params![key, value],
                        )
                        .map_err(|_| ())?;
                    }
                    None => {
                        tx.execute("DELETE FROM ItemTable WHERE key=?1", [key])
                            .map_err(|_| ())?;
                    }
                }
            }
            tx.commit().map_err(|_| ())
        })();
        if result.is_err() {
            log::error!("Failed to restore Qoder desktop account configuration");
        }
    }
}
pub(crate) async fn apply(
    c: &Credentials,
    executable: &Path,
) -> Result<NativeDesktopTransaction, String> {
    if identity(executable) != Some((c.edition, ProductKind::Ide)) {
        return Err("accountError.invalidAccount".into());
    }
    let directory = data_dir(c.edition)?;
    if !directory.join("User/globalStorage/state.vscdb").is_file() {
        return Err("accountError.initializeClient".into());
    }
    let profile = load_profile(c).await?;
    if crate::services::process_manager::desktop_tool_is_running(c.edition.tool()).await? {
        return Err("accountError.closeClient".into());
    }
    let cipher = cipher(&directory)?;
    let encrypted = electron_storage::encrypt_bytes(
        &cipher,
        &serde_json::to_vec(&profile).map_err(|_| "accountError.format")?,
    )?;
    write(
        &directory,
        &json!({"type":"Buffer","data":encrypted}).to_string(),
    )
}
fn cipher(directory: &Path) -> Result<electron_storage::Cipher, String> {
    #[cfg(target_os = "linux")]
    {
        let db = Connection::open_with_flags(
            directory.join("User/globalStorage/state.vscdb"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|_| "accountError.read")?;
        let raw = db
            .query_row("SELECT value FROM ItemTable WHERE key=?1", [USER], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(|_| "accountError.read")?;
        if raw
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| serde_json::from_value::<Vec<u8>>(value["data"].clone()).ok())
            .is_some_and(|bytes| bytes.len() > 3 && bytes.starts_with(b"v10"))
        {
            let mut key = [0u8; 16];
            pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"peanuts", b"saltysalt", 1, &mut key);
            return Ok(electron_storage::Cipher::Cbc {
                key,
                prefix: *b"v10",
                linux: true,
            });
        }
    }
    electron_storage::cipher(directory)
}
fn write(directory: &Path, encrypted: &str) -> Result<NativeDesktopTransaction, String> {
    let path = directory.join("User/globalStorage/state.vscdb");
    let lock = DirectoryLock::acquire(&directory.join("User/globalStorage/.echobird-auth"))?;
    let mut db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| "accountError.initializeClient")?;
    db.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "accountError.write")?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "accountError.busy")?;
    let mut original = vec![];
    for key in KEYS {
        let previous = tx
            .query_row("SELECT value FROM ItemTable WHERE key=?1", [key], |row| {
                row.get::<_, SqlValue>(0)
            })
            .optional()
            .map_err(|_| "accountError.read")?;
        original.push((key.into(), previous));
    }
    tx.execute(
        "INSERT OR REPLACE INTO ItemTable(key,value) VALUES(?1,?2)",
        rusqlite::params![USER, encrypted],
    )
    .map_err(|_| "accountError.write")?;
    for key in &KEYS[1..] {
        tx.execute("DELETE FROM ItemTable WHERE key=?1", [key])
            .map_err(|_| "accountError.write")?;
    }
    tx.commit().map_err(|_| "accountError.write")?;
    Ok(NativeDesktopTransaction {
        db,
        original,
        committed: false,
        _lock: lock,
    })
}

pub(super) fn discover(edition: Edition) -> Result<Option<Credentials>, String> {
    let directory = data_dir(edition)?;
    let path = directory.join("User/globalStorage/state.vscdb");
    if !path.is_file() {
        return Ok(None);
    }
    discover_at(edition, &directory, &cipher(&directory)?)
}
fn discover_at(
    edition: Edition,
    directory: &Path,
    key: &electron_storage::Cipher,
) -> Result<Option<Credentials>, String> {
    let path = directory.join("User/globalStorage/state.vscdb");
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "accountError.read")?;
    let bytes: Option<String> = db
        .query_row("SELECT value FROM ItemTable WHERE key=?1", [USER], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| "accountError.read")?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let encoded: Value = serde_json::from_str(&bytes).map_err(|_| "accountError.format")?;
    let bytes: Vec<u8> =
        serde_json::from_value(encoded["data"].clone()).map_err(|_| "accountError.format")?;
    let plain = electron_storage::decrypt_bytes(key, &bytes)?;
    let value: Value = serde_json::from_slice(&plain).map_err(|_| "accountError.format")?;
    let machine: String = db
        .query_row(
            "SELECT value FROM ItemTable WHERE key='storage.serviceMachineId'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "accountError.read")?;
    super::device_credentials(edition, &value, &machine, false).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scanned_ide_device_snapshot_reads_encrypted_text_without_touching_database() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path();
        std::fs::create_dir_all(directory.join("User/globalStorage")).unwrap();
        let path = directory.join("User/globalStorage/state.vscdb");
        for key in electron_storage::test_ciphers() {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS ItemTable(key TEXT PRIMARY KEY,value TEXT);",
            )
            .unwrap();
            let profile = json!({"id":"fixture-user","email":"fixture@example.test","token":"device-access","refreshToken":"device-refresh","expireTime":"4070908800000"});
            let bytes =
                electron_storage::encrypt_bytes(&key, profile.to_string().as_bytes()).unwrap();
            let row = json!({"type":"Buffer","data":bytes}).to_string();
            db.execute(
                "INSERT OR REPLACE INTO ItemTable VALUES(?1,?2)",
                rusqlite::params![USER, row],
            )
            .unwrap();
            db.execute("INSERT OR REPLACE INTO ItemTable VALUES('storage.serviceMachineId','fixture-machine')",[]).unwrap();
            drop(db);
            let before = std::fs::read(&path).unwrap();
            let c = discover_at(Edition::Qoder, directory, &key)
                .unwrap()
                .unwrap();
            assert!(c.native_device_only);
            assert_eq!(c.device_token, "device-access");
            assert_eq!(c.machine_id, "fixture-machine");
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    fn credentials(edition: Edition) -> Credentials {
        Credentials {
            edition,
            access_token: "job-secret".into(),
            refresh_token: "job-refresh".into(),
            expires_at: 2_000_000_000_000,
            user_id: "fixture".into(),
            name: "name".into(),
            email: "mail".into(),
            machine_id: "machine".into(),
            device_token: "device-token".into(),
            device_refresh: "device-refresh".into(),
            device_expires_at: 2_000_000_000_000,
            device_chat: false,
            native_device_only: false,
        }
    }
    #[test]
    fn desktop_metadata_and_three_platform_paths_are_region_bound() {
        let root = std::env::temp_dir().join(format!("qoder-product-{}", uuid::Uuid::new_v4()));
        let exe = if cfg!(target_os = "macos") {
            root.join("Contents/MacOS/Qoder")
        } else {
            root.join("Qoder.exe")
        };
        let metadata = if cfg!(target_os = "macos") {
            root.join("Contents/Resources/app")
        } else {
            root.join("resources/app")
        };
        std::fs::create_dir_all(&metadata).unwrap();
        for (e, name, data, protocol) in [
            (Edition::Qoder, "Qoder", ".qoder", "qoder"),
            (Edition::QoderCn, "QoderCN", ".qoder-cn", "qoder-cn"),
        ] {
            std::fs::write(
                metadata.join("product.json"),
                json!({"nameShort":name,"dataFolderName":data,"urlProtocol":protocol}).to_string(),
            )
            .unwrap();
            assert_eq!(edition(&exe), Some(e));
        }
        std::fs::write(
            metadata.join("product.json"),
            json!({"nameShort":"QoderWork","dataFolderName":".qoder","urlProtocol":"qoder"})
                .to_string(),
        )
        .unwrap();
        assert_eq!(edition(&exe), None);
        for (os, name) in [
            ("windows", "Qoder"),
            ("linux", "QoderCN"),
            ("macos", "Qoder"),
        ] {
            let e = if name == "QoderCN" {
                Edition::QoderCn
            } else {
                Edition::Qoder
            };
            let home = Path::new("home");
            let config = Path::new("config");
            let expected = if os == "macos" {
                home.join("Library/Application Support").join(name)
            } else {
                config.join(name)
            };
            assert_eq!(resolve_dir(os, home, config, e), expected);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn profile_uses_device_pair_and_actual_vendor_status_without_fake_quota() {
        let c = credentials(Edition::QoderCn);
        let status = json!({"id":"fixture","name":"actual","whitelistStatus":"PASS","userType":"personal_standard"});
        let p = profile(
            &c,
            &status,
            &json!({"success":true,"result":{"status":"DISAGREE"}}),
        )
        .unwrap();
        assert_eq!(p["token"], "device-token");
        assert_eq!(p["refreshToken"], "device-refresh");
        assert_eq!(p["expireTime"], "2000000000000");
        assert_eq!(p["status"], 2);
        assert_eq!(p["whitelist"], 3);
        assert_eq!(p["login_source"], "qodercn");
        assert_eq!(p["name"], "actual");
        assert_eq!(p["privacyPolicyAgreed"], false);
        assert!(p.get("quota").is_none());
        assert!(!p.to_string().contains("job-secret"));
        assert!(profile(
            &c,
            &json!({"id":"other","whitelistStatus":"PASS"}),
            &Value::Null
        )
        .is_err());
        assert!(profile(
            &c,
            &json!({"id":"fixture","whitelistStatus":"WAIT"}),
            &Value::Null
        )
        .is_err());
    }
    #[test]
    fn secret_storage_buffer_wrapper_round_trips_synthetic_native_cipher() {
        use aes_gcm::KeyInit;
        let cipher = electron_storage::Cipher::Gcm(Box::new(
            aes_gcm::Aes256Gcm::new_from_slice(&[7u8; 32]).unwrap(),
        ));
        let plain = profile(
            &credentials(Edition::Qoder),
            &json!({"id":"fixture","whitelistStatus":"PASS"}),
            &Value::Null,
        )
        .unwrap()
        .to_string();
        let bytes = electron_storage::encrypt_bytes(&cipher, plain.as_bytes()).unwrap();
        let stored = json!({"type":"Buffer","data":bytes}).to_string();
        let stored: Value = serde_json::from_str(&stored).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(stored["data"].clone()).unwrap();
        assert!(bytes.starts_with(b"v10"));
        assert_eq!(
            electron_storage::decrypt_bytes(&cipher, &bytes).unwrap(),
            plain.as_bytes()
        );
    }
    #[test]
    fn sqlite_switch_is_atomic_and_restores_raw_types_after_store_failure() {
        let root = std::env::temp_dir().join(format!("qoder-desktop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("User/globalStorage")).unwrap();
        let path = root.join("User/globalStorage/state.vscdb");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value BLOB);INSERT INTO ItemTable VALUES('theme',X'010203');").unwrap();
        db.execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            rusqlite::params![USER, "old-user"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            rusqlite::params![KEYS[1], vec![4u8, 5, 6]],
        )
        .unwrap();
        {
            let _guard = write(&root, "new-user").unwrap();
            assert_eq!(
                db.query_row("SELECT value FROM ItemTable WHERE key=?1", [USER], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
                "new-user"
            );
            assert!(write(&root, "racing").is_err());
        }
        assert_eq!(
            db.query_row("SELECT value FROM ItemTable WHERE key=?1", [USER], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "old-user"
        );
        assert_eq!(
            db.query_row("SELECT value FROM ItemTable WHERE key=?1", [KEYS[1]], |r| r
                .get::<_, Vec<u8>>(0))
                .unwrap(),
            vec![4, 5, 6]
        );
        db.execute_batch("CREATE TRIGGER deny_auth BEFORE INSERT ON ItemTable WHEN NEW.key='secret://aicoding.auth.userInfo' BEGIN SELECT RAISE(ABORT,'fixture failure');END;").unwrap();
        assert!(write(&root, "failed").is_err());
        assert_eq!(
            db.query_row("SELECT value FROM ItemTable WHERE key=?1", [USER], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "old-user"
        );
        db.execute_batch("DROP TRIGGER deny_auth").unwrap();
        write(&root, "committed").unwrap().commit();
        assert_eq!(
            db.query_row("SELECT value FROM ItemTable WHERE key='theme'", [], |r| r
                .get::<_, Vec<
                u8,
            >>(
                0
            ))
            .unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM ItemTable WHERE key=?1",
                [KEYS[1]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn explicit_profile_reads_only_status_and_policy_with_device_token() {
        use axum::{extract::Request, routing::get, Json, Router};
        let requests = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let capture = requests.clone();
        let app = Router::new().fallback(get(move |request: Request| {
            let capture = capture.clone();
            async move {
                assert_eq!(request.headers()["authorization"], "Bearer device-token");
                capture
                    .lock()
                    .unwrap()
                    .push(request.uri().path().to_string());
                Json(if request.uri().path() == "/api/v3/user/status" {
                    json!({"id":"fixture","whitelistStatus":"PASS"})
                } else {
                    json!({"success":true,"result":{"status":"AGREE"}})
                })
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let p = load_profile_at(&credentials(Edition::Qoder), &base)
            .await
            .unwrap();
        assert_eq!(p["privacyPolicyAgreed"], true);
        assert_eq!(
            *requests.lock().unwrap(),
            vec!["/api/v3/user/status", "/api/v2/config/getDataPolicy"]
        );
        server.abort();
    }
}
