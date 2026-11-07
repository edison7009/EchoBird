//! Native account capabilities are supplied by installed, enabled code packages.
use super::plugin_manager::{InstalledPlugin, PluginManager};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
pub mod bindings;
pub mod stream;

fn logins() -> &'static Mutex<HashMap<String, String>> {
    static LOGINS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    LOGINS.get_or_init(Mutex::default)
}
pub fn login_application(id: &str) -> Result<String, String> {
    logins()
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or("accountError.cancelled".into())
}

struct Worker {
    application: String,
    directory: String,
    manifest: super::plugin_manager::manifest::Manifest,
    port: u16,
    secret: String,
    cancelled: CancellationToken,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.cancel();
    }
}

fn workers() -> &'static Mutex<HashMap<String, Arc<Worker>>> {
    static WORKERS: OnceLock<Mutex<HashMap<String, Arc<Worker>>>> = OnceLock::new();
    WORKERS.get_or_init(Mutex::default)
}

fn starting() -> &'static Mutex<HashMap<String, Arc<CancellationToken>>> {
    static STARTING: OnceLock<Mutex<HashMap<String, Arc<CancellationToken>>>> = OnceLock::new();
    STARTING.get_or_init(Mutex::default)
}
struct Starting {
    id: String,
    cancelled: Arc<CancellationToken>,
}
impl Starting {
    fn new(id: &str, token: CancellationToken) -> Self {
        let cancelled = Arc::new(token);
        starting()
            .lock()
            .unwrap()
            .insert(id.into(), cancelled.clone());
        Self {
            id: id.into(),
            cancelled,
        }
    }
}
impl Drop for Starting {
    fn drop(&mut self) {
        let mut starts = starting().lock().unwrap();
        if starts
            .get(&self.id)
            .is_some_and(|token| Arc::ptr_eq(token, &self.cancelled))
        {
            starts.remove(&self.id);
            self.cancelled.cancel();
        }
    }
}
pub fn stop(id: &str) {
    let mut starts = starting().lock().unwrap();
    if let Some(token) = starts.remove(id) {
        token.cancel();
    }
    if let Some(worker) = workers().lock().unwrap().remove(id) {
        logins()
            .lock()
            .unwrap()
            .retain(|_, application| application != &worker.application);
        worker.cancelled.cancel();
    }
}
pub fn stop_all() {
    let mut starts = starting().lock().unwrap();
    for (_, token) in starts.drain() {
        token.cancel();
    }
    logins().lock().unwrap().clear();
    for (_, worker) in workers().lock().unwrap().drain() {
        worker.cancelled.cancel();
    }
}

async fn handshake(
    output: &mut (impl tokio::io::AsyncBufRead + Unpin),
    cancelled: &CancellationToken,
) -> Result<Value, String> {
    let mut line = String::new();
    let mut limited = output.take(4097);
    tokio::select! {
        biased;
        _ = cancelled.cancelled() => return Err("Account extension stopped during startup".into()),
        result = tokio::time::timeout(Duration::from_secs(20), limited.read_line(&mut line)) => {
            result.map_err(|_| "Account extension startup timed out")?.map_err(|_| "Account extension handshake failed")?;
        }
    }
    if line.len() > 4096 {
        return Err("Account extension handshake exceeds limit".into());
    }
    serde_json::from_str(&line).map_err(|_| "Invalid account extension handshake".into())
}

/// Native modules and application packages use the persistent account protocol.
fn runs_service(manifest: &super::plugin_manager::manifest::Manifest) -> bool {
    manifest.echobird.worker.as_ref().is_some_and(|worker| {
        worker.runtime.as_deref() == Some("native") || !manifest.tools().is_empty()
    })
}

/// Node cannot start a script from a Windows verbatim path, which canonicalize returns.
fn node_entry(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(plain) if !plain.starts_with("UNC\\") => plain.into(),
        _ => path.into(),
    }
}

