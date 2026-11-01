//! Native Copilot CLI browser sign-in, isolated from the user's active account.
//! The official runtime owns OAuth and OS-vault writes; editor OAuth is not used.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[path = "copilot_desktop.rs"]
pub(crate) mod desktop;

const LOGIN_SECONDS: i64 = 60;
const INTEGRATION: &str = "copilot-developer-cli";
static PENDING: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
static STORE_LOCK: Mutex<()> = Mutex::new(());
static APPLY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub login: String,
    pub host: String,
    pub plan: Option<String>,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub reset_at: Option<i64>,
    pub active: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    summary: Account,
    access_token: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default, rename = "activeTools")]
    active_tools: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    login_id: String,
    expires_at: i64,
}

struct Pending {
    id: String,
    root: PathBuf,
    dir: PathBuf,
    expires: i64,
    child: Child,
}

fn kill_owned_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(windows)]
    {
        // npm's cmd wrapper can own a second process. Kill only this owned tree.
        let _ = crate::utils::process::command("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for Pending {
    fn drop(&mut self) {
        kill_owned_child(&mut self.child);
        if let (Ok(root), Ok(dir)) = (self.root.canonicalize(), self.dir.canonicalize()) {
            if dir.parent() == Some(root.as_path())
                && dir
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("login-"))
            {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }
}

fn store() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/copilot-accounts"))
}

fn path_at(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("accountError.invalidAccount".into());
    }
    Ok(dir.join(format!("{id}.json")))
}

fn read_saved(id: &str) -> Result<Saved, String> {
    read_saved_at(&store()?, id)
}

fn read_saved_at(dir: &Path, id: &str) -> Result<Saved, String> {
    let mut saved: Saved = super::cursor_auth::read(&path_at(dir, id)?)?;
    if let Some(encoded) = saved.access_token.strip_prefix("enc:v1:") {
        if encoded.len() % 2 != 0 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("accountError.keychain".into());
        }
    }
    saved.access_token = super::model_manager::decrypt_key_for_use(&saved.access_token);
    if saved.access_token.is_empty()
        || !saved
            .access_token
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err("accountError.keychain".into());
    }
    Ok(saved)
}

fn write_saved(path: &Path, saved: &Saved) -> Result<(), String> {
    let mut protected = saved.clone();
    protected.access_token = super::model_manager::encrypt_key_for_storage(&saved.access_token);
    if !protected.access_token.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    super::cursor_auth::write(path, &protected)
}

fn validated_host(host: &str) -> Result<String, String> {
    let host = host.trim().trim_end_matches('/');
    let host = host.strip_prefix("https://").unwrap_or(host);
    if host == "github.com"
        || host.ends_with(".ghe.com") && !host.contains('/') && !host.contains('@')
    {
        Ok(host.to_string())
    } else {
        Err("accountError.invalidAccount".into())
    }
}

fn saved_from_native(config: &Value) -> Result<Saved, String> {
    let user = &config["lastLoggedInUser"];
    let login = user["login"]
        .as_str()
        .filter(|login| !login.trim().is_empty())
        .ok_or("accountError.invalidAccount")?;
    let raw_host = user["host"].as_str().unwrap_or("github.com");
    let host = validated_host(raw_host)?;
    let key = format!("https://{host}:{login}");
    let bare_key = format!("{host}:{login}");
    let raw_key = format!("{raw_host}:{login}");
    // Current CLI authTokens entries wrap the token; legacy copilotTokens were strings.
    // An existing current map is authoritative, so stale legacy tokens cannot win.
    let token = if let Some(tokens) = config.get("authTokens") {
        let tokens = tokens.as_object().ok_or("accountError.format")?;
        [&raw_key, &key, &bare_key]
            .into_iter()
            .find_map(|key| tokens.get(key).and_then(|entry| entry["token"].as_str()))
    } else {
        config["copilotTokens"][&key]
            .as_str()
            .or_else(|| config["copilotTokens"][&bare_key].as_str())
    }
    .filter(|token| !token.is_empty() && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
    .ok_or("accountError.loginRequired")?;
    Ok(Saved {
        summary: Account {
            id: format!("{:x}", Sha256::digest(format!("{host}\0{login}"))),
            login: login.into(),
            host,
            plan: None,
            remaining: None,
            total: None,
            reset_at: None,
            active: false,
        },
        access_token: token.into(),
        base_url: None,
        active_tools: Some(Vec::new()),
    })
}

fn read_login_config(path: &Path) -> Result<Value, String> {
    let content = fs::read_to_string(path).map_err(|_| "accountError.read")?;
    // The official CLI prefixes managed config.json with // comments.
    let config = jsonc_parser::parse_to_serde_value::<Value>(&content, &Default::default())
        .map_err(|_| "accountError.format")?;
    if !config.is_object() {
        return Err("accountError.format".into());
    }
    Ok(config)
}

fn save_at(dir: &Path, mut saved: Saved) -> Result<Account, String> {
    let path = path_at(dir, &saved.summary.id)?;
    if path.exists() {
        let previous: Saved = super::cursor_auth::read(&path)?;
        saved.summary.plan = previous.summary.plan;
        saved.summary.remaining = previous.summary.remaining;
        saved.summary.total = previous.summary.total;
        saved.summary.reset_at = previous.summary.reset_at;
        saved.summary.active = previous.summary.active;
        saved.base_url = previous.base_url;
        saved.active_tools = previous.active_tools;
    }
    write_saved(&path, &saved)?;
    Ok(saved.summary)
}

fn cli_command(executable: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        super::process_manager::copilot_native_command(executable)
    }
    #[cfg(not(windows))]
    {
        crate::utils::process::command(executable)
    }
}

