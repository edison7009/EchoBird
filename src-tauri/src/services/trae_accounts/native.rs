use super::Edition;
use aes::Aes128;
use base64::{engine::general_purpose::STANDARD, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::path::{Path, PathBuf};

const AUTH: &str = "iCubeAuthInfo://icube.cloudide";
const DEVICE: &str = "iCubeAuthInfo://icube-dc:";
const USER_TAG: &str = "iCubeAuthInfo://usertag";
const HEADER: [u8; 6] = [116, 99, 5, 16, 0, 0];
const PRIVATE_HEADER: [u8; 6] = [18, 57, 32, 32, 2, 3];
const SALT: &str = "4dd4c2e6b83162090e52b3c7a6733ba41cb2462b829ab58a196b39db57177524f49baf7f08e8d68d26a72e37c1a95a2f1f05a51892aef2949732b62a38aadd58";
const PRIVATE_SALT: &str = "490cc21292b05d0cc06ccbe91fb92e21b5a7c538811dc85e27a96e28104e1cf69c7166efab232807390da78a8b91e49c44b49a43feb941a1e46f2656e613e47b";

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct KeyPair {
    #[serde(rename = "privateKeyPEM")]
    pub private_key: String,
    #[serde(rename = "publicKeyPEM")]
    pub public_key: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Identity {
    pub device_id: String,
    pub machine_id: String,
    pub app_version: String,
    pub client_id: String,
    pub key_pair: Option<KeyPair>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Credentials {
    pub auth: Value,
    pub identity: Identity,
}

pub(super) fn data_candidates(platform: &str, root: &Path, edition: Edition) -> Vec<PathBuf> {
    let names: &[&str] = match (platform, edition) {
        ("linux", Edition::Traecn) => &["trae-cn", "Trae CN"],
        ("linux", Edition::Trae) => &["trae", "Trae"],
        ("windows", Edition::Traecn) => &["Trae CN", "trae-cn"],
        (_, Edition::Traecn) => &["Trae CN"],
        (_, Edition::Trae) => &["Trae"],
    };
    names.iter().map(|name| root.join(name)).collect()
}

pub(super) fn data_dir(edition: Edition) -> Result<PathBuf, String> {
    let root = dirs::config_dir().ok_or("accountError.home")?;
    let candidates = data_candidates(std::env::consts::OS, &root, edition);
    Ok(candidates
        .iter()
        .find(|dir| dir.join("User/globalStorage/storage.json").is_file())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone()))
}

pub(super) fn storage_path(dir: &Path) -> PathBuf {
    dir.join("User/globalStorage/storage.json")
}

fn material(random: &[u8], private: bool) -> Result<[u8; 64], String> {
    let salt = hex::decode(if private { PRIVATE_SALT } else { SALT })
        .map_err(|_| "accountError.format")?;
    let mut digest = Sha512::new();
    digest.update(Sha512::digest(random));
    digest.update(salt);
    Ok(digest.finalize().into())
}

pub(super) fn decode(value: &Value) -> Result<Value, String> {
    if value.is_object() {
        return Ok(value.clone());
    }
    let text = value.as_str().ok_or("accountError.format")?.trim();
    if text.starts_with('{') {
        return serde_json::from_str(text).map_err(|_| "accountError.format".into());
    }
    let bytes = STANDARD.decode(text).map_err(|_| "accountError.format")?;
    if bytes.len() < 118 || (bytes.len() - 38) % 16 != 0 {
        return Err("accountError.format".into());
    }
    let private = if bytes[..6] == HEADER {
        false
    } else if bytes[..6] == PRIVATE_HEADER {
        true
    } else {
        return Err("accountError.format".into());
    };
    let key = material(&bytes[6..38], private)?;
    let plain = cbc::Decryptor::<Aes128>::new_from_slices(&key[..16], &key[16..32])
        .map_err(|_| "accountError.format")?
        .decrypt_padded_vec_mut::<Pkcs7>(&bytes[38..])
        .map_err(|_| "accountError.format")?;
    if plain.len() < 64 || Sha512::digest(&plain[64..]).as_slice() != &plain[..64] {
        return Err("accountError.format".into());
    }
    serde_json::from_slice(&plain[64..]).map_err(|_| "accountError.format".into())
}

pub(super) fn encode(value: &Value) -> Result<Value, String> {
    let plain = serde_json::to_vec(value).map_err(|_| "accountError.format")?;
    let mut random = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let key = material(&random, false)?;
    let mut authenticated = Sha512::digest(&plain).to_vec();
    authenticated.extend_from_slice(&plain);
    let cipher = cbc::Encryptor::<Aes128>::new_from_slices(&key[..16], &key[16..32])
        .map_err(|_| "accountError.format")?
        .encrypt_padded_vec_mut::<Pkcs7>(&authenticated);
    let mut bytes = HEADER.to_vec();
    bytes.extend_from_slice(&random);
    bytes.extend_from_slice(&cipher);
    Ok(Value::String(STANDARD.encode(bytes)))
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|s| !s.trim().is_empty())
}

pub(super) fn access(auth: &Value) -> Result<&str, String> {
    text(auth, "token")
        .or_else(|| text(auth, "accessToken"))
        .ok_or_else(|| "accountError.invalidAccount".into())
}

pub(super) fn region(auth: &Value) -> Option<Edition> {
    let region = auth["userRegion"]["region"]
        .as_str()
        .or_else(|| auth["userRegion"].as_str());
    match region.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("cn") => return Some(Edition::Traecn),
        Some("sg" | "ai" | "us" | "usttp") => return Some(Edition::Trae),
        _ => {}
    }
    let host = text(auth, "host").or_else(|| text(auth, "loginHost"))?;
    let url = url::Url::parse(host).ok()?;
    match url.host_str()? {
        host if host == "trae.cn"
            || host.ends_with(".trae.cn")
            || host.ends_with(".trae.com.cn") =>
        {
            Some(Edition::Traecn)
        }
        host if host == "trae.ai"
            || host.ends_with(".trae.ai")
            || host.ends_with(".traeapi.us") =>
        {
            Some(Edition::Trae)
        }
        _ => None,
    }
}

