use super::{api, Account, Cookie, Credentials, Edition};
use crate::services::account_login_browser::Login;
use serde::Serialize;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub expires_at: i64,
}
#[derive(Clone)]
struct Pending {
    edition: Edition,
    login: Login,
}
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static START_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub(super) async fn start(app: tauri::AppHandle, edition: Edition) -> Result<LoginStart, String> {
    let _start = START_LOCK.lock().await;
    api::installed(edition)?;
    let domains: &'static [&'static str] = match edition {
        Edition::DoubaoWork => &["doubao.com", "feishu.cn", "larksuite.com"],
        Edition::Dola => &[
            "dola.com",
            "cici.com",
            "ciciai.com",
            "accounts.google.com",
            "appleid.apple.com",
        ],
    };
    let login = Login::start(
        &app,
        edition.title(),
        &format!("{}/", edition.base()),
        domains,
    )?;
    let result = LoginStart {
        login_id: login.id.clone(),
        expires_at: login.expires,
    };
    let old = PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .replace(Pending { edition, login });
    if let Some(old) = old {
        old.login.cancel(&app);
    }
    Ok(result)
}
fn snapshot(id: &str) -> Result<Pending, String> {
    let pending = PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .as_ref()
        .filter(|pending| pending.login.id == id)
        .cloned()
        .ok_or("accountError.expired")?;
    pending.login.check()?;
    Ok(pending)
}
fn save_if_current<T>(
    current_id: Option<&str>,
    id: &str,
    save: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if current_id != Some(id) {
        return Err("accountError.cancelled".into());
    }
    save()
}
pub(super) fn with_current<T>(
    id: &str,
    save: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    // Login::complete already holds its attempt cancellation lock. Only guard
    // the current attempt and disk callback here; never close windows under it.
    let pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    save_if_current(
        pending.as_ref().map(|pending| pending.login.id.as_str()),
        id,
        save,
    )
}
pub(super) fn cancel(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let old = {
        let mut state = PENDING.lock().map_err(|_| "accountError.busy")?;
        if state.as_ref().is_some_and(|pending| pending.login.id == id) {
            state.take()
        } else {
            None
        }
    };
    if let Some(old) = old {
        old.login.cancel(app);
    }
    Ok(())
}
pub(super) async fn poll(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<Option<(Credentials, Account, Login)>, String> {
    let pending = snapshot(id)?;
    let now = chrono::Utc::now().timestamp();
    let cookies = pending
        .login
        .cookies(app)?
        .into_iter()
        .filter_map(|cookie| {
            let domain = cookie.domain()?.to_owned();
            let domain = if domain == pending.edition.domain() {
                format!(".{domain}")
            } else {
                domain
            };
            let value = Cookie {
                name: cookie.name().into(),
                value: cookie.value().into(),
                domain,
                path: cookie.path().unwrap_or("/").into(),
                secure: cookie.secure().unwrap_or(false),
                http_only: cookie.http_only().unwrap_or(false),
                expires: cookie
                    .expires_datetime()
                    .map(|expiry| expiry.unix_timestamp()),
            };
            value.valid(pending.edition, now).then(|| {
                (
                    (value.domain.clone(), value.path.clone(), value.name.clone()),
                    value,
                )
            })
        })
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    let mut c = Credentials {
        edition: pending.edition,
        user_id: "pending".into(),
        enterprise_type: String::new(),
        cookies,
    };
    if c.cookie_header().is_err() {
        return Ok(None);
    }
    let (account, user, enterprise) = match api::identity(&c).await {
        Ok(identity) => identity,
        Err(error) if error == "accountError.loginRequired" => return Ok(None),
        Err(error) => return Err(error),
    };
    snapshot(id)?;
    c.user_id = user;
    c.enterprise_type = enterprise;
    c.validate()?;
    Ok(Some((c, account, pending.login)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_attempt_cannot_save_after_its_earlier_snapshot() {
        let mut current = "old-attempt";
        let earlier_snapshot = current;
        current = "new-attempt";
        let mut saved = false;
        assert_eq!(
            save_if_current(Some(current), earlier_snapshot, || {
                saved = true;
                Ok(())
            })
            .unwrap_err(),
            "accountError.cancelled"
        );
        assert!(!saved);
    }

    #[test]
    fn current_attempt_saves_but_cancelled_attempt_does_not() {
        let mut writes = 0;
        let result = save_if_current(Some("current"), "current", || {
            writes += 1;
            Ok("saved")
        })
        .unwrap();
        assert_eq!(result, "saved");
        assert_eq!(writes, 1);
        assert!(save_if_current(None, "current", || {
            writes += 1;
            Ok(())
        })
        .is_err());
        assert_eq!(writes, 1);
    }
}