fn executable(tool_id: &str) -> Result<String, String> {
    if !matches!(tool_id, "copilot" | "copilotdesktop") {
        return Err("accountError.invalidAccount".into());
    }
    if super::tool_manager::get_tool_exe_path(tool_id).is_none() {
        return Err("accountError.initializeClient".into());
    }
    // Account OAuth is delegated to the official CLI runtime, not a reused editor app.
    super::tool_manager::get_copilot_runtime_path()
        .ok_or_else(|| "accountError.initializeClient".into())
}

pub async fn start_login(tool_id: &str) -> Result<LoginStart, String> {
    let executable = executable(tool_id)?;
    let mut pending = PENDING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "accountError.busy")?;
    *pending = None;
    let id = uuid::Uuid::new_v4().to_string();
    let root = store()?;
    let dir = root.join(format!("login-{id}"));
    super::cursor_auth::write(
        &dir.join("settings.json"),
        &json!({"storeTokenPlaintext":true}),
    )?;
    let child = cli_command(&executable)
        .args(["login", "--web-flow"])
        .env("COPILOT_HOME", &dir)
        .env("COPILOT_DISABLE_KEYTAR", "1")
        .env_remove("COPILOT_GITHUB_TOKEN")
        .env_remove("COPILOT_SDK_AUTH_TOKEN")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let child = match child {
        Ok(child) => child,
        Err(_) => {
            // This direct child directory was just created by this attempt.
            let _ = fs::remove_file(dir.join("settings.json"));
            let _ = fs::remove_dir(&dir);
            return Err("accountError.auth".into());
        }
    };
    let expires = chrono::Utc::now().timestamp() + LOGIN_SECONDS;
    *pending = Some(Pending {
        id: id.clone(),
        root,
        dir,
        expires,
        child,
    });
    let timeout_id = id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(LOGIN_SECONDS as u64)).await;
        let _ = cancel_login(&timeout_id);
    });
    Ok(LoginStart {
        login_id: id,
        expires_at: expires,
    })
}

pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let mut pending = PENDING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "accountError.busy")?;
    let Some(login) = pending.as_mut().filter(|login| login.id == id) else {
        return Ok(None);
    };
    if chrono::Utc::now().timestamp() >= login.expires {
        *pending = None;
        return Err("accountError.expired".into());
    }
    match login.child.try_wait().map_err(|_| "accountError.auth")? {
        None => return Ok(None),
        Some(status) if !status.success() => {
            *pending = None;
            return Err("accountError.auth".into());
        }
        Some(_) => {}
    }
    let result = (|| {
        let config = read_login_config(&login.dir.join("config.json"))?;
        let saved = saved_from_native(&config)?;
        let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
        let account = save_at(&store()?, saved)?;
        super::smart_router::forget_account_authentication(
            crate::models::model::AccountProvider::Copilot,
            &account.id,
        );
        Ok(Some(account))
    })();
    *pending = None;
    result
}

pub fn cancel_login(id: &str) -> Result<(), String> {
    let mut pending = PENDING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "accountError.busy")?;
    if pending.as_ref().is_some_and(|login| login.id == id) {
        *pending = None;
    }
    Ok(())
}

fn list_at(dir: &Path) -> Result<Vec<Account>, String> {
    list_for_tool_at(dir, None)
}

fn list_for_tool_at(dir: &Path, tool_id: Option<&str>) -> Result<Vec<Account>, String> {
    if tool_id.is_some_and(|id| !matches!(id, "copilot" | "copilotdesktop")) {
        return Err("accountError.invalidAccount".into());
    }
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut accounts = Vec::new();
    for entry in fs::read_dir(dir).map_err(|_| "accountError.read")? {
        let path = entry.map_err(|_| "accountError.read")?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let saved: Saved = super::cursor_auth::read(&path)?;
            let mut summary = saved.summary;
            if let Some(tools) = saved.active_tools {
                summary.active = tool_id.is_some_and(|id| tools.iter().any(|tool| tool == id));
            }
            accounts.push(summary);
        }
    }
    accounts.sort_by(|left, right| {
        left.login
            .cmp(&right.login)
            .then(left.host.cmp(&right.host))
    });
    Ok(accounts)
}

pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
    list_at(&store()?)
}

