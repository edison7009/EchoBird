use super::{api, Account, Credentials};
use serde::Serialize;
use serde_json::json;
use std::sync::{Mutex, OnceLock};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub expires_at: i64,
}
#[derive(Clone)]
struct Pending {
    id: String,
    expires: i64,
    candidate: Option<Credentials>,
}
fn pending() -> &'static Mutex<Option<Pending>> {
    static PENDING: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}
fn label(id: &str) -> String {
    format!("dumate-login-{id}")
}
fn browser_cookie(cookie: &tauri::webview::Cookie<'_>) -> serde_json::Value {
    // Cookie::domain() removes the leading dot, so it cannot establish hostOnly.
    // Restore only the BCE parent domains used by the native client; keep other
    // hosts narrow instead of broadening every browser cookie's scope.
    let domain = cookie.domain().map(|domain| match domain {
        "baidu.com" | "bce.baidu.com" => format!(".{domain}"),
        _ => domain.to_owned(),
    });
    let mut value = json!({
        "name": cookie.name(), "value": cookie.value(), "domain": domain,
        "path": cookie.path().unwrap_or("/"),
        "secure": cookie.secure().unwrap_or(false),
        "httpOnly": cookie.http_only().unwrap_or(false),
    });
    if let Some(expiry) = cookie.expires_datetime() {
        value["expirationDate"] = json!(expiry.unix_timestamp());
    }
    if let Some(same_site) = cookie.same_site() {
        let same_site = same_site.to_string().to_ascii_lowercase();
        value["sameSite"] = json!(if same_site == "none" {
            "no_restriction"
        } else {
            &same_site
        });
    }
    value
}
fn browser_session_ready(cookies: &[serde_json::Value]) -> bool {
    let now = chrono::Utc::now().timestamp() as f64;
    ["bce-long-term-sessionid", "bce-user-info"]
        .iter()
        .all(|name| {
            cookies.iter().any(|cookie| {
                cookie["name"] == *name
                    && api::text(cookie, "value").is_some()
                    && matches!(
                        api::text(cookie, "domain"),
                        Some(".bce.baidu.com" | "console.bce.baidu.com")
                    )
                    && cookie["expirationDate"]
                        .as_f64()
                        .is_none_or(|expiry| expiry > now)
            })
        })
}
fn close(app: &tauri::AppHandle, id: &str) {
    if let Some(window) = app.get_webview_window(&label(id)) {
        let _ = window.close();
    }
}
fn discard(id: &str) -> Result<(), String> {
    let mut state = pending().lock().map_err(|_| "accountError.read")?;
    if state.as_ref().is_some_and(|p| p.id == id) {
        state.take();
    }
    Ok(())
}
fn snapshot(id: &str) -> Result<Pending, String> {
    pending()
        .lock()
        .map_err(|_| "accountError.read")?
        .as_ref()
        .filter(|p| p.id == id && p.expires > chrono::Utc::now().timestamp())
        .cloned()
        .ok_or_else(|| "accountError.expired".into())
}
fn open(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    snapshot(id)?;
    let url="https://login.bce.baidu.com/?from=oa_dumate&redirect=https%3A%2F%2Fconsole.bce.baidu.com%2F"
        .parse().map_err(|_| "accountError.format")?;
    let window = WebviewWindowBuilder::new(app, label(id), WebviewUrl::External(url))
        .title("DuMate")
        .inner_size(950.0, 730.0)
        .incognito(true)
        .on_navigation(|url| {
            url.scheme() == "https"
                && url.host_str().is_some_and(|host| {
                    ["baidu.com", "dumate.cn"]
                        .iter()
                        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
                })
        })
        .build()
        .map_err(|_| "accountError.browser")?;
    let login_id = id.to_owned();
    window.on_window_event(move |event| {
        if matches!(
            event,
            tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
        ) {
            let _ = discard(&login_id);
        }
    });
    if snapshot(id).is_err() {
        let _ = window.close();
        return Err("accountError.expired".into());
    }
    Ok(())
}
pub(super) async fn start(app: tauri::AppHandle) -> Result<LoginStart, String> {
    api::installed()?;
    let candidate = super::native::current().ok().flatten();
    let candidate = {
        let _guard = super::STORE_LOCK.lock().await;
        let saved = super::load()?;
        candidate.filter(|c| !saved.contains_key(&c.id()))
    };
    let id = uuid::Uuid::new_v4().to_string();
    let expires =
        chrono::Utc::now().timestamp() + super::super::account_login_browser::LOGIN_TIMEOUT_SECONDS;
    let old = {
        let mut pending = pending().lock().map_err(|_| "accountError.read")?;
        pending.replace(Pending {
            id: id.clone(),
            expires,
            candidate: candidate.clone(),
        })
    };
    if let Some(old) = old {
        close(&app, &old.id);
    }
    if candidate.is_none() {
        if let Err(error) = open(&app, &id) {
            cancel(&app, &id)?;
            return Err(error);
        }
    }
    Ok(LoginStart {
        login_id: id,
        expires_at: expires,
    })
}
pub(super) fn cancel(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    discard(id)?;
    close(app, id);
    Ok(())
}
pub(super) async fn poll(
    app: &tauri::AppHandle,
    id: &str,
) -> Result<Option<(Credentials, Account)>, String> {
    let state = snapshot(id)?;
    let native = state.candidate.is_some();
    let mut c = if let Some(c) = state.candidate {
        c
    } else {
        let window = app
            .get_webview_window(&label(id))
            .ok_or("accountError.expired")?;
        let cookies = window
            .cookies()
            .map_err(|_| "accountError.read")?
            .iter()
            .map(browser_cookie)
            .collect::<Vec<_>>();
        if !browser_session_ready(&cookies) {
            return Ok(None);
        }
        Credentials {
            profile: json!({"loginType":"bce","bceUserId":"pending","bceAccountId":"pending"}),
            cookies,
            gateway_token: None,
        }
    };
    let client = api::Client::installed(&c)?;
    let user = match client.user(&c).await {
        Ok(user) => user,
        Err(error) if error == "accountError.loginRequired" => {
            snapshot(id)?;
            if native {
                {
                    let mut state = pending().lock().map_err(|_| "accountError.read")?;
                    let state = state
                        .as_mut()
                        .filter(|p| p.id == id)
                        .ok_or("accountError.expired")?;
                    state.candidate = None;
                }
                open(app, id)?;
            }
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let user_id = api::text(&user, "bceUserId").ok_or("accountError.format")?;
    if native && api::text(&c.profile, "bceUserId") != Some(user_id) {
        return Err("accountError.invalidAccount".into());
    }
    c.profile["bceUserId"] = json!(user_id);
    if !native {
        c.profile["lastSuccessfulLoginAt"] = json!(chrono::Utc::now().timestamp_millis());
    }
    if !c.enterprise() {
        c.profile["bceAccountId"] =
            json!(api::text(&user, "bceAccountId").ok_or("accountError.format")?);
    }
    if let Some(name) = api::text(&user, "displayName") {
        c.profile["displayName"] = json!(name);
    }
    c.validate()?;
    let mut account = c.summary();
    if let Ok(quota) = client.quota(&c).await {
        quota.apply(&mut account);
    }
    if !c.enterprise() {
        account.daily_reward = client.daily(&c).await.ok();
    }
    snapshot(id)?;
    Ok(Some((c, account)))
}
pub(super) fn complete<T>(
    app: &tauri::AppHandle,
    id: &str,
    write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut state = pending().lock().map_err(|_| "accountError.read")?;
    if !state
        .as_ref()
        .is_some_and(|p| p.id == id && p.expires > chrono::Utc::now().timestamp())
    {
        return Err("accountError.expired".into());
    }
    let result = write()?;
    state.take();
    drop(state);
    close(app, id);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Explicit read-only live check through the browser Cookie conversion; no login writes or claims"]
    async fn installed_dumate_browser_cookie_probe() {
        assert_eq!(
            std::env::var("ECHOBIRD_DUMATE_NATIVE_PROBE").as_deref(),
            Ok("1")
        );
        let mut c = super::super::native::current()
            .unwrap()
            .expect("signed-in DuMate");
        let now = chrono::Utc::now().timestamp() as f64;
        c.cookies = c
            .cookies
            .iter()
            .filter_map(|cookie| {
                if cookie["expirationDate"]
                    .as_f64()
                    .is_some_and(|expiry| expiry <= now)
                {
                    return None;
                }
                let parsed = tauri::webview::Cookie::build((
                    api::text(cookie, "name")?.to_owned(),
                    cookie["value"].as_str()?.to_owned(),
                ))
                .domain(api::text(cookie, "domain")?.to_owned())
                .path(cookie["path"].as_str().unwrap_or("/").to_owned())
                .secure(cookie["secure"].as_bool().unwrap_or(false))
                .http_only(cookie["httpOnly"].as_bool().unwrap_or(false))
                .build();
                Some(browser_cookie(&parsed))
            })
            .collect();
        assert!(browser_session_ready(&c.cookies));
        api::Client::installed(&c)
            .unwrap()
            .verify(&c)
            .await
            .unwrap();
        println!("Browser Cookie conversion: live DuMate identity verified; no saves or claims");
    }
    #[tokio::test]
    async fn browser_login_keeps_bce_domain_cookies_usable_by_the_real_account_client() {
        use axum::{routing::get, Json, Router};
        use tauri::webview::Cookie;

        let session = Cookie::build(("bce-long-term-sessionid", "fixture-session"))
            .domain(".bce.baidu.com")
            .path("/")
            .secure(true)
            .http_only(true)
            .build();
        let csrf = Cookie::build(("bce-user-info", "fixture-csrf"))
            .domain(".bce.baidu.com")
            .path("/")
            .build();
        assert_eq!(session.domain(), Some("bce.baidu.com"));
        let mut c = super::super::tests::fixture();
        c.cookies = vec![browser_cookie(&session), browser_cookie(&csrf)];
        assert_eq!(c.cookies[0]["domain"], ".bce.baidu.com");
        assert!(c.cookies[0].get("hostOnly").is_none());
        assert_eq!(c.cookies[0]["httpOnly"], true);
        assert!(c.cookies[0].get("expirationDate").is_none());
        assert!(!browser_session_ready(&c.cookies[..1]));
        assert!(browser_session_ready(&c.cookies));
        let mut incomplete = c.cookies.clone();
        incomplete[1]["expirationDate"] = json!(1);
        assert!(!browser_session_ready(&incomplete));
        incomplete[1]["expirationDate"] = serde_json::Value::Null;
        incomplete[1]["domain"] = json!(".bce.baidu.com.attacker.test");
        assert!(!browser_session_ready(&incomplete));

        // Exercise the same cookie filtering and request construction as login,
        // redirecting only transport to a local fixture (no external request).
        let app = Router::new().route("/api/dumate/user/info", get(|headers: axum::http::HeaderMap| async move {
            assert!(headers["cookie"].to_str().unwrap().contains("bce-long-term-sessionid=fixture-session"));
            assert_eq!(headers["csrftoken"], "fixture-csrf");
            Json(json!({"code":0,"result":{"bceUserId":"fixture-user","bceAccountId":"fixture-account"}}))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            api::Client::for_login_test(format!("http://console.bce.baidu.com:{port}"), port);
        for cookie in &mut c.cookies {
            cookie["secure"] = json!(false);
        }
        client.verify(&c).await.unwrap();
        server.abort();

        let host_cookie = Cookie::build(("host-only", "fixture"))
            .domain("login.bce.baidu.com")
            .build();
        assert_eq!(
            browser_cookie(&host_cookie)["domain"],
            "login.bce.baidu.com"
        );
        let same_site =
            Cookie::parse("session=fixture; Domain=.bce.baidu.com; SameSite=None; Secure").unwrap();
        assert_eq!(browser_cookie(&same_site)["sameSite"], "no_restriction");
    }
    #[test]
    fn old_and_expired_attempts_cannot_commit_or_reuse_a_new_attempt() {
        let mut state = pending().lock().unwrap();
        *state = Some(Pending {
            id: "new".into(),
            expires: chrono::Utc::now().timestamp() + 60,
            candidate: None,
        });
        drop(state);
        assert!(snapshot("old").is_err());
        assert!(snapshot("new").is_ok());
        discard("old").unwrap();
        assert!(snapshot("new").is_ok());
        discard("new").unwrap();
        assert!(snapshot("new").is_err());
        *pending().lock().unwrap() = Some(Pending {
            id: "new".into(),
            expires: 0,
            candidate: None,
        });
        assert!(snapshot("new").is_err());
        pending().lock().unwrap().take();
    }
}