pub(super) fn product(edition: Edition) -> Option<Value> {
    let executable = super::super::tool_manager::get_tool_exe_path(edition.tool())?;
    super::super::cursor_auth::read(&product_path(Path::new(&executable), std::env::consts::OS)?)
        .ok()
}

fn product_path(executable: &Path, platform: &str) -> Option<PathBuf> {
    let executable = std::fs::canonicalize(executable).unwrap_or_else(|_| executable.to_path_buf());
    let root = if platform == "macos" {
        executable
            .ancestors()
            .find(|dir| dir.extension().is_some_and(|ext| ext == "app"))?
            .join("Contents/Resources/app")
    } else if executable
        .parent()?
        .file_name()
        .is_some_and(|name| name == "bin")
    {
        executable.parent()?.parent()?.join("resources/app")
    } else {
        executable.parent()?.join("resources/app")
    };
    Some(root.join("product.json"))
}

pub(super) fn identity(root: &Value, edition: Edition) -> Result<Identity, String> {
    if !root.is_object() {
        return Err("accountError.format".into());
    }
    let keys: Vec<_> = root
        .as_object()
        .ok_or("accountError.format")?
        .keys()
        .filter(|key| key.starts_with(DEVICE))
        .collect();
    if keys.len() > 1 {
        return Err("accountError.invalidAccount".into());
    }
    let device_id = keys
        .first()
        .map(|key| key.trim_start_matches(DEVICE).to_owned())
        .unwrap_or_default();
    let key_pair = keys
        .first()
        .map(|key| {
            serde_json::from_value::<KeyPair>(decode(&root[*key])?)
                .map_err(|_| String::from("accountError.format"))
        })
        .transpose()?;
    let product = product(edition).unwrap_or(Value::Null);
    Ok(Identity {
        device_id,
        machine_id: text(root, "telemetry.machineId").unwrap_or_default().into(),
        app_version: text(&product, "appVersion").unwrap_or_default().into(),
        client_id: product["iCubeApp"]["authConfig"]["TRAE"]
            [text(&product, "quality").unwrap_or("stable")]
        .as_str()
        .unwrap_or("ono9krqynydwx5")
        .into(),
        key_pair,
    })
}