pub async fn list(tool_id: Option<&str>) -> Result<Vec<Account>, String> {
    let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
    list_for_tool_at(&store()?, tool_id)
}

fn set_active_at(dir: &Path, id: &str, tool_id: &str) -> Result<Account, String> {
    if !matches!(tool_id, "copilot" | "copilotdesktop") {
        return Err("accountError.invalidAccount".into());
    }
    let _: Saved = super::cursor_auth::read(&path_at(dir, id)?)?;
    for account in list_at(dir)? {
        let path = path_at(dir, &account.id)?;
        let mut row: Saved = super::cursor_auth::read(&path)?;
        let previous_tools = row.active_tools.clone();
        let mut tools = row.active_tools.take().unwrap_or_else(|| {
            if row.summary.active {
                vec!["copilot".into(), "copilotdesktop".into()]
            } else {
                Vec::new()
            }
        });
        tools.retain(|tool| tool != tool_id);
        if row.summary.id == id {
            tools.push(tool_id.into());
        }
        if previous_tools.as_ref() == Some(&tools) {
            continue;
        }
        row.active_tools = Some(tools);
        write_saved(&path, &row)?;
    }
    list_for_tool_at(dir, Some(tool_id))?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| "accountError.invalidAccount".into())
}

// The framing and methods are from github/copilot-sdk's public generated RPC.
fn runtime_content_length(header: &str) -> Option<usize> {
    let (name, value) = header.trim().split_once(':')?;
    if name.eq_ignore_ascii_case("Content-Length") {
        value.trim().parse().ok()
    } else {
        None
    }
}

struct Runtime {
    child: tokio::process::Child,
    reader: BufReader<tokio::process::ChildStdout>,
    writer: tokio::process::ChildStdin,
    next_id: u64,
}

