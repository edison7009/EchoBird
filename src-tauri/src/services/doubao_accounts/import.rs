//! Read native Chromium sessions without launching or changing the client.
use super::*;
use crate::services::electron_storage;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

fn root(edition: Edition) -> Result<PathBuf, String> {
    let product = match edition {
        Edition::DoubaoWork => "DoubaoWork",
        Edition::Dola => "Cici",
    };
    #[cfg(windows)]
    let root = dirs::data_local_dir().map(|p| p.join(product).join("User Data"));
    #[cfg(target_os = "macos")]
    let root = dirs::data_dir().map(|p| p.join(product));
    #[cfg(not(any(windows, target_os = "macos")))]
    let root = dirs::config_dir().map(|p| p.join(product));
    root.ok_or("accountError.home".into())
}

fn cookies(root: &Path, profile: &Path, edition: Edition) -> Result<Vec<Cookie>, String> {
    let Some(path) = ["Network/Cookies", "Cookies"]
        .iter()
        .map(|p| profile.join(p))
        .find(|p| p.is_file())
    else {
        return Ok(vec![]);
    };
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "accountError.read")?;
    db.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "accountError.read")?;
    let version = db
        .query_row("SELECT value FROM meta WHERE key='version'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut query = db.prepare("SELECT host_key,name,path,value,encrypted_value,is_secure,is_httponly,expires_utc FROM cookies").map_err(|_| "accountError.read")?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Vec<u8>>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, bool>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })
        .map_err(|_| "accountError.read")?;
    let mut cipher = None;
    let now = chrono::Utc::now().timestamp();
    let mut result = vec![];
    for row in rows {
        let (domain, name, path, mut value, encrypted, secure, http_only, expires) =
            row.map_err(|_| "accountError.read")?;
        let host = domain.trim_start_matches('.');
        if !(host == edition.domain() || host.ends_with(&format!(".{}", edition.domain()))) {
            continue;
        }
        let expires = (expires > 0).then(|| expires / 1_000_000 - 11_644_473_600);
        if expires.is_some_and(|e| e <= now) {
            continue;
        }
        if value.is_empty() && !encrypted.is_empty() {
            if cipher.is_none() {
                cipher = Some(electron_storage::cipher(root)?);
            }
            let plain = electron_storage::decrypt_bytes(
                cipher.as_ref().ok_or("accountError.keychain")?,
                &encrypted,
            )?;
            let bytes = if version >= 24 {
                plain
                    .strip_prefix(Sha256::digest(domain.as_bytes()).as_slice())
                    .ok_or("accountError.format")?
            } else {
                &plain
            };
            value = String::from_utf8(bytes.to_vec()).map_err(|_| "accountError.format")?;
        }
        let cookie = Cookie {
            name,
            value,
            domain,
            path,
            secure,
            http_only,
            expires,
        };
        if cookie.valid(edition, now) {
            result.push(cookie);
        }
    }
    Ok(result)
}

pub(crate) async fn scan(
    edition: Edition,
) -> Result<crate::services::auto_import::native::Counts, String> {
    let root = root(edition)?;
    let mut profiles = vec![root.clone(), root.join("Default")];
    if root.is_dir() {
        for entry in std::fs::read_dir(&root).map_err(|_| "accountError.read")? {
            let entry = entry.map_err(|_| "accountError.read")?;
            if entry.file_name().to_string_lossy().starts_with("Profile ") && entry.path().is_dir()
            {
                profiles.push(entry.path());
            }
        }
    }
    let mut counts = crate::services::auto_import::native::Counts::default();
    for profile in profiles {
        let result = async {
            let cookies = cookies(&root, &profile, edition)?;
            if cookies.is_empty() {
                return Ok(None);
            }
            let mut c = Credentials {
                edition,
                user_id: "pending".into(),
                enterprise_type: String::new(),
                cookies,
            };
            if c.cookie_header().is_err() {
                return Ok(None);
            }
            let (account, user, enterprise) = api::identity(&c).await?;
            c.user_id = user;
            c.enterprise_type = enterprise;
            c.validate()?;
            let _guard = STORE_LOCK.lock().await;
            let path = path(edition)?;
            let mut rows = load(&path)?;
            if rows.values().any(|s| {
                crate::services::auto_import::native::same_account(
                    &account.id,
                    account.email.as_deref().unwrap_or_default(),
                    &s.account.id,
                    s.account.email.as_deref().unwrap_or_default(),
                )
            }) {
                return Ok(Some(false));
            }
            rows.insert(
                account.id.clone(),
                Saved {
                    account,
                    credential: protect(&c)?,
                },
            );
            crate::services::cursor_auth::write(&path, &rows)?;
            Ok(Some(true))
        }
        .await;
        counts.record(result);
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_cookie_scan_filters_region_expiry_and_preserves_source() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("Default");
        std::fs::create_dir(&profile).unwrap();
        let path = profile.join("Cookies");
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE meta(key TEXT,value TEXT); INSERT INTO meta VALUES('version','24'); CREATE TABLE cookies(host_key TEXT,name TEXT,path TEXT,value TEXT,encrypted_value BLOB,is_secure INTEGER,is_httponly INTEGER,expires_utc INTEGER);").unwrap();
        for (host, name, expiry) in [
            (".doubao.com", "sessionid", 0),
            (".dola.com", "sessionid", 0),
            (".doubao.com", "expired", 1),
            (".doubao.com.evil.test", "sessionid", 0),
        ] {
            db.execute(
                "INSERT INTO cookies VALUES(?1,?2,'/','fixture-cookie',X'',1,1,?3)",
                rusqlite::params![host, name, expiry],
            )
            .unwrap();
        }
        drop(db);
        let before = std::fs::read(&path).unwrap();
        let cn = cookies(root.path(), &profile, Edition::DoubaoWork).unwrap();
        assert_eq!(cn.len(), 1);
        assert_eq!(cn[0].name, "sessionid");
        assert_eq!(cn[0].domain, ".doubao.com");
        let global = cookies(root.path(), &profile, Edition::Dola).unwrap();
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].domain, ".dola.com");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(cookies(
            root.path(),
            &root.path().join("missing"),
            Edition::DoubaoWork
        )
        .unwrap()
        .is_empty());
    }
}