/// Catalog directories of enabled plugin applications, as `(id, directory)`.
pub fn tool_directories() -> Vec<(String, std::path::PathBuf)> {
    if super::developer_preview::token().is_err() {
        return Vec::new();
    }
    let manager = PluginManager::default();
    manager
        .list()
        .unwrap_or_default()
        .iter()
        .filter(|plugin| plugin.enabled)
        .flat_map(|plugin| {
            let package = manager
                .package_root(plugin)
                .and_then(|root| root.canonicalize().map_err(|e| e.to_string()))
                .ok();
            plugin.manifest.tools().into_iter().filter_map(move |tool| {
                let package = package.clone()?;
                let directory = package.join(&tool.directory).canonicalize().ok()?;
                directory
                    .starts_with(&package)
                    .then_some((tool.id, directory))
            })
        })
        .collect()
}

pub fn provides_tool(application: &str) -> bool {
    tool_directories().iter().any(|(id, _)| id == application)
}

/// Install references of enabled plugin applications, as `(id, json)`.
pub fn tool_install_references() -> Vec<(String, String)> {
    tool_directories()
        .into_iter()
        .filter_map(|(id, directory)| {
            let reference = std::fs::read_to_string(directory.join("install.json")).ok()?;
            serde_json::from_str::<Value>(&reference).ok()?;
            Some((id, reference))
        })
        .collect()
}

fn contribution(application: &str) -> Result<Option<InstalledPlugin>, String> {
    let mut installed = PluginManager::default()
        .list()?
        .into_iter()
        .filter(|plugin| {
            plugin.enabled
                && runs_service(&plugin.manifest)
                && plugin
                    .manifest
                    .echobird
                    .contributes
                    .providers
                    .iter()
                    .any(|provider| provider.applications.iter().any(|id| id == application))
        });
    let plugin = installed.next();
    if installed.next().is_some() {
        return Err("Multiple enabled account plugins support this application".into());
    }
    Ok(plugin)
}

pub fn available(application: &str) -> bool {
    super::developer_preview::token().is_ok()
        && contribution(application).is_ok_and(|plugin| plugin.is_some())
}

fn allows(plugin: &InstalledPlugin, application: &str, operation: &str) -> bool {
    let operation = match operation {
        "accounts.saved" | "accounts.options" => "accounts",
        "connection" | "metadata" => "models",
        operation => operation,
    };
    if matches!(operation, "model.read" | "model.apply" | "model.restore") {
        return plugin.manifest.tools().iter().any(|t| t.id == application);
    }
    plugin
        .manifest
        .echobird
        .contributes
        .providers
        .iter()
        .any(|provider| {
            provider.applications.iter().any(|id| id == application)
                && provider
                    .operations
                    .iter()
                    .any(|declared| declared == operation)
        })
}

pub fn application(provider: crate::models::model::AccountProvider) -> Option<&'static str> {
    use crate::models::model::AccountProvider::*;
    match provider {
        ClaudeCode => Some("claudecode"),
        WorkBuddy => Some("workbuddy"),
        WorkBuddyAi => Some("workbuddyai"),
        DoubaoWork => Some("doubaowork"),
        Dola => Some("dola"),
        Trae => Some("trae"),
        Traecn => Some("traecn"),
        Plugin(application) => Some(application.0),
        _ => None,
    }
}

