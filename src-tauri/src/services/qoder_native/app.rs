use super::desktop::{self, ProductKind};
use crate::services::{
    electron_storage,
    native_account_files::{self as files, Transaction},
    qoder_accounts::{Credentials, Edition},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn resources(executable: &Path) -> Result<PathBuf, String> {
    let resolved = fs::canonicalize(executable).ok();
    let executable = resolved.as_deref().unwrap_or(executable);
    if cfg!(target_os = "macos") {
        Ok(executable
            .parent()
            .and_then(Path::parent)
            .ok_or("accountError.home")?
            .join("Resources"))
    } else {
        Ok(executable
            .parent()
            .ok_or("accountError.home")?
            .join("resources"))
    }
}
fn metadata(executable: &Path, edition: Edition) -> Result<Value, String> {
    if desktop::identity(executable) != Some((edition, ProductKind::App)) {
        return Err("accountError.invalidAccount".into());
    }
    serde_json::from_slice(
        &fs::read(resources(executable)?.join("product.json")).map_err(|_| "accountError.read")?,
    )
    .map_err(|_| "accountError.format".into())
}
fn data_dir(product: &Value) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("accountError.home")?;
    let config = dirs::config_dir().ok_or("accountError.home")?;
    resolve_dir(std::env::consts::OS, &home, &config, product)
}
fn resolve_dir(
    platform: &str,
    home: &Path,
    config: &Path,
    product: &Value,
) -> Result<PathBuf, String> {
    let name = product["dataDirectoryName"]
        .as_str()
        .filter(|name| matches!(*name, "com.qoder.app.stable" | "com.qodercn.app.stable"))
        .ok_or("accountError.format")?;
    Ok(if platform == "macos" {
        home.join("Library/Application Support")
    } else {
        config.to_path_buf()
    }
    .join(name))
}
fn text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value[*key].as_str().filter(|s| !s.is_empty()))
}
fn auth(c: &Credentials, user: &Value) -> Result<Value, String> {
    let id = text(user, &["id", "user_id", "uid"]).ok_or("accountError.format")?;
    if id != c.user_id {
        return Err("accountError.invalidAccount".into());
    }
    let expires = chrono::DateTime::from_timestamp_millis(c.device_expires_at)
        .ok_or("accountError.format")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(
        json!({"schemaVersion":1,"token":c.device_token,"refreshToken":c.device_refresh,"expiresAt":expires,
        "user":{"id":id,"name":text(user,&["name","username","user_name"]).unwrap_or(&c.name),"email":text(user,&["email"]).unwrap_or(&c.email),"phone":text(user,&["security_mobile"]),"avatarUrl":text(user,&["avatar_url","avatar","image_url"])}}),
    )
}
async fn load_auth(c: &Credentials, base: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "accountError.network")?;
    let response = client
        .get(format!("{base}/api/v1/userinfo"))
        .bearer_auth(&c.device_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    auth(
        c,
        &response
            .json::<Value>()
            .await
            .map_err(|_| "accountError.format")?,
    )
}
fn cipher(directory: &Path, product: &Value) -> Result<electron_storage::Cipher, String> {
    #[cfg(target_os = "linux")]
    if files::read_optional(&directory.join("auth.v1.dat"))?
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
    let name = product["electronApplicationName"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.format")?;
    electron_storage::cipher_with_name(directory, name)
}
fn active_user_id_at(
    directory: &Path,
    cipher: &electron_storage::Cipher,
) -> Result<String, String> {
    let bytes = files::read_optional(&directory.join("auth.v1.dat"))?
        .ok_or("Sign in to Qoder before configuring its model")?;
    let plain = electron_storage::decrypt_bytes(cipher, &bytes)?;
    let auth: Value = serde_json::from_slice(&plain).map_err(|_| "accountError.format")?;
    if auth["schemaVersion"] != 1 || auth["token"].as_str().is_none_or(|token| token.is_empty()) {
        return Err("accountError.format".into());
    }
    auth["user"]["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "accountError.format".into())
}
pub(crate) fn active_user_id(edition: Edition, executable: &Path) -> Result<String, String> {
    let product = metadata(executable, edition)?;
    let directory = data_dir(&product)?;
    if !directory.join("auth.v1.dat").is_file() {
        return Err("Sign in to Qoder before configuring its model".into());
    }
    active_user_id_at(&directory, &cipher(&directory, &product)?)
}
struct AuthLock {
    path: PathBuf,
    _file: fs::File,
}
impl AuthLock {
    fn acquire(directory: &Path) -> Result<Self, String> {
        let path = directory.join("auth.v1.lock");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "accountError.busy"
            } else {
                "accountError.write"
            }
        })?;
        Ok(Self { path, _file: file })
    }
}
impl Drop for AuthLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
pub(crate) struct NativeAppTransaction {
    transaction: Option<Transaction>,
    _lock: AuthLock,
}
impl NativeAppTransaction {
    pub(crate) fn commit(mut self) {
        if let Some(transaction) = self.transaction.take() {
            transaction.commit();
        }
    }
}
fn preserve_profile(incoming: &mut Value, previous: &Value) -> Result<(), String> {
    let overlay = previous.get("profileOverlay");
    if previous["schemaVersion"] != 1
        || !previous["token"].is_string()
        || !previous["refreshToken"].is_string()
        || !previous["expiresAt"].is_string()
        || !previous["user"]["id"].is_string()
        || overlay.is_some_and(|v| {
            !v.is_object()
                || ["avatarUrl", "about"]
                    .iter()
                    .any(|key| v.get(*key).is_some_and(|v| !v.is_null() && !v.is_string()))
        })
        || previous
            .get("firstLoginOnboardingSeen")
            .is_some_and(|v| !v.is_boolean())
    {
        return Err("accountError.format".into());
    }
    if previous["user"]["id"] == incoming["user"]["id"] {
        for field in ["profileOverlay", "firstLoginOnboardingSeen"] {
            if let Some(value) = previous.get(field) {
                incoming[field] = value.clone();
            }
        }
    }
    Ok(())
}
fn write(
    directory: &Path,
    cipher: &electron_storage::Cipher,
    incoming: &Value,
) -> Result<NativeAppTransaction, String> {
    let lock = AuthLock::acquire(directory)?;
    let mut incoming = incoming.clone();
    if let Some(bytes) = files::read_optional(&directory.join("auth.v1.dat"))? {
        let plain = electron_storage::decrypt_bytes(cipher, &bytes)?;
        let previous: Value = serde_json::from_slice(&plain).map_err(|_| "accountError.format")?;
        preserve_profile(&mut incoming, &previous)?;
    }
    let bytes = electron_storage::encrypt_bytes(
        cipher,
        &serde_json::to_vec(&incoming).map_err(|_| "accountError.format")?,
    )?;
    let mut transaction = Transaction::new();
    transaction.replace(&directory.join("auth.v1.dat"), &bytes)?;
    Ok(NativeAppTransaction {
        transaction: Some(transaction),
        _lock: lock,
    })
}
pub(crate) async fn apply(
    c: &Credentials,
    executable: &Path,
) -> Result<NativeAppTransaction, String> {
    let product = metadata(executable, c.edition)?;
    let directory = data_dir(&product)?;
    if !directory.is_dir() || !directory.join("Local State").is_file() {
        return Err("accountError.initializeClient".into());
    }
    let auth = load_auth(c, c.edition.openapi()).await?;
    if desktop::identity(executable) != Some((c.edition, ProductKind::App)) {
        return Err("accountError.invalidAccount".into());
    }
    let cipher = cipher(&directory, &product)?;
    if crate::services::process_manager::desktop_tool_is_running(c.edition.tool()).await? {
        return Err("accountError.closeClient".into());
    }
    write(&directory, &cipher, &auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_cipher(key: u8) -> electron_storage::Cipher {
        use aes_gcm::KeyInit;
        electron_storage::Cipher::Gcm(Box::new(
            aes_gcm::Aes256Gcm::new_from_slice(&[key; 32]).unwrap(),
        ))
    }
    fn read_auth(directory: &Path, cipher: &electron_storage::Cipher) -> Value {
        serde_json::from_slice(
            &electron_storage::decrypt_bytes(
                cipher,
                &fs::read(directory.join("auth.v1.dat")).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn store_auth(directory: &Path, cipher: &electron_storage::Cipher, value: &Value) -> Vec<u8> {
        let bytes = electron_storage::encrypt_bytes(cipher, value.to_string().as_bytes()).unwrap();
        files::write(&directory.join("auth.v1.dat"), &bytes).unwrap();
        bytes
    }
    fn credentials() -> Credentials {
        Credentials {
            edition: Edition::QoderCn,
            access_token: "job-secret".into(),
            refresh_token: "job-refresh".into(),
            expires_at: 2_000_000_000_000,
            user_id: "fixture".into(),
            name: "fallback".into(),
            email: "fallback-mail".into(),
            machine_id: "machine".into(),
            device_token: "device-secret".into(),
            device_refresh: "device-refresh".into(),
            device_expires_at: 2_000_000_000_000,
            device_chat: false,
        }
    }
    #[test]
    fn model_selection_uses_native_signed_in_user_without_saved_account() {
        let root = std::env::temp_dir().join(format!("qoder-app-model-{}", uuid::Uuid::new_v4()));
        let cipher = test_cipher(11);
        assert_eq!(
            active_user_id_at(&root, &cipher).unwrap_err(),
            "Sign in to Qoder before configuring its model"
        );
        assert!(!root.exists());

        store_auth(
            &root,
            &cipher,
            &auth(&credentials(), &json!({"id":"fixture"})).unwrap(),
        );
        assert_eq!(active_user_id_at(&root, &cipher).unwrap(), "fixture");

        let mut signed_out = read_auth(&root, &cipher);
        signed_out["token"] = json!("");
        store_auth(&root, &cipher, &signed_out);
        assert!(active_user_id_at(&root, &cipher).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn standalone_schema_uses_raw_safe_storage_device_auth_and_iso_expiry() {
        use aes_gcm::KeyInit;
        let auth=auth(&credentials(),&json!({"id":"fixture","name":"actual","email":"mail","security_mobile":"masked","avatar_url":"https://example.test/avatar"})).unwrap();
        assert_eq!(auth["schemaVersion"], 1);
        assert_eq!(auth["token"], "device-secret");
        assert_eq!(auth["refreshToken"], "device-refresh");
        assert_eq!(auth["expiresAt"], "2033-05-18T03:33:20.000Z");
        assert_eq!(auth["user"]["name"], "actual");
        assert!(auth.get("profileOverlay").is_none());
        assert!(auth.get("refreshTokenExpiresAt").is_none());
        assert!(!auth.to_string().contains("job-secret"));
        let cipher = electron_storage::Cipher::Gcm(Box::new(
            aes_gcm::Aes256Gcm::new_from_slice(&[4u8; 32]).unwrap(),
        ));
        let bytes = electron_storage::encrypt_bytes(&cipher, auth.to_string().as_bytes()).unwrap();
        assert!(bytes.starts_with(b"v10"));
        let decoded: Value =
            serde_json::from_slice(&electron_storage::decrypt_bytes(&cipher, &bytes).unwrap())
                .unwrap();
        assert_eq!(decoded, auth);
        assert!(super::auth(&credentials(), &json!({"id":"other"})).is_err());
    }
    #[test]
    fn standalone_directories_use_appdata_product_ids_on_all_platforms() {
        for name in ["com.qoder.app.stable", "com.qodercn.app.stable"] {
            let metadata = json!({"dataDirectoryName":name});
            for os in ["windows", "macos", "linux"] {
                let home = Path::new("home");
                let config = Path::new("config");
                let path = resolve_dir(os, home, config, &metadata).unwrap();
                assert_eq!(
                    path,
                    if os == "macos" {
                        home.join("Library/Application Support").join(name)
                    } else {
                        config.join(name)
                    }
                );
            }
        }
        assert!(resolve_dir(
            "windows",
            Path::new("home"),
            Path::new("config"),
            &json!({"dataDirectoryName":"../foreign"})
        )
        .is_err());
    }
    #[test]
    fn native_file_lock_and_rollback_preserve_unrelated_data() {
        let root = std::env::temp_dir().join(format!("qoder-app-auth-{}", uuid::Uuid::new_v4()));
        let cipher = test_cipher(8);
        let incoming = auth(&credentials(), &json!({"id":"fixture"})).unwrap();
        let previous = json!({"schemaVersion":1,"token":"old-device","refreshToken":"old-refresh","expiresAt":"old-expiry","user":{"id":"fixture"},"profileOverlay":{"avatarUrl":"local-avatar","about":"local-about","name":"legacy-nickname"},"firstLoginOnboardingSeen":true});
        let old_bytes = store_auth(&root, &cipher, &previous);
        files::write(&root.join("qoder-data.v1.json"), b"sessions").unwrap();
        files::write(&root.join("auth-profile-overlays.v1.dat"), b"profiles").unwrap();
        {
            let transaction = write(&root, &cipher, &incoming).unwrap();
            let current = read_auth(&root, &cipher);
            assert_eq!(current["token"], "device-secret");
            assert_eq!(current["refreshToken"], "device-refresh");
            assert_eq!(current["expiresAt"], incoming["expiresAt"]);
            assert_eq!(current["profileOverlay"], previous["profileOverlay"]);
            assert_eq!(current["firstLoginOnboardingSeen"], true);
            assert!(write(&root, &cipher, &incoming).is_err());
            assert!(root.join("auth.v1.lock").is_file());
            drop(transaction);
        }
        assert_eq!(fs::read(root.join("auth.v1.dat")).unwrap(), old_bytes);
        assert!(!root.join("auth.v1.lock").exists());
        write(&root, &cipher, &incoming).unwrap().commit();
        assert_eq!(read_auth(&root, &cipher)["token"], "device-secret");
        assert_eq!(
            fs::read(root.join("qoder-data.v1.json")).unwrap(),
            b"sessions"
        );
        assert_eq!(
            fs::read(root.join("auth-profile-overlays.v1.dat")).unwrap(),
            b"profiles"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn switching_accounts_does_not_inherit_another_users_inline_profile() {
        let root = std::env::temp_dir().join(format!("qoder-app-other-{}", uuid::Uuid::new_v4()));
        let cipher = test_cipher(9);
        store_auth(
            &root,
            &cipher,
            &json!({"schemaVersion":1,"token":"other-device","refreshToken":"other-refresh","expiresAt":"other-expiry","user":{"id":"other"},"profileOverlay":{"avatarUrl":null,"about":"other-about","name":"other-name"},"firstLoginOnboardingSeen":true}),
        );
        let incoming = auth(&credentials(), &json!({"id":"fixture"})).unwrap();
        write(&root, &cipher, &incoming).unwrap().commit();
        let current = read_auth(&root, &cipher);
        assert_eq!(current, incoming);
        assert!(current.get("profileOverlay").is_none());
        assert!(current.get("firstLoginOnboardingSeen").is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unreadable_or_invalid_native_auth_is_never_overwritten() {
        let root = std::env::temp_dir().join(format!("qoder-app-invalid-{}", uuid::Uuid::new_v4()));
        let cipher = test_cipher(10);
        let incoming = auth(&credentials(), &json!({"id":"fixture"})).unwrap();
        let valid = json!({"schemaVersion":1,"token":"old","refreshToken":"refresh","expiresAt":"expiry","user":{"id":"fixture"}});
        let mut invalid = vec![];
        for (field, value) in [
            ("schemaVersion", json!(2)),
            ("token", Value::Null),
            ("profileOverlay", Value::Null),
            ("profileOverlay", json!({"avatarUrl":1})),
            ("profileOverlay", json!({"about":false})),
            ("firstLoginOnboardingSeen", json!("true")),
            ("user", json!({"id":0})),
        ] {
            let mut value_before = valid.clone();
            value_before[field] = value;
            invalid.push(value_before);
        }
        for previous in invalid {
            let before = store_auth(&root, &cipher, &previous);
            assert!(write(&root, &cipher, &incoming).is_err());
            assert_eq!(fs::read(root.join("auth.v1.dat")).unwrap(), before);
            assert!(!root.join("auth.v1.lock").exists());
        }
        let before = store_auth(&root, &test_cipher(11), &valid);
        assert!(write(&root, &cipher, &incoming).is_err());
        assert_eq!(fs::read(root.join("auth.v1.dat")).unwrap(), before);
        let malformed = electron_storage::encrypt_bytes(&cipher, b"{").unwrap();
        files::write(&root.join("auth.v1.dat"), &malformed).unwrap();
        assert!(write(&root, &cipher, &incoming).is_err());
        assert_eq!(fs::read(root.join("auth.v1.dat")).unwrap(), malformed);
        fs::remove_file(root.join("auth.v1.dat")).unwrap();
        fs::create_dir(root.join("auth.v1.dat")).unwrap();
        assert!(write(&root, &cipher, &incoming).is_err());
        assert!(root.join("auth.v1.dat").is_dir());
        assert!(!root.join("auth.v1.lock").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn native_user_projection_only_queries_userinfo_with_selected_device_token() {
        use axum::{extract::Request, routing::get, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let count = Arc::new(AtomicUsize::new(0));
        let requests = count.clone();
        let app = Router::new().route(
            "/api/v1/userinfo",
            get(move |request: Request| {
                let requests = requests.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(request.headers()["authorization"], "Bearer device-secret");
                    Json(json!({"uid":"fixture","username":"actual","email":"mail"}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let auth = load_auth(&credentials(), &base).await.unwrap();
        assert_eq!(auth["user"]["id"], "fixture");
        assert_eq!(auth["user"]["name"], "actual");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