impl Runtime {
    async fn start(executable: &str) -> Result<Self, String> {
        let mut command: tokio::process::Command = cli_command(executable).into();
        let mut child = command
            .args([
                "--headless",
                "--stdio",
                "--no-auto-update",
                "--no-auto-login",
            ])
            .env_remove("COPILOT_GITHUB_TOKEN")
            .env_remove("COPILOT_SDK_AUTH_TOKEN")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                #[cfg(test)]
                if std::env::var("ECHOBIRD_LIVE_COPILOT_RUNTIME").as_deref() == Ok("1") {
                    eprintln!("provider=Copilot phase=runtime_spawn status=failed category={:?} osCode={:?}", error.kind(), error.raw_os_error());
                }
                let _ = error;
                "accountError.auth"
            })?;
        let reader = BufReader::new(child.stdout.take().ok_or("accountError.auth")?);
        let writer = child.stdin.take().ok_or("accountError.auth")?;
        let mut runtime = Self {
            child,
            reader,
            writer,
            next_id: 0,
        };
        let handshake = runtime
            .request("connect", json!({"supportedTaskKinds":["client"]}))
            .await?;
        if handshake["protocolVersion"].as_u64() != Some(3) {
            return Err("Unsupported Copilot runtime protocol; update GitHub Copilot CLI.".into());
        }
        Ok(runtime)
    }

    async fn send(&mut self, message: &Value) -> Result<(), String> {
        let body = serde_json::to_vec(message).map_err(|_| "accountError.format")?;
        self.writer
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .map_err(|_| "accountError.auth")?;
        self.writer
            .write_all(&body)
            .await
            .map_err(|_| "accountError.auth")?;
        self.writer
            .flush()
            .await
            .map_err(|_| "accountError.auth".into())
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let mut length = None;
                loop {
                    let mut header = String::new();
                    if self.reader.read_line(&mut header).await.map_err(|_| "accountError.read")? == 0 {
                        #[cfg(test)]
                        if std::env::var("ECHOBIRD_LIVE_COPILOT_RUNTIME").as_deref() == Ok("1") {
                            let code = self.child.try_wait().ok().flatten().and_then(|status| status.code());
                            eprintln!("provider=Copilot phase=runtime_receive status=failed category=eof childExitCode={code:?}");
                        }
                        return Err("accountError.auth".into());
                    }
                    if header.trim().is_empty() { break; }
                    if let Some(value) = runtime_content_length(&header) {
                        #[cfg(test)]
                        if std::env::var("ECHOBIRD_LIVE_COPILOT_RUNTIME").as_deref() == Ok("1") {
                            let canonical = header.trim().starts_with("Content-Length:");
                            eprintln!("provider=Copilot phase=runtime_frame frame=content_length canonicalHeader={canonical}");
                        }
                        length = Some(value);
                    }
                }
                let Some(length) = length.filter(|length| *length <= 8 * 1024 * 1024) else {
                    #[cfg(test)]
                    if std::env::var("ECHOBIRD_LIVE_COPILOT_RUNTIME").as_deref() == Ok("1") {
                        eprintln!("provider=Copilot phase=runtime_frame status=failed category=length_missing_or_invalid");
                    }
                    return Err("accountError.format".into());
                };
                let mut body = vec![0; length];
                self.reader.read_exact(&mut body).await.map_err(|_| "accountError.read")?;
                let message: Value = serde_json::from_slice(&body).map_err(|_| "accountError.format")?;
                if message["id"] == id && message.get("method").is_none() {
                    // Runtime errors may contain credential context. Never forward raw RPC errors.
                    if message.get("error").is_some() {
                        #[cfg(test)]
                        if std::env::var("ECHOBIRD_LIVE_COPILOT_RUNTIME").as_deref() == Ok("1") {
                            let code = message["error"]["code"].as_i64();
                            eprintln!("provider=Copilot phase=runtime_rpc status=failed category=rpc rpcCode={code:?}");
                        }
                        return Err("accountError.auth".into());
                    }
                    return Ok(message["result"].clone());
                }
                if message.get("method").is_some() && message.get("id").is_some() {
                    self.send(&json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Unsupported host request"}})).await?;
                }
            }
        }).await.map_err(|_| "accountError.expired")?
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(id) = self.child.id() {
            let _ = crate::utils::process::command("taskkill.exe")
                .args(["/PID", &id.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.start_kill();
    }
}

pub async fn import_native_accounts() -> Result<Vec<Account>, String> {
    let executable = executable("copilot")?;
    let mut runtime = Runtime::start(&executable).await?;
    let users = runtime.request("account.getAllUsers", json!({})).await?;
    let rows = users["users"].as_array().ok_or("accountError.format")?;
    let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
    for row in rows {
        let identity = &row["authInfo"];
        if identity["type"] != "user" {
            continue;
        }
        let Some(token) = row["token"].as_str().filter(|token| !token.is_empty()) else {
            continue;
        };
        let host = validated_host(identity["host"].as_str().unwrap_or("github.com"))?;
        let login = identity["login"]
            .as_str()
            .ok_or("accountError.invalidAccount")?;
        let saved = saved_from_native(
            &json!({"lastLoggedInUser":{"host":host,"login":login},"copilotTokens":{format!("https://{host}:{login}"):token}}),
        )?;
        let account = save_at(&store()?, saved)?;
        super::smart_router::forget_account_authentication(
            crate::models::model::AccountProvider::Copilot,
            &account.id,
        );
    }
    list_at(&store()?)
}

pub async fn switch(tool_id: &str, id: &str) -> Result<Account, String> {
    let _apply_guard = APPLY_LOCK.lock().await;
    let saved = read_saved(id)?;
    if tool_id == "copilotdesktop" {
        let mut session = desktop::WriteSession::start().await?;
        let switched = switch_desktop(&saved, &mut session.bridge).await;
        let cleanup = session.finish();
        switched.and(cleanup)?;
    } else {
        let executable = executable(tool_id)?;
        let mut runtime = Runtime::start(&executable).await?;
        let result = runtime.request("account.login", json!({"host":format!("https://{}",saved.summary.host),"login":saved.summary.login,"token":saved.access_token})).await?;
        if result["storedInVault"] != true {
            return Err("accountError.write".into());
        }
        let restore = super::tool_config_manager::restore_copilot_account_config();
        if !restore.success {
            return Err(restore.message);
        }
    }
    let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
    set_active_at(&store()?, id, tool_id)
}

async fn switch_desktop(saved: &Saved, bridge: &mut desktop::Desktop) -> Result<(), String> {
    let change = desktop::write_account(
        &mut desktop::database()?,
        &saved.summary.login,
        &saved.summary.host,
        &saved.access_token,
    )?;
    let switched = async {
        let response = bridge
            .request(
                json!({"type":"set_default_github_account","account_id":change.id}),
                "github_auth_success",
            )
            .await?;
        if response["account_id"] != change.id {
            return Err("accountError.auth".into());
        }
        let selected: bool = desktop::database()?
            .query_row(
                "SELECT is_default=1 FROM accounts WHERE id=?1",
                [&change.id],
                |row| row.get(0),
            )
            .map_err(|_| "accountError.read")?;
        if !selected {
            return Err("accountError.write".into());
        }
        let restored =
            super::tool_config_manager::restore_copilot_desktop_account_config_with_bridge(bridge)
                .await;
        if !restored.success {
            return Err(restored.message);
        }
        Ok(())
    }
    .await;
    if let Err(error) = switched {
        desktop::rollback_account(&mut desktop::database()?, &change)?;
        // Rebind the running client's in-memory auth to the restored database.
        let rebound = if let Some(previous) = change.defaults.first() {
            bridge
                .request(
                    json!({"type":"set_default_github_account","account_id":previous}),
                    "github_auth_success",
                )
                .await
        } else {
            bridge
                .request(json!({"type":"check_github_auth"}), "github_auth_required")
                .await
        };
        if rebound.is_err() {
            return Err("accountError.write".into());
        }
        return Err(error);
    }
    Ok(())
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".to_string())
}

fn valid_api_url(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value).map_err(|_| "accountError.format")?;
    let host = url.host_str().ok_or("accountError.format")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !(host == "api.githubcopilot.com"
            || host.ends_with(".githubcopilot.com")
            || host.ends_with(".ghe.com"))
    {
        return Err("accountError.format".into());
    }
    Ok(value.trim_end_matches('/').into())
}