async fn worker(plugin: &InstalledPlugin, application: &str) -> Result<Arc<Worker>, String> {
    let generation = super::developer_preview::token()?;
    static START: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _start = START.lock().await;
    if generation.is_cancelled() {
        return Err("Developer preview is paused".into());
    }
    if let Some(worker) = workers()
        .lock()
        .unwrap()
        .get(&plugin.id)
        .filter(|worker| {
            // An application package serves all its tools from one process.
            (worker.application == application || !plugin.manifest.tools().is_empty())
                && worker.directory == plugin.directory
                && worker.manifest == plugin.manifest
                && !worker.cancelled.is_cancelled()
        })
        .cloned()
    {
        return Ok(worker);
    }
    stop(&plugin.id);
    let startup = Starting::new(&plugin.id, generation.child_token());
    let current = contribution(application)?.ok_or("Account plugin was removed before startup")?;
    if current.directory != plugin.directory || current.manifest != plugin.manifest {
        return Err("Account plugin changed before startup".into());
    }
    let manager = PluginManager::default();
    let root = manager.package_root(plugin)?;
    let manifest = super::plugin_manager::manifest::Manifest::read(&root)?;
    if manifest != plugin.manifest {
        return Err("Installed account plugin changed".into());
    }
    let entry = &manifest
        .echobird
        .worker
        .as_ref()
        .ok_or("Account plugin has no code module")?
        .entry;
    let executable = root
        .join(entry)
        .canonicalize()
        .map_err(|_| "Missing account code module")?;
    if !executable.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
        return Err("Account module escapes its package".into());
    }
    let native = manifest
        .echobird
        .worker
        .as_ref()
        .is_some_and(|worker| worker.runtime.as_deref() == Some("native"));
    #[cfg(unix)]
    if native {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let mut tools = serde_json::Map::new();
    for id in manifest
        .echobird
        .contributes
        .providers
        .iter()
        .flat_map(|provider| provider.applications.iter())
    {
        let uri = super::tool_manager::get_tool_launch_uri(id);
        #[cfg(windows)]
        let roaming = uri
            .as_ref()
            .and_then(|uri| super::msix::roaming_data_dir(uri, "Claude"));
        #[cfg(not(windows))]
        let roaming: Option<std::path::PathBuf> = None;
        tools.insert(id.clone(),json!({"executable":super::tool_manager::get_tool_exe_path(id),"launchUri":uri,"roamingDataDir":roaming,"metadata":super::tool_manager::doubao_desktop_metadata(id)}));
    }
    let secret = format!(
        "Bearer {}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let mut command = if native {
        crate::utils::process::async_command(executable)
    } else {
        let node =
            which::which("node").map_err(|_| "Node.js is required for this application plugin")?;
        let mut command = crate::utils::process::async_command(node);
        command.arg(node_entry(&executable));
        command
    };
    command.current_dir(&root).env_clear();
    for key in [
        "SystemRoot",
        "WINDIR",
        "PATH",
        "PATHEXT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "HOME",
        "USERPROFILE",
        "USERNAME",
        "USER",
        "APPDATA",
        "LOCALAPPDATA",
        "COMPUTERNAME",
        "HOSTNAME",
        "CLAUDE_CONFIG_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    if startup.cancelled.is_cancelled() {
        return Err("Account extension stopped during startup".into());
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Could not start account extension")?;
    #[cfg(windows)]
    let tree = crate::utils::process::owned_process_tree(&child)?;
    let mut input = child
        .stdin
        .take()
        .ok_or("Account extension input unavailable")?;
    let output = child
        .stdout
        .take()
        .ok_or("Account extension output unavailable")?;
    let config = json!({"provider":application,"secret":secret,"tools":tools}).to_string() + "\n";
    input
        .write_all(config.as_bytes())
        .await
        .map_err(|_| "Account extension initialization failed")?;
    let mut output = BufReader::new(output);
    let ready = handshake(&mut output, &startup.cancelled).await?;
    let port = ready["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
        .ok_or("Invalid account extension port")?;
    if ready["protocol"] != 1 {
        return Err("Unsupported account extension protocol".into());
    }
    let _gate = super::developer_preview::enter()?;
    let mut starts = starting().lock().unwrap();
    if generation.is_cancelled() || startup.cancelled.is_cancelled() {
        return Err("Account extension stopped during startup".into());
    }
    let current = contribution(application)?.ok_or("Account plugin was removed during startup")?;
    if current.directory != plugin.directory || current.manifest != plugin.manifest {
        return Err("Account plugin changed during startup".into());
    }
    let cancelled = (*startup.cancelled).clone();
    let cleanup = cancelled.clone();
    tokio::spawn(async move {
        let _input = input;
        let _output = output;
        tokio::select! {_ =cleanup.cancelled()=>{},_ = child.wait()=>{cleanup.cancel();}}
        #[cfg(windows)]
        drop(tree);
        #[cfg(unix)]
        if let Some(id) = child.id() {
            unsafe {
                libc::kill(-(id as i32), libc::SIGKILL);
            }
        }
        let _ = child.kill().await;
        let _ = child.wait().await;
    });
    let worker = Arc::new(Worker {
        application: application.into(),
        directory: plugin.directory.clone(),
        manifest,
        port,
        secret,
        cancelled,
    });
    workers()
        .lock()
        .unwrap()
        .insert(plugin.id.clone(), worker.clone());
    starts.remove(&plugin.id);
    drop(starts);
    Ok(worker)
}

pub async fn request(
    application: &str,
    operation: &str,
    params: Value,
) -> Result<reqwest::Response, String> {
    let generation = super::developer_preview::token()?;
    let plugin = contribution(application)?.ok_or("accountError.pluginRequired")?;
    if !allows(&plugin, application, operation) {
        return Err("Account operation is not declared by this plugin".into());
    }
    let worker = worker(&plugin, application).await?;
    let client = super::http_transport::local_worker()?;
    tokio::select! {
        biased;
        _=generation.cancelled()=>Err("Developer preview is paused".into()),
        _=worker.cancelled.cancelled()=>Err("Account extension stopped".into()),
        result=client.post(format!("http://127.0.0.1:{}/operation",worker.port)).header("authorization",&worker.secret).json(&json!({"operation":operation,"params":params})).send()=>result.map_err(|_|"Account extension request failed".into())
    }
}

pub async fn call(application: &str, operation: &str, params: Value) -> Result<Value, String> {
    super::developer_preview::run(call_inner(application, operation, params)).await
}

async fn call_inner(application: &str, operation: &str, params: Value) -> Result<Value, String> {
    let mut response = request(application, operation, params).await?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Account extension response failed")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Account extension response exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid account extension result")?;
    if !status.is_success() {
        let message = value["error"]["message"]
            .as_str()
            .unwrap_or("accountError.failed");
        return Err(if message.starts_with("accountError.") {
            message.to_owned()
        } else {
            "accountError.failed".into()
        });
    }
    Ok(value)
}

pub async fn list<T: serde::de::DeserializeOwned>(
    application: &str,
    saved: bool,
) -> Result<Vec<T>, String> {
    if !available(application) {
        return Ok(Vec::new());
    }
    serde_json::from_value(
        call(
            application,
            if saved { "accounts.saved" } else { "accounts" },
            json!({}),
        )
        .await?,
    )
    .map_err(|_| "Invalid account extension list".into())
}

pub async fn prepare_history(application: &str, only_if_stopped: bool) -> Result<(), String> {
    // Model configuration may be written while the desktop is still running.
    // Defer history preparation until launch has stopped it; account switching
    // retains its separate requirement for a graceful, manual exit.
    if available(application)
        && only_if_stopped
        && super::process_manager::desktop_tool_is_running(application).await?
    {
        return Ok(());
    }
    if available(application) {
        call(
            application,
            "history.merge",
            json!({"onlyIfStopped":only_if_stopped}),
        )
        .await?;
    }
    Ok(())
}

pub async fn proxy(
    application: &str,
    account_id: &str,
    model: &str,
    body: Value,
) -> Result<axum::response::Response, super::account_proxy::ProxyError> {
    use super::account_proxy::{ProxyError, ProxyFailureMetadata};
    use axum::{
        body::Body,
        http::{header, StatusCode},
    };
    let response = request(
        application,
        "proxy",
        json!({"accountId":account_id,"model":model,"body":body}),
    )
    .await
    .map_err(super::account_model_sources::credential_error)?;
    let status = response.status();
    let headers = response.headers().clone();
    if !status.is_success() {
        let value = response.json::<Value>().await.unwrap_or_default();
        let mut error = ProxyError::new(
            status,
            value["error"]["message"]
                .as_str()
                .unwrap_or("Account extension request failed"),
        );
        error.error_code = value["error"]["code"].as_str().map(str::to_owned);
        error.error_type = value["error"]["type"].as_str().map(str::to_owned);
        error.retry_after = headers.get(header::RETRY_AFTER).cloned();
        error.model_scope = headers
            .get("x-echobird-model-scope")
            .is_some_and(|value| value == "1");
        return Err(error);
    }
    let mut output = axum::response::Response::new(Body::from_stream(response.bytes_stream()));
    *output.status_mut() = status;
    for key in [header::CONTENT_TYPE, header::CACHE_CONTROL] {
        if let Some(value) = headers.get(&key) {
            output.headers_mut().insert(key, value.clone());
        }
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        if let Some(value) = headers.get(header::RETRY_AFTER) {
            output
                .headers_mut()
                .insert(header::RETRY_AFTER, value.clone());
        }
    }
    output
        .extensions_mut()
        .insert(ProxyFailureMetadata { model_scope: false });
    Ok(output)
}

#[cfg(test)]
mod provider_tests {
    use super::{application, node_entry};
    use crate::models::model::AccountProvider;
    use std::path::Path;

    #[test]
    fn node_entries_drop_only_the_drive_verbatim_prefix() {
        assert_eq!(
            node_entry(Path::new(r"\\?\C:\plugin\service.mjs")),
            Path::new(r"C:\plugin\service.mjs")
        );
        assert_eq!(
            node_entry(Path::new(r"\\?\UNC\host\share\service.mjs")),
            Path::new(r"\\?\UNC\host\share\service.mjs")
        );
        assert_eq!(
            node_entry(Path::new("/plugin/service.mjs")),
            Path::new("/plugin/service.mjs")
        );
    }

    #[test]
    fn regional_account_modules_keep_distinct_application_ids() {
        assert_eq!(application(AccountProvider::Trae), Some("trae"));
        assert_eq!(application(AccountProvider::Traecn), Some("traecn"));
        assert_eq!(application(AccountProvider::Codex), None);
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    #[tokio::test]
    async fn readiness_positive_control_and_revocation_during_partial_handshake() {
        let token = CancellationToken::new();
        let ready = handshake(
            &mut BufReader::new(&b"{\"protocol\":1,\"port\":12345}\n"[..]),
            &token,
        )
        .await
        .unwrap();
        assert_eq!(ready["port"], 12345);
        let id = format!("fixture-start-{}", uuid::Uuid::new_v4());
        let startup = Starting::new(&id, CancellationToken::new());
        let (mut writer, reader) = tokio::io::duplex(128);
        writer.write_all(b"{\"protocol\":").await.unwrap();
        let mut output = BufReader::new(reader);
        let pending = handshake(&mut output, &startup.cancelled);
        tokio::pin!(pending);
        tokio::select! {result=&mut pending=>panic!("Incomplete readiness accepted: {result:?}"),_=tokio::time::sleep(Duration::from_millis(10))=>{}}
        stop(&id);
        assert!(tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap_err()
            .contains("stopped"));
        assert!(!starting().lock().unwrap().contains_key(&id));
    }
    #[tokio::test]
    async fn failed_start_guard_does_not_revoke_a_replacement_and_readiness_is_bounded() {
        let id = format!("fixture-start-{}", uuid::Uuid::new_v4());
        let old = Starting::new(&id, CancellationToken::new());
        let replacement = Starting::new(&id, CancellationToken::new());
        drop(old);
        assert!(!replacement.cancelled.is_cancelled());
        let token = CancellationToken::new();
        let huge = vec![b'x'; 4097];
        assert!(handshake(&mut BufReader::new(huge.as_slice()), &token)
            .await
            .unwrap_err()
            .contains("limit"));
        drop(replacement);
        assert!(!starting().lock().unwrap().contains_key(&id));
    }
}
