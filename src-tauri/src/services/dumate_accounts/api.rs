use super::{Account, Credentials, CreditBucket, DailyReward};
use reqwest::{Client as HttpClient, Method};
use serde_json::Value;
use std::time::Duration;

pub(super) fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v[key].as_str().map(str::trim).filter(|s| !s.is_empty())
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
}
fn timestamp(v: &Value) -> Option<i64> {
    v.as_i64().filter(|v| *v > 0 && *v < 253_402_214_400)
}
pub(super) fn enterprise_origin(profile: &Value) -> Result<String, String> {
    let url = url::Url::parse(
        text(profile, "enterpriseApiBaseUrl").ok_or("accountError.invalidAccount")?,
    )
    .map_err(|_| "accountError.invalidAccount")?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(url.origin().ascii_serialization())
}
pub(super) fn installed() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        return Err("accountError.unavailable".into());
    }
    let executable = super::super::tool_manager::get_tool_exe_path("dumate")
        .ok_or("accountError.unavailable")?;
    if !super::super::tool_manager::packaged_desktop_metadata(std::path::Path::new(&executable))
        .is_some_and(|m| m["name"] == "qianfan-desktop-app")
    {
        return Err("accountError.unavailable".into());
    }
    Ok(())
}

pub(super) struct Client {
    http: HttpClient,
    base: String,
}
impl Client {
    #[cfg(test)]
    pub(super) fn for_login_test(base: String, port: u16) -> Self {
        Self {
            http: HttpClient::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .resolve(
                    "console.bce.baidu.com",
                    std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                )
                .build()
                .unwrap(),
            base,
        }
    }
    pub(super) fn installed(c: &Credentials) -> Result<Self, String> {
        installed()?;
        Self::authenticated(c)
    }
    pub(super) fn authenticated(c: &Credentials) -> Result<Self, String> {
        c.validate()?;
        Self::new(if c.enterprise() {
            enterprise_origin(&c.profile)?
        } else {
            "https://console.bce.baidu.com".into()
        })
    }
    pub(super) fn new(base: String) -> Result<Self, String> {
        Ok(Self {
            http: HttpClient::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "accountError.network")?,
            base,
        })
    }
    async fn request(&self, c: &Credentials, method: Method, path: &str) -> Result<Value, String> {
        self.request_as(c, method, path, false).await
    }
    async fn request_as(
        &self,
        c: &Credentials,
        method: Method,
        path: &str,
        desktop: bool,
    ) -> Result<Value, String> {
        let url =
            url::Url::parse(&format!("{}{path}", self.base)).map_err(|_| "accountError.format")?;
        let (cookie, csrf) = headers(c, &url)?;
        let mut request = self
            .http
            .request(method.clone(), url)
            .header("Cookie", cookie)
            .header("Accept", "application/json")
            .header(
                "X-Dumate-Client-Type",
                if desktop { "desktop" } else { "web" },
            )
            .header("Origin", &self.base)
            .header(
                "Referer",
                format!("{}/{}", self.base, if desktop { "" } else { "app" }),
            );
        if desktop {
            request = request
                .header("Content-Type", "application/json")
                .header("X-Requested-With", "XMLHttpRequest");
            if let Some(version) = super::super::tool_manager::get_tool_exe_path("dumate")
                .and_then(|path| {
                    super::super::tool_manager::packaged_desktop_metadata(std::path::Path::new(
                        &path,
                    ))
                })
                .filter(|metadata| metadata["name"] == "qianfan-desktop-app")
                .and_then(|metadata| metadata["version"].as_str().map(str::to_owned))
            {
                request = request.header("dumate-version", version);
            }
        }
        if let Some(csrf) = csrf {
            request = request.header("csrftoken", csrf);
        }
        if method == Method::POST {
            request = request.json(&serde_json::json!({}));
        }
        let response = request.send().await.map_err(|_| "accountError.network")?;
        let status = response.status();
        if status.as_u16() == 401 {
            return Err("accountError.loginRequired".into());
        }
        if status.as_u16() == 429 {
            return Err("accountError.rateLimited".into());
        }
        if !status.is_success() {
            return Err("accountError.network".into());
        }
        let value: Value = response.json().await.map_err(|_| "accountError.format")?;
        result(value)
    }
    pub(super) async fn user(&self, c: &Credentials) -> Result<Value, String> {
        self.request(c, Method::GET, "/api/dumate/user/info").await
    }
    pub(super) async fn speech_session(&self, c: &Credentials) -> Result<Value, String> {
        if c.enterprise() {
            return Err("accountError.unavailable".into());
        }
        self.request_as(c, Method::GET, "/api/dumate/system/sessionToken", true)
            .await
    }
    pub(super) async fn gateway_token(
        &self,
        c: &Credentials,
    ) -> Result<super::proxy::GatewayToken, String> {
        if c.enterprise() {
            return Err("accountError.unavailable".into());
        }
        let value = self
            .request(c, Method::GET, "/api/dumate/app/token/get")
            .await?;
        let token = text(&value, "token")
            .ok_or("accountError.authResponse")?
            .to_owned();
        let expires_at = text(&value, "expired_at")
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|time| time.timestamp())
            .filter(|time| *time > chrono::Utc::now().timestamp())
            .ok_or("accountError.authResponse")?;
        Ok(super::proxy::GatewayToken { token, expires_at })
    }
    pub(super) async fn verify(&self, c: &Credentials) -> Result<(), String> {
        let user = self.user(c).await?;
        if text(&user, "bceUserId") != text(&c.profile, "bceUserId")
            || (!c.enterprise() && text(&user, "bceAccountId") != text(&c.profile, "bceAccountId"))
        {
            return Err("accountError.invalidAccount".into());
        }
        Ok(())
    }
    pub(super) async fn quota(&self, c: &Credentials) -> Result<Quota, String> {
        let value = self.request(c,Method::GET,
            "/api/dumate/points/quota_overview?clientType=desktop&timezone=Asia%2FShanghai&ignoreLoginBonus=true").await?;
        parse_quota(&value, chrono::Utc::now().timestamp())
    }
    pub(super) async fn daily(&self, c: &Credentials) -> Result<DailyReward, String> {
        if c.enterprise() {
            return Err("accountError.unavailable".into());
        }
        let value = self
            .request(c, Method::GET, "/api/dumate/points/loginBonusInfo")
            .await?;
        parse_daily(&value, chrono::Utc::now().timestamp())
    }
    pub(super) async fn claim(&self, c: &Credentials) -> Result<DailyReward, String> {
        let current = self.daily(c).await?;
        if current.status != "CLAIMABLE" {
            return Ok(current);
        }
        let issued = self
            .request(c, Method::POST, "/api/dumate/points/loginBonus")
            .await?;
        if issued != true {
            let confirmed = self.daily(c).await?;
            return if confirmed.status == "CLAIMED_TODAY" {
                Ok(confirmed)
            } else {
                Err("accountError.network".into())
            };
        }
        // The successful POST confirms issuance; a failed follow-up must not invite another click.
        Ok(DailyReward {
            status: "CLAIMED_TODAY".into(),
            ..current
        })
    }
}
fn result(value: Value) -> Result<Value, String> {
    if matches!(value["code"].as_i64(), Some(10001 | 10006)) {
        return Err("accountError.loginRequired".into());
    }
    if value["code"] != 0 || value["success"] == false {
        return Err("accountError.network".into());
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| "accountError.format".into())
}
fn headers(c: &Credentials, url: &url::Url) -> Result<(String, Option<String>), String> {
    let host = url.host_str().ok_or("accountError.format")?;
    let now = chrono::Utc::now().timestamp() as f64;
    let mut cookies = Vec::new();
    let mut csrf = None;
    for cookie in &c.cookies {
        let (Some(name), Some(value), Some(domain)) = (
            text(cookie, "name"),
            cookie["value"].as_str(),
            text(cookie, "domain"),
        ) else {
            continue;
        };
        let domain = domain.trim_start_matches('.');
        if !(host == domain
            || (cookie["hostOnly"] != true && host.ends_with(&format!(".{domain}"))))
            || cookie["expirationDate"]
                .as_f64()
                .is_some_and(|time| time <= now)
            || (cookie["secure"] == true && url.scheme() != "https")
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || value.bytes().any(|b| matches!(b, b'\r' | b'\n' | 0))
        {
            continue;
        }
        let path = cookie["path"].as_str().unwrap_or("/");
        if !url.path().starts_with(path) {
            continue;
        }
        let value = value.trim_matches(['"', '\'']);
        if name == "bce-user-info" {
            csrf = Some(value.to_owned());
        }
        let value = if value.bytes().any(|b| b" :|;=\t".contains(&b)) {
            format!("\"{value}\"")
        } else {
            value.into()
        };
        cookies.push(format!("{name}={value}"));
    }
    if cookies.is_empty() {
        return Err("accountError.loginRequired".into());
    }
    Ok((cookies.join("; "), csrf))
}

