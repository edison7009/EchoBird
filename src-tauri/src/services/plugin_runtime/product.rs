use super::*;
use crate::services::plugin_manager::{manifest::Manifest, product::Theme};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use tauri::Emitter;
use tokio::sync::{oneshot, Notify};

pub(super) const CHANGED: &str = "plugin-sdk-changed";
pub(super) struct ProductSession {
    pub manifest: Manifest,
    pub background: bool,
    ready: AtomicBool,
    ready_event: Notify,
    subscriptions: Mutex<HashSet<String>>,
    busy: AtomicBool,
}
impl ProductSession {
    pub(super) fn new(manifest: Manifest, background: bool) -> Self {
        Self {
            manifest,
            background,
            ready: AtomicBool::new(false),
            ready_event: Notify::new(),
            subscriptions: Mutex::new(HashSet::new()),
            busy: AtomicBool::new(false),
        }
    }
    fn permits(&self, permission: &str) -> Result<(), String> {
        self.manifest
            .echobird
            .permissions
            .iter()
            .any(|p| p == permission)
            .then_some(())
            .ok_or_else(|| "Plugin permission denied".into())
    }
}
pub(super) struct Pending {
    label: String,
    sender: oneshot::Sender<Result<Value, String>>,
}
struct RequestGuard<'a> {
    runtime: &'a PluginRuntime,
    app: &'a tauri::AppHandle,
    label: String,
    id: String,
    session: Arc<Session>,
}
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        self.runtime.pending.lock().unwrap().remove(&self.id);
        if let Some(product) = &self.session.product {
            product.busy.store(false, Ordering::Release);
        }
        if let Some(window) = self.app.get_webview_window(&self.label) {
            let _ = window.eval(format!(
                "window.__echobirdCancel?.({})",
                serde_json::to_string(&self.id).unwrap()
            ));
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub plugin_id: String,
    pub id: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    #[serde(default)]
    theme: Option<Selection>,
    #[serde(default)]
    processors: Vec<Selection>,
}
#[derive(Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    configuration: BTreeMap<String, Value>,
    #[serde(default)]
    storage: BTreeMap<String, Value>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductState {
    pub theme: Option<Selection>,
    pub palette: Option<Theme>,
    pub processors: Vec<Selection>,
    pub configuration: BTreeMap<String, BTreeMap<String, Value>>,
    pub errors: BTreeMap<String, String>,
}

fn data_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(Mutex::default)
}
fn file(manager: &PluginManager, id: &str) -> Result<PathBuf, String> {
    if id != "host"
        && !id.split_once('.').is_some_and(|(p, n)| {
            super::super::plugin_manager::manifest::identifier(p)
                && super::super::plugin_manager::manifest::identifier(n)
        })
    {
        return Err("Invalid plugin data identity".into());
    }
    let root = if manager.root.exists() {
        manager.root.canonicalize().map_err(|e| e.to_string())?
    } else {
        manager.root.clone()
    };
    let directory = root.join("data");
    if directory.exists()
        && !directory
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&root)
    {
        return Err("Plugin data directory escapes storage".into());
    }
    let path = directory.join(format!("{id}.json"));
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Plugin data cannot be a link".into());
    }
    Ok(path)
}
fn read<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("Plugin data exceeds 1 MiB".into());
    }
    crate::services::cursor_auth::read(path)
}
fn write<T: Serialize>(path: &std::path::Path, value: &T) -> Result<(), String> {
    if serde_json::to_vec_pretty(value)
        .map_err(|e| e.to_string())?
        .len()
        > 1024 * 1024
    {
        return Err("Plugin data exceeds 1 MiB".into());
    }
    let directory = path.parent().ok_or("Invalid plugin data path")?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let root = directory
        .parent()
        .ok_or("Invalid plugin data root")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !directory
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(root)
        || std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Plugin data cannot escape storage".into());
    }
    crate::services::cursor_auth::write(path, value)
}
fn configuration(
    manager: &PluginManager,
    manifest: &Manifest,
) -> Result<BTreeMap<String, Value>, String> {
    let data: Data = read(&file(manager, &manifest.id())?)?;
    Ok(manifest
        .echobird
        .contributes
        .configuration
        .iter()
        .map(|field| {
            (
                field.key.clone(),
                data.configuration
                    .get(&field.key)
                    .filter(|v| field.accepts(v))
                    .unwrap_or(&field.default)
                    .clone(),
            )
        })
        .collect())
}
fn update_configuration(
    manager: &PluginManager,
    manifest: &Manifest,
    key: &str,
    value: Option<Value>,
) -> Result<(), String> {
    let field = manifest
        .echobird
        .contributes
        .configuration
        .iter()
        .find(|field| field.key == key)
        .ok_or("Unknown plugin setting")?;
    let path = file(manager, &manifest.id())?;
    let mut data: Data = read(&path)?;
    if let Some(value) = value {
        if !field.accepts(&value) {
            return Err("Invalid plugin setting value".into());
        }
        data.configuration.insert(key.into(), value);
    } else {
        data.configuration.remove(key);
    }
    write(&path, &data)
}

