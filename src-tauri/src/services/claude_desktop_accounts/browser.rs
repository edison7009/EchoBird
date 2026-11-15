use super::*;
use crate::services::{account_login_browser, electron_storage::encrypt_bytes};

// Normalized WebView cookies are sealed by EchoBird, not tied to an Electron profile.
pub(super) const COOKIE_VERSION: i64 = -1;

pub(super) fn value(cookie: &Cookie) -> Result<String, String> {
    account_login_browser::unseal(cookie.text("browser_value").ok_or("accountError.format")?)
}

/// Builds a browser-format snapshot, as earlier versions saved them. Only the
/// tests need it: saved browser snapshots are still applied.
#[cfg(test)]
fn capture_with(
    cookies: &[tauri::webview::Cookie<'_>],
    now: i64,
    seal: impl Fn(&str) -> Result<String, String>,
) -> Result<Option<Snapshot>, String> {
    let accepted = cookies
        .iter()
        .filter(|cookie| {
            cookie
                .domain()
                .is_some_and(|host| is_auth_cookie(host, cookie.name()))
                && cookie.path() == Some("/")
                && !cookie.value().is_empty()
                && !cookie.value().contains(['\r', '\n', ';'])
                && cookie
                    .expires_datetime()
                    .is_none_or(|expiry| expiry.unix_timestamp() > now)
        })
        .collect::<Vec<_>>();
    if !accepted
        .iter()
        .any(|cookie| cookie.name() == "sessionKey" && cookie.secure() == Some(true))
        || !accepted.iter().any(|cookie| {
            cookie.name() == "lastActiveOrg" && uuid::Uuid::parse_str(cookie.value()).is_ok()
        })
    {
        return Ok(None);
    }
    let mut normalized = BTreeMap::new();
    for cookie in accepted {
        let host = cookie.domain().ok_or("accountError.format")?;
        let sealed = seal(cookie.value())?;
        let timestamp = (now + 11_644_473_600) * 1_000_000;
        let expiry = cookie
            .expires_datetime()
            .map(|date| (date.unix_timestamp() + 11_644_473_600) * 1_000_000);
        let row = Cookie(BTreeMap::from([
            ("host_key".into(), Cell::Text(host.into())),
            ("name".into(), Cell::Text(cookie.name().into())),
            ("path".into(), Cell::Text("/".into())),
            ("value".into(), Cell::Text(String::new())),
            ("encrypted_value".into(), Cell::Blob(Vec::new())),
            ("browser_value".into(), Cell::Text(sealed)),
            ("creation_utc".into(), Cell::Integer(timestamp)),
            ("expires_utc".into(), Cell::Integer(expiry.unwrap_or(0))),
            ("last_access_utc".into(), Cell::Integer(timestamp)),
            ("last_update_utc".into(), Cell::Integer(timestamp)),
            (
                "is_secure".into(),
                Cell::Integer(i64::from(cookie.secure() == Some(true))),
            ),
            (
                "is_httponly".into(),
                Cell::Integer(i64::from(cookie.http_only() == Some(true))),
            ),
            (
                "has_expires".into(),
                Cell::Integer(i64::from(expiry.is_some())),
            ),
            (
                "is_persistent".into(),
                Cell::Integer(i64::from(expiry.is_some())),
            ),
            ("priority".into(), Cell::Integer(1)),
            (
                "samesite".into(),
                Cell::Integer(match cookie.same_site() {
                    Some(tauri::webview::cookie::SameSite::None) => 0,
                    Some(tauri::webview::cookie::SameSite::Lax) => 1,
                    Some(tauri::webview::cookie::SameSite::Strict) => 2,
                    None => -1,
                }),
            ),
            (
                "source_scheme".into(),
                Cell::Integer(if cookie.secure() == Some(true) { 2 } else { 1 }),
            ),
            (
                "source_port".into(),
                Cell::Integer(if cookie.secure() == Some(true) {
                    443
                } else {
                    80
                }),
            ),
            ("source_type".into(), Cell::Integer(0)),
            ("top_frame_site_key".into(), Cell::Text(String::new())),
            ("has_cross_site_ancestor".into(), Cell::Integer(0)),
        ]));
        normalized.insert((host.to_owned(), cookie.name().to_owned()), row);
    }
    Ok(Some(Snapshot {
        cookies: normalized.into_values().collect(),
        config: BTreeMap::new(),
        cookie_version: COOKIE_VERSION,
    }))
}

pub(super) fn native_snapshot(
    db: &Connection,
    snapshot: &Snapshot,
    key: &Cipher,
) -> Result<Snapshot, String> {
    native_snapshot_with(db, snapshot, key, value)
}

fn native_snapshot_with(
    db: &Connection,
    snapshot: &Snapshot,
    key: &Cipher,
    unseal: impl Fn(&Cookie) -> Result<String, String>,
) -> Result<Snapshot, String> {
    let mut result = snapshot.clone();
    if result.cookie_version != COOKIE_VERSION {
        return Ok(result);
    }
    let version = db
        .query_row("SELECT value FROM meta WHERE key='version'", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| "accountError.format")?
        .parse::<i64>()
        .map_err(|_| "accountError.format")?;
    for cookie in &mut result.cookies {
        let host = cookie.text("host_key").ok_or("accountError.format")?;
        if !is_auth_cookie(host, cookie.text("name").ok_or("accountError.format")?) {
            return Err("accountError.format".into());
        }
        let mut bytes = if version >= 24 {
            Sha256::digest(host.as_bytes()).to_vec()
        } else {
            Vec::new()
        };
        bytes.extend_from_slice(unseal(cookie)?.as_bytes());
        cookie.0.insert(
            "encrypted_value".into(),
            Cell::Blob(encrypt_bytes(key, &bytes)?),
        );
        cookie.0.remove("browser_value");
    }
    result.cookie_version = version;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::electron_storage::{decrypt, encrypt};
    use tauri::webview::{
        cookie::{time::OffsetDateTime, SameSite},
        Cookie as WebCookie,
    };

    fn browser_cookies() -> Vec<WebCookie<'static>> {
        vec![
            WebCookie::build(("sessionKey", "fixture-session"))
                .domain(".claude.ai")
                .path("/")
                .secure(true)
                .http_only(true)
                .same_site(SameSite::Lax)
                .build(),
            WebCookie::build(("lastActiveOrg", "12345678-1234-1234-1234-123456789abc"))
                .domain(".claude.ai")
                .path("/")
                .secure(true)
                .build(),
        ]
    }

    #[test]
    fn real_webview_cookie_metadata_requires_both_valid_auth_cookies() {
        let cookies = browser_cookies();
        let seal = |_: &str| -> Result<String, String> {
            panic!("incomplete login must not seal or save")
        };
        assert!(capture_with(&cookies[..1], 100, seal).unwrap().is_none());
        let mut expired = cookies.clone();
        expired[0].set_expires(OffsetDateTime::from_unix_timestamp(99).unwrap());
        assert!(capture_with(&expired, 100, seal).unwrap().is_none());
        let mut wrong = cookies.clone();
        wrong[0].set_domain("claude.ai.evil.test");
        assert!(capture_with(&wrong, 100, seal).unwrap().is_none());
        let mut insecure = cookies.clone();
        insecure[0].set_secure(false);
        assert!(capture_with(&insecure, 100, seal).unwrap().is_none());
        let captured = capture_with(&cookies, 100, |_| Ok("sealed-fixture".into()))
            .unwrap()
            .unwrap();
        let session = primary_cookie(&captured, "sessionKey").unwrap();
        assert_eq!(session.text("host_key"), Some("claude.ai"));
        assert_eq!(session.integer("is_httponly"), Some(1));
        assert_eq!(session.integer("samesite"), Some(1));
        assert_eq!(session.integer("has_expires"), Some(0));
        assert_eq!(session.text("value"), Some(""));
        assert!(!serde_json::to_string(&captured)
            .unwrap()
            .contains("fixture-session"));
    }

    #[test]
    fn browser_sessions_convert_to_current_and_legacy_electron_cookies_on_explicit_apply() {
        use aes_gcm::KeyInit;
        let key = Cipher::Gcm(Box::new(
            aes_gcm::Aes256Gcm::new_from_slice(&[7; 32]).unwrap(),
        ));
        let cookies = browser_cookies();
        let captured = capture_with(&cookies, 100, |plain| encrypt(&key, plain))
            .unwrap()
            .unwrap();
        for version in [23, 24] {
            let mut db = Connection::open_in_memory().unwrap();
            db.execute_batch(&format!("CREATE TABLE meta(key TEXT,value TEXT);INSERT INTO meta VALUES('version','{version}');
                CREATE TABLE cookies(host_key TEXT NOT NULL,name TEXT NOT NULL,path TEXT NOT NULL,value TEXT NOT NULL,encrypted_value BLOB NOT NULL,has_expires INTEGER NOT NULL,expires_utc INTEGER NOT NULL,last_update_utc INTEGER NOT NULL,future_flag INTEGER NOT NULL DEFAULT 0);
                INSERT INTO cookies VALUES('claude.ai','theme','/','dark',x'',0,0,1,42);")).unwrap();
            let converted = native_snapshot_with(&db, &captured, &key, |cookie| {
                decrypt(&key, cookie.text("browser_value").unwrap())
            })
            .unwrap();
            assert_eq!(converted.cookie_version, version);
            assert_eq!(
                cookie_value(
                    version,
                    primary_cookie(&converted, "sessionKey").unwrap(),
                    &key
                )
                .unwrap(),
                "fixture-session"
            );
            assert!(converted
                .cookies
                .iter()
                .all(|cookie| cookie.text("browser_value").is_none()));
            let dir = std::env::temp_dir()
                .join(format!("echobird-browser-auth-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&dir).unwrap();
            let config = dir.join("config.json");
            write(
                &config,
                &json!({"locale":"ja","oauth:tokenCache":"old-account"}),
            )
            .unwrap();
            restore(&mut db, &config, &converted).unwrap();
            let native = Snapshot {
                cookies: super::super::cookies(&db).unwrap(),
                config: BTreeMap::new(),
                cookie_version: version,
            };
            assert_eq!(
                cookie_value(
                    version,
                    primary_cookie(&native, "sessionKey").unwrap(),
                    &key
                )
                .unwrap(),
                "fixture-session"
            );
            assert_eq!(
                db.query_row(
                    "SELECT future_flag FROM cookies WHERE name='theme'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                42
            );
            assert_eq!(read::<Value>(&config).unwrap(), json!({"locale":"ja"}));
            std::fs::remove_file(config).unwrap();
            std::fs::remove_dir(dir).unwrap();
        }
    }
}