pub(super) fn read(dir: &Path, edition: Edition) -> Result<Option<Credentials>, String> {
    let path = storage_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let root: Value = super::super::cursor_auth::read(&path)?;
    let Some(value) = root.get(AUTH) else {
        return Ok(None);
    };
    let auth = decode(value)?;
    if region(&auth).is_some_and(|native| native != edition) {
        return Err("accountError.invalidAccount".into());
    }
    access(&auth)?;
    Ok(Some(Credentials {
        auth,
        identity: identity(&root, edition)?,
    }))
}

pub(super) fn login_identity(dir: &Path, edition: Edition) -> Result<Identity, String> {
    let path = storage_path(dir);
    if !path.is_file() {
        return Err("accountError.initializeClient".into());
    }
    let root: Value = super::super::cursor_auth::read(&path)?;
    let identity = identity(&root, edition)?;
    if identity.device_id.is_empty()
        || identity.machine_id.is_empty()
        || identity.key_pair.is_none()
    {
        return Err("accountError.initializeClient".into());
    }
    Ok(identity)
}

pub(super) fn validate_identity(current: &Identity, saved: &Identity) -> Result<(), String> {
    if current.device_id != saved.device_id
        || current.machine_id != saved.machine_id
        || current.client_id != saved.client_id
    {
        return Err("accountError.invalidAccount".into());
    }
    if let (Some(current), Some(saved)) = (&current.key_pair, &saved.key_pair) {
        if current.public_key != saved.public_key || current.private_key != saved.private_key {
            return Err("accountError.invalidAccount".into());
        }
    }
    Ok(())
}

pub(super) fn apply(dir: &Path, credentials: &Credentials) -> Result<(), String> {
    let path = storage_path(dir);
    if !path.is_file() {
        return Err("accountError.initializeClient".into());
    }
    let mut root: Value = super::super::cursor_auth::read(&path)?;
    apply_to_document(&mut root, credentials)?;
    super::super::cursor_auth::write(&path, &root)
}

fn apply_to_document(root: &mut Value, credentials: &Credentials) -> Result<(), String> {
    let object = root.as_object_mut().ok_or("accountError.format")?;
    let current_device = object
        .keys()
        .filter_map(|key| key.strip_prefix(DEVICE))
        .collect::<Vec<_>>();
    if current_device.len() != 1 || current_device[0] != credentials.identity.device_id {
        return Err("accountError.invalidAccount".into());
    }
    if let Some(saved_pair) = &credentials.identity.key_pair {
        let current_pair: KeyPair = serde_json::from_value(decode(
            &object[&format!("{DEVICE}{}", credentials.identity.device_id)],
        )?)
        .map_err(|_| "accountError.format")?;
        if current_pair.public_key != saved_pair.public_key
            || current_pair.private_key != saved_pair.private_key
        {
            return Err("accountError.invalidAccount".into());
        }
    }
    let auth = credentials.auth.clone();
    access(&auth)?;
    object.insert(AUTH.into(), encode(&auth)?);
    if let (Some(user), Some(tag)) = (text(&auth, "userId"), text(&auth["account"], "userTag")) {
        let mut tags = match object.get(USER_TAG) {
            Some(value) => decode(value)?,
            None => json!({}),
        };
        tags.as_object_mut()
            .ok_or("accountError.format")?
            .insert(user.into(), tag.to_ascii_lowercase().into());
        object.insert(USER_TAG.into(), encode(&tags)?);
    }
    Ok(())
}

pub(super) fn timestamp(value: &Value) -> Option<i64> {
    timestamp_ms(value).map(|value| value / 1000)
}

