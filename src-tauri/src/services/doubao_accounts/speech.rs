//! Domestic speech projection. Never forward the flattened www cookie header.
use super::{credential, load, path, Cookie, Credentials, Edition, STORE_LOCK};
use serde_json::{json, Value};

pub(crate) const SCOPE: &str = "wss://frontier-audio-web-ws.doubao.com/api/v2/sami/voicegenie";
const HOST: &str = "frontier-audio-web-ws.doubao.com";
const PATH: &str = "/api/v2/sami/voicegenie";

pub(crate) async fn speech_credentials(
    id: &str,
    rejected: Option<&Value>,
) -> Result<Value, String> {
    let _guard = STORE_LOCK.lock().await;
    let c = credential(&load(&path(Edition::DoubaoWork)?)?, Edition::DoubaoWork, id)?;
    let current = project(&c, chrono::Utc::now().timestamp())?;
    check_rejection(&current, rejected)?;
    Ok(current)
}

fn check_rejection(current: &Value, rejected: Option<&Value>) -> Result<(), String> {
    if let Some(rejected) = rejected {
        if rejected["edition"] != "doubaowork" || rejected["cookieScope"] != SCOPE {
            return Err("accountError.invalidAccount".into());
        }
        if current["cookieHeader"] == rejected["cookieHeader"] {
            return Err("accountError.loginRequired".into());
        }
    }
    Ok(())
}

fn project(c: &Credentials, now: i64) -> Result<Value, String> {
    if c.edition != Edition::DoubaoWork || c.user_id.trim().is_empty() {
        return Err("accountError.invalidAccount".into());
    }
    // The stored object has already passed credential() validation. Revalidate
    // syntax/edition here without using a www projection as a speech credential.
    if c.cookies
        .iter()
        .any(|cookie| !cookie.valid(c.edition, i64::MIN))
    {
        return Err("accountError.invalidAccount".into());
    }
    let mut cookies: Vec<_> = c
        .cookies
        .iter()
        .filter(|cookie| matches_target(cookie, now, true))
        .collect();
    cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
    let header = cookies
        .into_iter()
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ");
    Ok(
        json!({"edition":"doubaowork","accountId":c.id(),"userId":c.user_id,
        "enterpriseType":c.enterprise_type,"cookieScope":SCOPE,"cookieHeader":header}),
    )
}

fn matches_target(cookie: &Cookie, now: i64, secure_transport: bool) -> bool {
    let domain = cookie.domain.trim_start_matches('.');
    let domain_matches = if cookie.domain.starts_with('.') {
        HOST == domain || HOST.ends_with(&format!(".{domain}"))
    } else {
        HOST == domain
    };
    let path_matches = PATH == cookie.path
        || (PATH.starts_with(&cookie.path)
            && (cookie.path.ends_with('/')
                || PATH.as_bytes().get(cookie.path.len()) == Some(&b'/')));
    cookie.valid(Edition::DoubaoWork, now)
        && (!cookie.secure || secure_transport)
        && domain_matches
        && path_matches
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cookie(name: &str, domain: &str, path: &str) -> Cookie {
        Cookie {
            name: name.into(),
            value: format!("{name}-fixture"),
            domain: domain.into(),
            path: path.into(),
            secure: true,
            http_only: true,
            expires: None,
        }
    }
    fn credentials() -> Credentials {
        Credentials {
            edition: Edition::DoubaoWork,
            user_id: "real-saved-user".into(),
            enterprise_type: "102".into(),
            cookies: vec![cookie("sessionid", ".doubao.com", "/")],
        }
    }
    #[test]
    fn scope_rules_cover_host_only_path_secure_expiry_and_same_name_order() {
        let mut c = credentials();
        c.cookies.extend([
            cookie("host-only", HOST, "/api/v2"),
            cookie("www-only", "www.doubao.com", "/"),
            cookie("root-host", "doubao.com", "/"),
            cookie("wrong-path", ".doubao.com", "/api/v"),
            cookie("other-sibling", "accounts.doubao.com", "/"),
            cookie(
                "longer-path",
                ".doubao.com",
                "/api/v2/sami/voicegenie-extra",
            ),
            Cookie {
                expires: Some(100),
                ..cookie("expired", ".doubao.com", "/")
            },
            Cookie {
                secure: false,
                ..cookie("nonsecure-on-wss", ".doubao.com", "/")
            },
            Cookie {
                value: "path-specific".into(),
                ..cookie("sessionid", ".doubao.com", "/api/v2/sami")
            },
        ]);
        let result = project(&c, 100).unwrap();
        assert_eq!(result["cookieHeader"],"sessionid=path-specific; host-only=host-only-fixture; sessionid=sessionid-fixture; nonsecure-on-wss=nonsecure-on-wss-fixture");
        assert_eq!(result["userId"], "real-saved-user");
        assert_ne!(result["userId"], result["accountId"]);
        assert_eq!(result["enterpriseType"], "102");
        assert!(!matches_target(&c.cookies[0], 100, false));
        assert!(matches_target(
            c.cookies
                .iter()
                .find(|c| c.name == "nonsecure-on-wss")
                .unwrap(),
            100,
            false
        ));
    }
    #[test]
    fn dola_and_cookie_injection_are_rejected_without_a_cross_region_projection() {
        let mut c = credentials();
        c.edition = Edition::Dola;
        c.cookies = vec![cookie("sessionid", ".dola.com", "/")];
        assert!(project(&c, 100).is_err());
        let mut c = credentials();
        c.cookies.push(cookie("foreign", ".dola.com", "/"));
        assert!(project(&c, 100).is_err());
        let mut c = credentials();
        c.cookies[0].value = "secret\r\nInjected: true".into();
        assert!(project(&c, 100).is_err());
    }
    #[test]
    fn rejection_compares_only_the_same_speech_scope_and_never_www_flattening() {
        let c = credentials();
        let previous = project(&c, 100).unwrap();
        assert_eq!(
            check_rejection(&previous, Some(&previous)).unwrap_err(),
            "accountError.loginRequired"
        );
        let mut next = previous.clone();
        next["cookieHeader"] = json!("sessionid=rotated");
        assert!(check_rejection(&next, Some(&previous)).is_ok());
        let mut legacy = previous.clone();
        legacy.as_object_mut().unwrap().remove("cookieScope");
        legacy["cookieHeader"] = json!("sessionid=www-flat");
        assert_eq!(
            check_rejection(&next, Some(&legacy)).unwrap_err(),
            "accountError.invalidAccount"
        );
        let mut c = credentials();
        c.cookies[0].domain = "www.doubao.com".into();
        assert_eq!(project(&c, 100).unwrap()["cookieHeader"], "");
    }
}