async fn user_data(saved: &Saved) -> Result<Value, String> {
    let api = if saved.summary.host == "github.com" {
        "https://api.github.com".into()
    } else {
        format!("https://api.{}", saved.summary.host)
    };
    let response = client()?
        .get(format!("{api}/copilot_internal/user"))
        .header("Authorization", format!("token {}", saved.access_token))
        .header("Accept", "application/json")
        .header("User-Agent", "EchoBird")
        .header("Copilot-Integration-Id", INTEGRATION)
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.format".into())
}

fn signed(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|value: &f64| value.is_finite())
}

fn count(value: &Value) -> Option<f64> {
    signed(value).filter(|value| *value >= 0.0)
}

fn reset_date(value: &Value) -> Option<i64> {
    if let Some(seconds) = value.as_i64().filter(|seconds| *seconds > 0) {
        return Some(if seconds > 1_000_000_000_000 {
            seconds / 1000
        } else {
            seconds
        });
    }
    let text = value.as_str()?.trim();
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|time| time.timestamp())
        .ok()
        .or_else(|| {
            let date = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
            Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
        })
}

/// The monthly allowance as remaining, total and reset. Paid plans count premium
/// requests; Free counts chat messages and completions separately, of which the
/// more used one is the limit the user meets first.
fn monthly_quota(data: &Value) -> (Option<f64>, Option<f64>, Option<i64>) {
    let premium = &data["quota_snapshots"]["premium_interactions"];
    let paid = count(&premium["entitlement"])
        .filter(|total| *total > 0.0)
        // Overage leaves premium requests below zero, which still means used up.
        .zip(signed(&premium["remaining"]).map(|remaining| remaining.max(0.0)));
    let quota = paid.or_else(|| {
        ["chat", "completions"]
            .iter()
            .filter_map(|kind| {
                let total = count(&data["monthly_quotas"][kind]).filter(|total| *total > 0.0)?;
                Some((total, count(&data["limited_user_quotas"][kind])?.min(total)))
            })
            .min_by(|a, b| (a.1 / a.0).total_cmp(&(b.1 / b.0)))
    });
    let reset_at = reset_date(&data["quota_reset_date_utc"])
        .or_else(|| reset_date(&data["quota_reset_date"]))
        .or_else(|| reset_date(&data["limited_user_reset_date"]));
    match quota {
        Some((total, remaining)) => (Some(remaining), Some(total), reset_at),
        None => (None, None, reset_at),
    }
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let mut saved = read_saved(id)?;
    let data = user_data(&saved).await?;
    saved.summary.plan = data["copilot_plan"].as_str().map(str::to_string);
    let (remaining, total, reset_at) = monthly_quota(&data);
    saved.summary.remaining = remaining;
    saved.summary.total = total;
    saved.summary.reset_at = reset_at;
    if let Some(api) = data["endpoints"]["api"].as_str() {
        saved.base_url = Some(valid_api_url(api)?);
    }
    let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
    if !path_at(&store()?, id)?.exists() {
        return Err("accountError.invalidAccount".into());
    }
    let latest = read_saved(id)?;
    saved.access_token = latest.access_token;
    saved.active_tools = latest.active_tools;
    saved.summary.active = latest.summary.active;
    write_saved(&path_at(&store()?, id)?, &saved)?;
    super::account_proxy::forget_preparation_metadata(
        crate::models::model::AccountProvider::Copilot,
        id,
    );
    Ok(saved.summary)
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().map_err(|_| "accountError.busy")?;
    fs::remove_file(path_at(&store()?, id)?).map_err(|_| "accountError.write".to_string())?;
    super::account_proxy::forget_preparation_metadata(
        crate::models::model::AccountProvider::Copilot,
        id,
    );
    Ok(())
}