pub(super) fn timestamp_ms(value: &Value) -> Option<i64> {
    let number = value.as_f64().or_else(|| value.as_str()?.parse().ok());
    if let Some(number) = number.filter(|v| v.is_finite() && *v > 0.0) {
        return Some(if number > 1e12 {
            number as i64
        } else {
            (number * 1000.0) as i64
        });
    }
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|date| date.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_codec_roundtrip_and_integrity() {
        let value = json!({"token":"fixture-token", "userId":"fixture-user"});
        let encoded = encode(&value).unwrap();
        assert_eq!(decode(&encoded).unwrap(), value);
        let mut bytes = STANDARD.decode(encoded.as_str().unwrap()).unwrap();
        bytes[70] ^= 1;
        assert!(decode(&Value::String(STANDARD.encode(bytes))).is_err());
        assert_eq!(decode(&Value::String(value.to_string())).unwrap(), value);
    }

    #[test]
    fn both_native_headers_read_independent_synthetic_vectors() {
        let expected = json!({"token":"fixture-token", "userId":"fixture-user"});
        for value in [
            "dGMFEAAAAAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh+mHR6KVq9kO86Yg2BmPFHhIE8YcHSn/Vx8TMpiF9AeQIsiBmlyhgcCMN4nc3DwGgfs+D2fEP+I0M7QTaC3xcbmg56ErhH2+y3mYTcUuyOJymZC5NCkLdYtzZVa4cQQDF4yTGgLByRiX6sDHOcONkLspRYBvm9G6XkyqDNB+RXY+Q==",
            "EjkgIAIDAAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh9nvvStEisfKEJ7fVhP43eQAHKnAN6P/gUCOZMyoH4sq+OgeBeCanxnEpg9RTWgwoGK6kCYpmSr3Ee7uoHMKsldcAlfqpvwpyWUubfHv7kbqU+X1AO+xo4ELe/Gh7giPI/cxJMHMhGUFc6kCoF5edviOxr+M9r4OziSrEFDtfJDpA==",
        ] {
            assert_eq!(decode(&Value::String(value.into())).unwrap(), expected);
        }
    }

    #[test]
    fn switching_preserves_settings_sessions_and_device_identity() {
        let credentials = Credentials {
            auth: json!({"token":"next-token", "userId":"next-user", "futureField":"target-field", "account":{"userTag":"CN","futureAccountField":"target-value"}}),
            identity: Identity {
                device_id: "stable-device".into(),
                machine_id: "machine".into(),
                app_version: "3".into(),
                client_id: "client".into(),
                key_pair: None,
            },
        };
        let mut root = json!({
            "telemetry.machineId":"machine",
            "unknown":{"keep":true},
            "sessionState":[1,2],
            "iCubeAuthInfo://icube-dc:stable-device":"device-fixture",
            "iCubeServerData://icube.cloudide":{"modelConfig":"keep"},
            "iCubeAuthInfo://icube.cloudide":{"token":"prior-token","userId":"prior-user","futureUserSecret":"must-not-copy","futureField":"keep","account":{"email":"prior@example.test","userTag":"old-tag","futureAccountField":"keep"}},
            "iCubeAuthInfo://usertag":{"prior-user":"CN"}
        });
        apply_to_document(&mut root, &credentials).unwrap();
        assert_eq!(root["unknown"], json!({"keep":true}));
        assert_eq!(root["sessionState"], json!([1, 2]));
        assert_eq!(root["telemetry.machineId"], "machine");
        assert_eq!(
            root["iCubeAuthInfo://icube-dc:stable-device"],
            "device-fixture"
        );
        assert_eq!(
            root["iCubeServerData://icube.cloudide"]["modelConfig"],
            "keep"
        );
        assert_eq!(decode(&root[AUTH]).unwrap()["token"], "next-token");
        assert_eq!(decode(&root[AUTH]).unwrap()["futureField"], "target-field");
        assert!(decode(&root[AUTH]).unwrap()["futureUserSecret"].is_null());
        assert!(decode(&root[AUTH]).unwrap()["account"]["email"].is_null());
        assert_eq!(
            decode(&root[AUTH]).unwrap()["account"]["futureAccountField"],
            "target-value"
        );
        assert_eq!(
            decode(&root[USER_TAG]).unwrap(),
            json!({"prior-user":"CN", "next-user":"cn"})
        );
        let mut mismatched = credentials;
        mismatched.identity.device_id = "another-device".into();
        let before = root.clone();
        assert!(apply_to_document(&mut root, &mismatched).is_err());
        assert_eq!(root, before);
    }

    #[test]
    fn paths_and_region_are_separate_on_all_platforms() {
        for platform in ["windows", "macos", "linux"] {
            assert_ne!(
                data_candidates(platform, Path::new("root"), Edition::Trae),
                data_candidates(platform, Path::new("root"), Edition::Traecn)
            );
        }
        assert_eq!(
            region(&json!({"userRegion":{"region":"CN"},"host":"https://growsg-normal.trae.ai"})),
            Some(Edition::Traecn)
        );
        assert_eq!(
            region(&json!({"host":"https://growsg-normal.trae.ai"})),
            Some(Edition::Trae)
        );
        assert_eq!(timestamp(&json!(1791774000000u64)), Some(1791774000));
        assert_eq!(timestamp(&json!(1791774000)), Some(1791774000));
    }

    #[test]
    fn switching_never_rewrites_registered_device_key_or_unknown_device_fields() {
        let key =
            json!({"privateKeyPEM":"private", "publicKeyPEM":"public", "futureDeviceField":"keep"});
        let device = encode(&key).unwrap();
        let mut root = json!({"iCubeAuthInfo://icube-dc:stable-device":device});
        let mut credentials = Credentials {
            auth: json!({"token":"next-token","userId":"next-user"}),
            identity: Identity {
                device_id: "stable-device".into(),
                machine_id: "machine".into(),
                app_version: "3".into(),
                client_id: "client".into(),
                key_pair: Some(serde_json::from_value(key).unwrap()),
            },
        };
        apply_to_document(&mut root, &credentials).unwrap();
        assert_eq!(root["iCubeAuthInfo://icube-dc:stable-device"], device);
        credentials.identity.key_pair.as_mut().unwrap().public_key = "another-key".into();
        let before = root.clone();
        assert!(apply_to_document(&mut root, &credentials).is_err());
        assert_eq!(root, before);
    }

    #[test]
    fn saved_identity_cannot_query_rewards_under_another_native_device() {
        let current = Identity {
            device_id: "new-device".into(),
            machine_id: "machine".into(),
            app_version: "3".into(),
            client_id: "client".into(),
            key_pair: None,
        };
        let mut saved = current.clone();
        saved.device_id = "old-device".into();
        assert!(validate_identity(&current, &saved).is_err());
        saved.device_id = current.device_id.clone();
        assert!(validate_identity(&current, &saved).is_ok());
    }

    #[test]
    fn product_paths_follow_native_bundle_and_launcher_layouts() {
        assert_eq!(
            product_path(Path::new("/opt/Trae CN/Trae CN.exe"), "windows").unwrap(),
            Path::new("/opt/Trae CN/resources/app/product.json")
        );
        assert_eq!(
            product_path(
                Path::new("/Applications/Trae.app/Contents/MacOS/Trae"),
                "macos"
            )
            .unwrap(),
            Path::new("/Applications/Trae.app/Contents/Resources/app/product.json")
        );
        assert_eq!(
            product_path(Path::new("/usr/share/trae/bin/trae"), "linux").unwrap(),
            Path::new("/usr/share/trae/resources/app/product.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn linux_symlink_reads_product_from_real_installation() {
        let dir = std::env::temp_dir().join(format!("echobird-trae-link-{}", uuid::Uuid::new_v4()));
        let target = dir.join("usr/share/trae/bin/trae");
        let launcher = dir.join("usr/bin/trae");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::fs::write(&target, b"fixture launcher").unwrap();
        std::os::unix::fs::symlink(&target, &launcher).unwrap();
        assert_eq!(
            product_path(&launcher, "linux").unwrap(),
            std::fs::canonicalize(&dir)
                .unwrap()
                .join("usr/share/trae/resources/app/product.json")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