pub(super) struct Quota {
    plan: Option<String>,
    expires_at: Option<i64>,
    remaining: Option<f64>,
    total: Option<f64>,
    buckets: Vec<CreditBucket>,
}
impl Quota {
    pub(super) fn apply(self, a: &mut Account) {
        a.plan = if a.enterprise {
            Some("team".into())
        } else {
            self.plan
        };
        a.expires_at = self.expires_at;
        a.remaining = self.remaining;
        a.total = self.total;
        a.credit_buckets = self.buckets;
    }
}
fn parse_quota(v: &Value, now: i64) -> Result<Quota, String> {
    let subscriptions = v["subscription"].as_array().ok_or("accountError.format")?;
    let incremental = v["incremental"].as_array().ok_or("accountError.format")?;
    let active = |p: &&Value| {
        p["status"] == "active" && timestamp(&p["expireDate"]).is_none_or(|end| end > now)
    };
    let current = subscriptions.iter().find(active);
    let plan = match current.and_then(|p| text(p, "packageType")) {
        Some("plan_pro") => Some("pro"),
        Some("plan_max") => Some("max"),
        Some("plan_team") => Some("team"),
        Some("plan_experience") => Some("experience"),
        None if v["isSubscribed"] == false => Some("free"),
        _ => None,
    }
    .map(str::to_owned);
    let mut buckets = Vec::new();
    for p in subscriptions.iter().chain(incremental).filter(active) {
        let name = text(p, "packageType").unwrap_or("unknown");
        let source = text(p, "source").unwrap_or("");
        let total = number(&p["totalPoints"]);
        let used = number(&p["usedPoints"]);
        buckets.push(CreditBucket {
            name: name.into(),
            source: source.into(),
            reward: matches!(name, "grant_point" | "event_bonus")
                || matches!(source, "login_bonus" | "event_bonus"),
            remaining: total.zip(used).map(|(t, u)| (t - u).max(0.0)),
            total,
            expires_at: timestamp(&p["expireDate"]),
        });
    }
    let total = number(&v["totalPoints"]);
    Ok(Quota {
        plan,
        expires_at: current.and_then(|p| timestamp(&p["expireDate"])),
        remaining: total
            .zip(number(&v["usedPoints"]))
            .map(|(t, u)| (t - u).max(0.0)),
        total,
        buckets,
    })
}
fn parse_daily(v: &Value, now: i64) -> Result<DailyReward, String> {
    let issued = v["hasIssued"].as_bool().ok_or("accountError.format")?;
    Ok(DailyReward {
        status: if issued { "CLAIMED_TODAY" } else { "CLAIMABLE" }.into(),
        checked_at: now,
        next_claim_at: (now + 8 * 3600).div_euclid(86400) * 86400 + 86400 - 8 * 3600,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    fn quota() -> Value {
        json!({"isSubscribed":false,"totalPoints":"5500.00","usedPoints":"0.00","subscription":[],
            "incremental":[{"packageType":"grant_point","source":"login_bonus","totalPoints":"500","usedPoints":"0","status":"active","expireDate":2000000000},
            {"packageType":"grant_point","source":"event_bonus","totalPoints":"5000","usedPoints":"0","status":"active","expireDate":2000000000}]})
    }
    #[test]
    fn plans_and_real_balances_do_not_invent_monthly_free_points() {
        let mut account = super::super::tests::fixture().summary();
        parse_quota(&quota(), 1_800_000_000)
            .unwrap()
            .apply(&mut account);
        assert_eq!(account.plan.as_deref(), Some("free"));
        assert_eq!(account.remaining, Some(5500.0));
        assert!(account.credit_buckets.iter().all(|b| b.reward));
        for (package, plan) in [
            ("plan_pro", "pro"),
            ("plan_max", "max"),
            ("plan_team", "team"),
            ("plan_experience", "experience"),
        ] {
            let mut q = quota();
            q["isSubscribed"] = json!(true);
            q["subscription"] = json!([{"packageType":package,"totalPoints":"1234","usedPoints":"34","status":"active","expireDate":2000000000}]);
            parse_quota(&q, 1_800_000_000).unwrap().apply(&mut account);
            assert_eq!(account.plan.as_deref(), Some(plan));
            assert_eq!(account.credit_buckets[0].remaining, Some(1200.0));
        }
        assert!(parse_quota(&json!({}), 1_800_000_000).is_err());
        let mut q = quota();
        q["usedPoints"] = Value::Null;
        parse_quota(&q, 1_800_000_000).unwrap().apply(&mut account);
        assert!(account.remaining.is_none());
        assert!(number(&json!("NaN")).is_none());
        assert!(result(json!({"code":7,"success":true,"result":{}})).is_err());
        assert_eq!(
            result(json!({"code":10001,"result":{}})).unwrap_err(),
            "accountError.loginRequired"
        );
        assert!(parse_daily(&json!({"hasIssued":"false"}), 1_800_000_000).is_err());
    }
    #[test]
    fn cookies_are_host_scoped_and_expired_or_invalid_values_are_not_forwarded() {
        let mut c = super::super::tests::fixture();
        c.cookies.extend([
            json!({"name":"foreign","value":"secret","domain":"other.test"}),
            json!({"name":"expired","value":"secret","domain":"127.0.0.1","expirationDate":1}),
            json!({"name":"bad","value":"a\r\nb","domain":"127.0.0.1"}),
            json!({"name":"","value":"value","domain":"127.0.0.1"}),
        ]);
        let (header, _) = headers(&c, &url::Url::parse("http://127.0.0.1/api").unwrap()).unwrap();
        assert_eq!(header, "bce-long-term-sessionid=fixture-secret");
        assert!(headers(&c, &url::Url::parse("https://other.example/api").unwrap()).is_err());
        assert!(enterprise_origin(
            &json!({"enterpriseApiBaseUrl":"https://user:password@host.test/"})
        )
        .is_err());
    }
    #[tokio::test]
    async fn real_http_refresh_never_claims_and_concurrent_clicks_issue_one_reward() {
        let issued = Arc::new(AtomicBool::new(false));
        let posts = Arc::new(AtomicUsize::new(0));
        let reads = Arc::new(AtomicUsize::new(0));
        let i = issued.clone();
        let j = issued.clone();
        let p = posts.clone();
        let r = reads.clone();
        let app = Router::new()
            .route(
                "/api/dumate/points/quota_overview",
                get(
                    move |axum::extract::Query(q): axum::extract::Query<
                        BTreeMap<String, String>,
                    >| {
                        let r = r.clone();
                        async move {
                            assert_eq!(q.get("ignoreLoginBonus").map(String::as_str), Some("true"));
                            r.fetch_add(1, Ordering::SeqCst);
                            Json(json!({"code":0,"success":true,"result":quota()}))
                        }
                    },
                ),
            )
            .route(
                "/api/dumate/points/loginBonusInfo",
                get(move |headers: axum::http::HeaderMap| {
                    let i = i.clone();
                    async move {
                        assert_eq!(headers["cookie"], "bce-long-term-sessionid=fixture-secret");
                        Json(json!({"code":0,"result":{"hasIssued":i.load(Ordering::SeqCst)}}))
                    }
                }),
            )
            .route(
                "/api/dumate/points/loginBonus",
                post(move || {
                    let j = j.clone();
                    let p = p.clone();
                    async move {
                        assert!(!j.swap(true, Ordering::SeqCst));
                        p.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"code":0,"result":true}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client =
            Arc::new(Client::new(format!("http://{}", listener.local_addr().unwrap())).unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let c = super::super::tests::fixture();
        client.quota(&c).await.unwrap();
        client.daily(&c).await.unwrap();
        assert_eq!(posts.load(Ordering::SeqCst), 0);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        let mut jobs = vec![];
        for _ in 0..2 {
            let client = client.clone();
            let c = c.clone();
            jobs.push(tokio::spawn(async move {
                let lock = super::super::request_lock("http-fixture").unwrap();
                let _guard = lock.lock().await;
                client.claim(&c).await.unwrap()
            }));
        }
        for job in jobs {
            assert_eq!(job.await.unwrap().status, "CLAIMED_TODAY");
        }
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