pub async fn proxy_credentials(id: &str, force_refresh: bool) -> Result<Value, String> {
    // Native OAuth has no refresh token. The proxy must not query account quota
    // to resolve a model request; an explicit refresh caches enterprise endpoints.
    let _ = force_refresh;
    let saved = read_saved(id)?;
    Ok(
        json!({"accessToken":saved.access_token,"baseUrl":valid_api_url(saved.base_url.as_deref().unwrap_or("https://api.githubcopilot.com"))?,
        "host":saved.summary.host,"login":saved.summary.login,"credentialSource":"copilot-cli",
        "headers":{"Copilot-Integration-Id":INTEGRATION,"Editor-Version":"copilot/1.0.95","X-GitHub-Api-Version":"2026-07-01"}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paid_plans_use_premium_requests_and_free_uses_its_most_used_allowance() {
        assert_eq!(
            monthly_quota(&json!({
                "quota_reset_date":"2026-09-15",
                "quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":"45"}},
                "monthly_quotas":{"chat":50},"limited_user_quotas":{"chat":1}
            })),
            (Some(45.0), Some(300.0), Some(1789430400))
        );
        assert_eq!(
            monthly_quota(&json!({
                "quota_snapshots":{"premium_interactions":{"unlimited":true,"entitlement":0}},
                "monthly_quotas":{"chat":50,"completions":2000},
                "limited_user_quotas":{"chat":40,"completions":"500"},
                "limited_user_reset_date":"2026-09-15"
            })),
            (Some(500.0), Some(2000.0), Some(1789430400))
        );
        assert_eq!(
            monthly_quota(&json!({
                "quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":-12}},
                "monthly_quotas":{"chat":50},"limited_user_quotas":{"chat":40}
            })),
            (Some(0.0), Some(300.0), None)
        );
        assert_eq!(
            monthly_quota(&json!({"quota_reset_date_utc":"2026-09-15T08:00:00Z"})),
            (None, None, Some(1789459200))
        );
        assert_eq!(
            monthly_quota(&json!({"monthly_quotas":{"chat":0},"limited_user_quotas":{"chat":0}})),
            (None, None, None)
        );
    }

    #[test]
    fn current_auth_tokens_select_the_exact_signed_in_identity() {
        let config = json!({
            "lastLoggedInUser":{"host":"https://github.com","login":"fixture"},
            "authTokens":{
                "cached:https://github.com:fixture":{"token":"fixture-cached-token"},
                "https://github.com:another":{"token":"fixture-other-account"},
                "https://github.com:fixture":{"token":"fixture-selected-token"}
            }
        });
        let saved = saved_from_native(&config).unwrap();
        assert_eq!(saved.summary.host, "github.com");
        assert_eq!(saved.summary.login, "fixture");
        assert_eq!(saved.access_token, "fixture-selected-token");

        let mut raw = config.clone();
        raw["lastLoggedInUser"]["host"] = json!("https://github.com/");
        raw["authTokens"]["https://github.com/:fixture"] =
            json!({"token":"fixture-raw-host-token"});
        assert_eq!(
            saved_from_native(&raw).unwrap().access_token,
            "fixture-raw-host-token"
        );
    }

    #[test]
    fn current_auth_tokens_are_authoritative_and_reject_invalid_or_unmatched_tokens() {
        let legacy = json!({
            "lastLoggedInUser":{"host":"https://github.com","login":"fixture"},
            "copilotTokens":{"https://github.com:fixture":"fixture-legacy-token"}
        });
        for token in [json!(""), json!(" "), json!(null), json!(7), json!({})] {
            let mut config = legacy.clone();
            config["authTokens"] = json!({"https://github.com:fixture":{"token":token}});
            assert_eq!(
                saved_from_native(&config).err(),
                Some("accountError.loginRequired".into())
            );
        }
        for entries in [
            json!({}),
            json!({"https://github.com:another":{"token":"fixture-other-token"}}),
            json!({"https://github.com:fixture":"fixture-unwrapped-token"}),
        ] {
            let mut config = legacy.clone();
            config["authTokens"] = entries;
            assert_eq!(
                saved_from_native(&config).err(),
                Some("accountError.loginRequired".into())
            );
        }
        for invalid in [json!(null), json!([]), json!("fixture-token")] {
            let mut config = legacy.clone();
            config["authTokens"] = invalid;
            assert_eq!(
                saved_from_native(&config).err(),
                Some("accountError.format".into())
            );
        }
    }

    #[test]
    fn legacy_tokens_and_current_normalized_keys_remain_supported() {
        for field in ["authTokens", "copilotTokens"] {
            for key in ["https://github.com:fixture", "github.com:fixture"] {
                let mut config = json!({
                    "lastLoggedInUser":{"host":"https://github.com/","login":"fixture"}
                });
                let token = if field == "authTokens" {
                    json!({"token":"fixture-token"})
                } else {
                    json!("fixture-token")
                };
                config[field] = json!({key:token});
                assert_eq!(
                    saved_from_native(&config).unwrap().access_token,
                    "fixture-token"
                );
            }
        }
    }

    #[test]
    fn login_config_accepts_native_comments_and_preserves_string_contents() {
        let dir =
            std::env::temp_dir().join(format!("copilot-login-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("config.json");
        let content = r#"// User settings belong in settings.json.
// This file is managed automatically.
{
  "lastLoggedInUser": {"host":"https://github.com", "login":"fixture"},
  "authTokens": {"https://github.com:fixture":{"token":"fixture-token-with-//-literal"}},
  "metadata": {"url":"https://fixture.invalid/a//b", "text":"/* literal */"},
}"#;
        fs::write(&path, content).unwrap();
        assert_eq!(
            super::super::cursor_auth::read::<Value>(&path).unwrap_err(),
            "accountError.format"
        );
        let config = read_login_config(&path).unwrap();
        assert_eq!(config["metadata"]["url"], "https://fixture.invalid/a//b");
        assert_eq!(config["metadata"]["text"], "/* literal */");
        let saved = saved_from_native(&config).unwrap();
        assert_eq!(saved.summary.login, "fixture");
        assert_eq!(saved.access_token, "fixture-token-with-//-literal");
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn login_config_keeps_legacy_json_and_rejects_invalid_documents() {
        let dir =
            std::env::temp_dir().join(format!("copilot-login-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("config.json");
        let legacy = json!({"lastLoggedInUser":{"host":"https://github.com","login":"legacy"},"copilotTokens":{"https://github.com:legacy":"fixture-legacy"}});
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(read_login_config(&path).unwrap(), legacy);
        assert_eq!(
            saved_from_native(&read_login_config(&path).unwrap())
                .unwrap()
                .access_token,
            "fixture-legacy"
        );
        for invalid in ["", "[]", "null", "{\"lastLoggedInUser\":}", "{broken"] {
            fs::write(&path, invalid).unwrap();
            assert_eq!(read_login_config(&path).unwrap_err(), "accountError.format");
        }
        fs::write(
            &path,
            "// Valid JSONC, but missing the authenticated identity.\n{}",
        )
        .unwrap();
        assert_eq!(
            saved_from_native(&read_login_config(&path).unwrap()).err(),
            Some("accountError.invalidAccount".into())
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn runtime_accepts_content_length_headers_case_insensitively() {
        for header in [
            "Content-Length: 81\r\n",
            "content-length: 81\r\n",
            "CONTENT-LENGTH:81\r\n",
            "cOnTeNt-LeNgTh: 81\r\n",
        ] {
            assert_eq!(runtime_content_length(header), Some(81));
        }
        assert_eq!(
            runtime_content_length("Content-Type: application/json\r\n"),
            None
        );
        assert_eq!(runtime_content_length("Content-Length: invalid\r\n"), None);
        assert_eq!(runtime_content_length("Content-Length: -1\r\n"), None);
    }

    #[tokio::test]
    #[ignore = "explicit official runtime connect/ping; ECHOBIRD_LIVE_COPILOT_RUNTIME=1 and isolated COPILOT_HOME required"]
    async fn live_official_runtime_connect_ping_probe() -> Result<(), String> {
        if std::env::var("ECHOBIRD_LIVE_COPILOT_RUNTIME").as_deref() != Ok("1") {
            return Err("Explicit Copilot runtime probe flag is required.".into());
        }
        let home = std::env::var_os("COPILOT_HOME")
            .map(PathBuf::from)
            .ok_or("An isolated temporary Copilot home is required.")?
            .canonicalize()
            .map_err(|_| "The isolated Copilot home must already exist.")?;
        let temporary = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| "The temporary root is unavailable.")?;
        let native = dirs::home_dir()
            .ok_or("The native home is unavailable.")?
            .join(".copilot");
        let native = native.canonicalize().unwrap_or(native);
        if !home.is_dir()
            || home.parent() != Some(temporary.as_path())
            || home == native
            || !home.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with("echobird-copilot-runtime-")
            })
            || fs::read_dir(&home)
                .map_err(|_| "The isolated Copilot home is unreadable.")?
                .next()
                .is_some()
        {
            return Err("The probe requires a new, dedicated temporary Copilot home.".into());
        }
        let executable = super::super::tool_manager::get_copilot_runtime_path()
            .ok_or("The verified official Copilot runtime is unavailable.")?;
        let mut runtime = Runtime::start(&executable)
            .await
            .map_err(|_| "Copilot runtime connect failed.")?;
        let ping = runtime
            .request("ping", json!({"message":"echobird-runtime-probe"}))
            .await
            .map_err(|_| "Copilot runtime ping failed.")?;
        let nonce_matches = ping["message"] == "echobird-runtime-probe";
        let fixed_pong = ping["message"] == "pong";
        let nonce_pong = ping["message"] == "pong: echobird-runtime-probe";
        let protocol_present = ping.get("protocolVersion").is_some();
        let protocol_version = ping["protocolVersion"].as_u64();
        let timestamp_valid = ping["timestamp"].is_number()
            || ping["timestamp"]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty());
        let keys = ping
            .as_object()
            .map(|object| object.keys().collect::<Vec<_>>());
        eprintln!("provider=Copilot phase=runtime_ping nonceMatches={nonce_matches} fixedPong={fixed_pong} noncePong={nonce_pong} protocolPresent={protocol_present} protocolVersion={protocol_version:?} timestampValid={timestamp_valid} keys={keys:?}");
        // Released CLI builds can acknowledge ping with a fixed pong instead of echoing.
        // The official SDK issue #1356 also records both timestamp wire types.
        if (!nonce_matches && !fixed_pong && !nonce_pong)
            || protocol_version != Some(3)
            || !timestamp_valid
        {
            return Err("Copilot runtime ping returned an unexpected protocol response.".into());
        }
        drop(runtime);
        eprintln!("provider=Copilot phase=runtime_connect status=ok protocolVersion=3");
        eprintln!("provider=Copilot phase=runtime_ping status=ok");
        Ok(())
    }

    #[test]
    fn saved_selections_are_separate_for_cli_and_desktop_and_listing_does_not_write() {
        let dir = std::env::temp_dir().join(format!("copilot-selection-{}", uuid::Uuid::new_v4()));
        let cli=saved_from_native(&json!({"lastLoggedInUser":{"host":"https://github.com","login":"cli"},"copilotTokens":{"https://github.com:cli":"fixture-cli"}})).unwrap();
        let desktop=saved_from_native(&json!({"lastLoggedInUser":{"host":"https://github.com","login":"desktop"},"copilotTokens":{"https://github.com:desktop":"fixture-desktop"}})).unwrap();
        save_at(&dir, cli.clone()).unwrap();
        save_at(&dir, desktop.clone()).unwrap();
        set_active_at(&dir, &cli.summary.id, "copilot").unwrap();
        set_active_at(&dir, &desktop.summary.id, "copilotdesktop").unwrap();
        let cli_path = path_at(&dir, &cli.summary.id).unwrap();
        let desktop_path = path_at(&dir, &desktop.summary.id).unwrap();
        let before = fs::read(&cli_path).unwrap();
        let active = |tool| {
            list_for_tool_at(&dir, Some(tool))
                .unwrap()
                .into_iter()
                .filter(|account| account.active)
                .map(|account| account.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(active("copilot"), vec![cli.summary.id.clone()]);
        assert_eq!(active("copilotdesktop"), vec![desktop.summary.id.clone()]);
        assert!(list_for_tool_at(&dir, Some("unrelated")).is_err());
        assert_eq!(fs::read(&cli_path).unwrap(), before);
        set_active_at(&dir, &desktop.summary.id, "copilotdesktop").unwrap();
        assert_eq!(active("copilot"), vec![cli.summary.id]);
        assert!(list_at(&dir).unwrap().iter().all(|row| !row.active));
        fs::remove_file(cli_path).unwrap();
        fs::remove_file(desktop_path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn own_store_list_is_read_only_and_never_serializes_credentials() {
        let dir =
            std::env::temp_dir().join(format!("echobird-copilot-account-{}", uuid::Uuid::new_v4()));
        let saved = saved_from_native(&json!({"lastLoggedInUser":{"host":"https://github.com","login":"fixture"},"copilotTokens":{"https://github.com:fixture":"fixture-private-token"}})).unwrap();
        save_at(&dir, saved.clone()).unwrap();
        let path = path_at(&dir, &saved.summary.id).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(!String::from_utf8(before.clone())
            .unwrap()
            .contains("fixture-private-token"));
        let protected: Saved = super::super::cursor_auth::read(&path).unwrap();
        assert!(protected.access_token.starts_with("enc:v1:"));
        assert_eq!(
            read_saved_at(&dir, &saved.summary.id).unwrap().access_token,
            "fixture-private-token"
        );
        let accounts = list_at(&dir).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(accounts.len(), 1);
        assert!(accounts[0].remaining.is_none());
        assert!(!serde_json::to_string(&accounts)
            .unwrap()
            .contains("fixture-private-token"));
        assert!(path_at(&dir, "../native").is_err());
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn legacy_and_unreadable_credentials_are_not_decrypted_or_migrated_by_listing() {
        let dir = std::env::temp_dir().join(format!("copilot-legacy-{}", uuid::Uuid::new_v4()));
        let mut saved = saved_from_native(&json!({"lastLoggedInUser":{"host":"github.com","login":"legacy"},"copilotTokens":{"https://github.com:legacy":"fixture-legacy-token"}})).unwrap();
        let path = path_at(&dir, &saved.summary.id).unwrap();
        super::super::cursor_auth::write(&path, &saved).unwrap();
        let original = fs::read(&path).unwrap();
        assert_eq!(list_at(&dir).unwrap().len(), 1);
        assert_eq!(
            read_saved_at(&dir, &saved.summary.id).unwrap().access_token,
            "fixture-legacy-token"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        saved.access_token = "enc:v1:bad".into();
        super::super::cursor_auth::write(&path, &saved).unwrap();
        let broken = fs::read(&path).unwrap();
        assert_eq!(list_at(&dir).unwrap().len(), 1);
        assert!(read_saved_at(&dir, &saved.summary.id).is_err());
        assert_eq!(fs::read(&path).unwrap(), broken);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn validates_github_hosts_and_refuses_unrelated_endpoints() {
        assert_eq!(validated_host("https://github.com").unwrap(), "github.com");
        assert!(validated_host("github.com.attacker.invalid").is_err());
        assert!(valid_api_url("https://api.githubcopilot.com").is_ok());
        assert!(valid_api_url("https://api.githubcopilot.com.attacker.invalid").is_err());
        assert!(valid_api_url("https://api.githubcopilot.com@attacker.invalid").is_err());
    }
}