impl PluginRuntime {
    fn session(&self, label: &str) -> Result<Arc<Session>, String> {
        self.sessions
            .lock()
            .unwrap()
            .get(label)
            .filter(|s| !s.cancelled.is_cancelled())
            .cloned()
            .ok_or_else(|| "Plugin session is closed".into())
    }
    fn background_label(&self, id: &str) -> Option<String> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .find(|(_, s)| {
                s.plugin_id == id
                    && !s.cancelled.is_cancelled()
                    && s.product.as_ref().is_some_and(|p| p.background)
            })
            .map(|(label, _)| label.clone())
    }
    pub fn product_state(
        &self,
        app: &tauri::AppHandle,
        manager: &PluginManager,
    ) -> Result<ProductState, String> {
        let plugins = manager.list()?;
        let mut state = ProductState {
            theme: None,
            palette: None,
            processors: Vec::new(),
            configuration: BTreeMap::new(),
            errors: BTreeMap::new(),
        };
        let stale: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| {
                s.product.as_ref().is_some_and(|p| p.background)
                    && !plugins.iter().any(|p| {
                        p.enabled
                            && p.id == s.plugin_id
                            && p.manifest == s.product.as_ref().unwrap().manifest
                    })
            })
            .map(|s| s.plugin_id.clone())
            .collect();
        for id in stale {
            self.revoke(app, &id);
        }
        for plugin in plugins.iter().filter(|p| p.enabled) {
            let sync = self.product_sync.lock().unwrap();
            if plugin.manifest.echobird.background.is_some()
                && self.background_label(&plugin.id).is_none()
            {
                if let Err(error) = self.launch(app, manager, plugin.clone(), "@background", None) {
                    state.errors.insert(plugin.id.clone(), error);
                }
            }
            drop(sync);
            if !plugin
                .manifest
                .echobird
                .contributes
                .configuration
                .is_empty()
            {
                let _lock = data_lock().lock().unwrap();
                match configuration(manager, &plugin.manifest) {
                    Ok(values) => {
                        state.configuration.insert(plugin.id.clone(), values);
                    }
                    Err(error) => {
                        state.errors.insert(plugin.id.clone(), error);
                    }
                }
            }
        }
        let _lock = data_lock().lock().unwrap();
        let preferences: Preferences = read(&file(manager, "host")?)?;
        state.processors = preferences.processors;
        if let Some(selected) = preferences.theme {
            state.palette = plugins
                .iter()
                .filter(|p| p.enabled && p.id == selected.plugin_id)
                .find_map(|p| {
                    p.manifest
                        .echobird
                        .contributes
                        .themes
                        .iter()
                        .find(|t| t.id == selected.id)
                })
                .cloned();
            state.theme = Some(selected);
        }
        Ok(state)
    }
    pub fn manage_product(
        &self,
        app: &tauri::AppHandle,
        manager: &PluginManager,
        id: Option<&str>,
        action: &str,
        key: &str,
        value: Value,
    ) -> Result<(), String> {
        let _lock = data_lock().lock().unwrap();
        let plugins = manager.list()?;
        let plugin = id.and_then(|id| plugins.iter().find(|p| p.id == id && p.enabled));
        if action == "configuration" || action == "configuration.reset" {
            let plugin = plugin.ok_or("Plugin is disabled or missing")?;
            update_configuration(
                manager,
                &plugin.manifest,
                key,
                (action == "configuration").then_some(value),
            )?;
            self.product_event(
                app,
                "echo.configuration.changed",
                json!({"pluginId":plugin.id,"key":key}),
            );
        } else {
            let path = file(manager, "host")?;
            let mut preferences: Preferences = read(&path)?;
            let selected = plugin.map(|p| Selection {
                plugin_id: p.id.clone(),
                id: key.into(),
            });
            match action {
                "theme" => {
                    if id.is_some()
                        && !plugin.is_some_and(|p| {
                            p.manifest
                                .echobird
                                .contributes
                                .themes
                                .iter()
                                .any(|t| t.id == key)
                        })
                    {
                        return Err("Unknown plugin theme".into());
                    }
                    preferences.theme = selected;
                }
                "processor" => {
                    let selected = selected.ok_or("Plugin is disabled or missing")?;
                    if !plugin
                        .unwrap()
                        .manifest
                        .echobird
                        .contributes
                        .text_processors
                        .iter()
                        .any(|p| p.id == key)
                    {
                        return Err("Unknown text processor".into());
                    }
                    preferences.processors.retain(|p| p != &selected);
                    if value == true {
                        preferences.processors.push(selected);
                    }
                }
                _ => return Err("Unknown plugin management action".into()),
            }
            write(&path, &preferences)?;
            if action == "theme" {
                self.product_event(app, "echo.theme.changed", json!({}));
            }
        }
        app.emit(CHANGED, ()).map_err(|e| e.to_string())
    }
    pub async fn host_call(
        &self,
        app: &tauri::AppHandle,
        manager: &PluginManager,
        label: &str,
        operation: &str,
        input: Value,
    ) -> Result<Value, String> {
        let session = self.session(label)?;
        let product = session.product.as_ref().ok_or("SDK session unavailable")?;
        if serde_json::to_vec(&input).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("SDK input exceeds 1 MiB".into());
        }
        let key = input["key"].as_str().unwrap_or_default();
        match operation {
            "ready" => {
                product.ready.store(true, Ordering::Release);
                product.ready_event.notify_waiters();
                Ok(Value::Null)
            }
            "configuration.get" => {
                product.permits("configuration")?;
                let _lock = data_lock().lock().unwrap();
                let values = configuration(manager, &product.manifest)?;
                if key.is_empty() {
                    Ok(json!(values))
                } else {
                    values
                        .get(key)
                        .cloned()
                        .ok_or("Unknown plugin setting".into())
                }
            }
            "configuration.update" | "configuration.reset" => {
                product.permits("configuration")?;
                {
                    let _lock = data_lock().lock().unwrap();
                    update_configuration(
                        manager,
                        &product.manifest,
                        key,
                        (operation == "configuration.update").then(|| input["value"].clone()),
                    )?;
                }
                self.product_event(
                    app,
                    "echo.configuration.changed",
                    json!({"pluginId":session.plugin_id,"key":key}),
                );
                app.emit(CHANGED, ()).map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "storage.get" | "storage.set" | "storage.delete" => {
                product.permits("storage")?;
                if key.is_empty() || key.len() > 128 {
                    return Err("Invalid storage key".into());
                }
                let _lock = data_lock().lock().unwrap();
                let path = file(manager, &session.plugin_id)?;
                let mut data: Data = read(&path)?;
                if operation == "storage.get" {
                    return Ok(data.storage.get(key).cloned().unwrap_or(Value::Null));
                }
                if operation == "storage.set" {
                    data.storage.insert(key.into(), input["value"].clone());
                } else {
                    data.storage.remove(key);
                }
                write(&path, &data)?;
                Ok(Value::Null)
            }
            "events.subscribe" | "events.unsubscribe" => {
                product.permits("events")?;
                if !valid_topic(key) {
                    return Err("Invalid event topic".into());
                }
                let mut topics = product.subscriptions.lock().unwrap();
                if operation == "events.subscribe" {
                    if topics.len() >= 32 && !topics.contains(key) {
                        return Err("Too many event subscriptions".into());
                    }
                    topics.insert(key.into());
                } else {
                    topics.remove(key);
                }
                Ok(Value::Null)
            }
            "events.emit" => {
                product.permits("events")?;
                if !key.starts_with(&format!("{}.", session.plugin_id)) || !valid_topic(key) {
                    return Err("Plugins can emit only their own event topics".into());
                }
                self.product_event(app, key, input["value"].clone());
                Ok(Value::Null)
            }
            "commands.execute" => {
                product.permits("commands")?;
                tokio::select! {
                    _ = session.cancelled.cancelled() => Err("Plugin session is closed".into()),
                    result = self.execute_product(
                    app,
                    manager,
                    &session.plugin_id,
                    key,
                    input["input"].clone(),
                    ) => result,
                }
            }
            _ => Err("Unknown SDK operation".into()),
        }
    }
    pub fn product_event(&self, app: &tauri::AppHandle, topic: &str, value: Value) {
        let labels: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, s)| {
                !s.cancelled.is_cancelled()
                    && s.product
                        .as_ref()
                        .is_some_and(|p| p.subscriptions.lock().unwrap().contains(topic))
            })
            .map(|(l, _)| l.clone())
            .collect();
        let event = json!({"topic":topic,"value":value});
        for label in labels {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.eval(format!("window.__echobirdEvent?.({event})"));
            }
        }
    }
    pub fn product_result(
        &self,
        label: &str,
        id: &str,
        value: Value,
        error: Option<String>,
    ) -> Result<(), String> {
        self.session(label)?;
        if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("SDK result exceeds 1 MiB".into());
        }
        let mut pending = self.pending.lock().unwrap();
        if pending.get(id).is_none_or(|p| p.label != label) {
            return Err("Unknown SDK request".into());
        }
        let request = pending.remove(id).unwrap();
        let _ = request.sender.send(match error {
            Some(_) => Err("Plugin handler failed".into()),
            None => Ok(value),
        });
        Ok(())
    }
    async fn product_invoke(
        &self,
        app: &tauri::AppHandle,
        manager: &PluginManager,
        plugin: InstalledPlugin,
        kind: &str,
        id: &str,
        input: Value,
    ) -> Result<Value, String> {
        let generation = super::super::developer_preview::token()?;
        {
            let _sync = self.product_sync.lock().unwrap();
            if self.background_label(&plugin.id).is_none() {
                self.launch(app, manager, plugin.clone(), "@background", None)?;
            }
        }
        let label = self
            .background_label(&plugin.id)
            .ok_or("Extension did not start")?;
        let session = self.session(&label)?;
        let product = session.product.as_ref().ok_or("SDK session unavailable")?;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let notified = product.ready_event.notified();
                if product.ready.load(Ordering::Acquire) {
                    break;
                }
                tokio::select! {
                    _ = generation.cancelled() => return Err("Developer preview is paused".into()),
                    _ = session.cancelled.cancelled() => return Err("Extension stopped".into()),
                    _ = notified => {}
                }
            }
            if product.busy.swap(true, Ordering::AcqRel) {
                return Err("Extension is busy".into());
            }
            let request_id = uuid::Uuid::new_v4().to_string();
            let guard = RequestGuard {
                runtime: self,
                app,
                label: label.clone(),
                id: request_id.clone(),
                session: session.clone(),
            };
            let (sender, receiver) = oneshot::channel();
            self.pending.lock().unwrap().insert(
                request_id.clone(),
                Pending {
                    label: label.clone(),
                    sender,
                },
            );
            let request = json!({"requestId":request_id,"kind":kind,"id":id,"input":input});
            app.get_webview_window(&label)
                .ok_or("Extension window is closed")?
                .eval(format!("window.__echobirdDispatch?.({request})"))
                .map_err(|e| e.to_string())?;
            let result = tokio::select! {
                _ = generation.cancelled() => Err("Developer preview is paused".into()),
                _ = session.cancelled.cancelled() => Err("Extension stopped".into()),
                result = receiver => result.map_err(|_| "Extension response cancelled".to_owned())?
            };
            drop(guard);
            result
        })
        .await
        .map_err(|_| "Extension request timed out".to_owned())?
    }

    pub async fn execute_product(
        &self,
        app: &tauri::AppHandle,
        manager: &PluginManager,
        plugin_id: &str,
        id: &str,
        input: Value,
    ) -> Result<Value, String> {
        let plugin = manager
            .list()?
            .into_iter()
            .find(|p| p.enabled && p.id == plugin_id)
            .ok_or("Plugin is disabled or missing")?;
        let command = plugin
            .manifest
            .echobird
            .contributes
            .commands
            .iter()
            .find(|c| c.id == id)
            .ok_or("Unknown plugin command")?;
        if let Some(page) = &command.page_id {
            self.launch(app, manager, plugin.clone(), page, None)?;
            return Ok(Value::Null);
        }
        if serde_json::to_vec(&input).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("Command input exceeds 1 MiB".into());
        }
        self.product_invoke(app, manager, plugin, "command", id, input)
            .await
    }
    pub async fn process_text(
        &self,
        app: &tauri::AppHandle,
        stage: &str,
        text: &str,
    ) -> Result<String, String> {
        if super::super::developer_preview::token().is_err() {
            return Ok(text.into());
        }
        let manager = app.state::<PluginManager>();
        let selections = {
            let _lock = data_lock().lock().unwrap();
            let path = file(&manager, "host")?;
            read::<Preferences>(&path)?.processors
        };
        let plugins = manager.list()?;
        let mut result = text.to_owned();
        for selected in selections {
            let Some(plugin) = plugins.iter().find(|p| {
                p.enabled
                    && p.id == selected.plugin_id
                    && p.manifest
                        .echobird
                        .contributes
                        .text_processors
                        .iter()
                        .any(|hook| hook.id == selected.id && hook.stage == stage)
            }) else {
                continue;
            };
            let value = self
                .product_invoke(
                    app,
                    &manager,
                    plugin.clone(),
                    "text",
                    &selected.id,
                    json!({"text":result,"stage":stage}),
                )
                .await?;
            let next = value.as_str().ok_or("Text processor must return text")?;
            if next.len() > 1024 * 1024 {
                return Err("Processed text exceeds 1 MiB".into());
            }
            result = next.into();
        }
        Ok(result)
    }
}
fn valid_topic(topic: &str) -> bool {
    topic.len() <= 200
        && topic
            .split('.')
            .all(super::super::plugin_manager::manifest::identifier)
}

#[cfg(test)]
mod tests;
