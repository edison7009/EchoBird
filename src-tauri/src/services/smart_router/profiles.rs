//! Independently configured loopback routers. The original ID and file stay stable.
use super::*;
use std::collections::{HashMap, HashSet};

tokio::task_local! { static CURRENT: String; }
#[cfg(test)]
tokio::task_local! { static FIXTURE_ROOT: PathBuf; }

#[cfg(test)]
pub(super) fn with_fixture_root<T>(root: PathBuf, run: impl FnOnce() -> T) -> T {
    FIXTURE_ROOT.sync_scope(root, run)
}
static RUNNING_GROUPS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    enabled: bool,
    ids: Vec<String>,
}

pub(crate) fn current_id() -> String {
    CURRENT
        .try_with(Clone::clone)
        .unwrap_or_else(|_| SMART_ROUTER_INTERNAL_ID.into())
}
pub(super) fn root() -> PathBuf {
    #[cfg(test)]
    if let Ok(root) = FIXTURE_ROOT.try_with(Clone::clone) {
        return root;
    }
    echobird_dir()
}
pub(crate) fn path_for(id: &str) -> PathBuf {
    root().join("config").join(format!("{id}.json"))
}
fn registry_path() -> PathBuf {
    root().join("config/smart-routers.json")
}

pub(super) fn legacy_port() -> Result<u16, String> {
    #[derive(Deserialize)]
    struct LegacyPort {
        port: u16,
    }
    match fs::read(root().join("config/smart-router-port.json")) {
        Ok(bytes) => {
            let value: LegacyPort =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if value.port == 0 {
                return Err("Invalid smart router port".into());
            }
            Ok(value.port)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SMART_ROUTER_PORT),
        Err(error) => Err(error.to_string()),
    }
}

fn registry() -> Result<Registry, String> {
    match fs::read(registry_path()) {
        Ok(bytes) => {
            let value: Registry = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if value.ids.is_empty()
                || value.ids.iter().any(|id| !valid_id(id))
                || value.ids.iter().collect::<HashSet<_>>().len() != value.ids.len()
            {
                return Err("Invalid smart router registry".into());
            }
            Ok(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let enabled = read_stored(SMART_ROUTER_INTERNAL_ID)
                .map(|c| c.enabled)
                .unwrap_or(true);
            Ok(Registry {
                enabled,
                ids: vec![SMART_ROUTER_INTERNAL_ID.into()],
            })
        }
        Err(e) => Err(e.to_string()),
    }
}
fn save_registry(value: &Registry) -> Result<(), String> {
    crate::services::native_account_files::write_private_file(
        &registry_path(),
        &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
}
pub(super) fn read_stored(id: &str) -> Option<StoredConfig> {
    serde_json::from_slice(&fs::read(path_for(id)).ok()?).ok()
}
fn valid_id(id: &str) -> bool {
    id == SMART_ROUTER_INTERNAL_ID
        || id
            .strip_prefix("smart-router-")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}
pub(crate) fn is_router_id(id: &str) -> bool {
    valid_id(id)
}
pub(crate) fn ids() -> Vec<String> {
    registry().map(|r| r.ids).unwrap_or_default()
}
fn check_id(id: &str) -> Result<(), String> {
    if registry()?.ids.iter().any(|candidate| candidate == id) {
        Ok(())
    } else {
        Err("Smart router not found".into())
    }
}
pub(crate) fn with<T>(id: &str, f: impl FnOnce() -> T) -> T {
    CURRENT.sync_scope(id.into(), f)
}
pub(crate) fn checked<T>(id: Option<String>, f: impl FnOnce() -> T) -> Result<T, String> {
    let id = id.unwrap_or_else(|| SMART_ROUTER_INTERNAL_ID.into());
    check_id(&id)?;
    Ok(with(&id, f))
}
pub(crate) fn current_port() -> Option<u16> {
    read_stored(&current_id()).and_then(|c| c.port)
}
pub(crate) fn all_ports() -> Vec<u16> {
    ids()
        .iter()
        .map(|id| {
            read_stored(id)
                .and_then(|c| c.port)
                .unwrap_or(SMART_ROUTER_PORT)
        })
        .collect()
}
pub fn available_port(exclude_port: Option<u16>) -> Result<u16, String> {
    let excluded: HashSet<_> = all_ports().into_iter().chain(exclude_port).collect();
    let mut rejected = Vec::new();
    loop {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(port_bind_error)?;
        let port = listener.local_addr().map_err(port_bind_error)?.port();
        if !excluded.contains(&port) {
            return Ok(port);
        }
        // Hold rejected ports so the OS cannot offer the same port again.
        rejected.push(listener);
    }
}
pub(crate) fn master_enabled() -> bool {
    registry().map(|r| r.enabled).unwrap_or(false)
}
pub(super) fn set_running(id: &str, running: bool) {
    if let Ok(mut groups) = RUNNING_GROUPS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        if running {
            groups.insert(id.into());
        } else {
            groups.remove(id);
        }
    }
}
pub(super) fn is_running(id: &str) -> bool {
    RUNNING_GROUPS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .is_ok_and(|groups| groups.contains(id))
}
pub(crate) fn candidate_used_elsewhere(candidate: &str) -> bool {
    ids()
        .iter()
        .filter(|id| *id != &current_id())
        .any(|id| read_stored(id).is_some_and(|c| c.candidate_ids.iter().any(|v| v == candidate)))
}
pub fn list() -> Result<Vec<PublicConfig>, String> {
    registry()?
        .ids
        .iter()
        .map(|id| with(id, get_public_config))
        .collect()
}
pub fn model_configs() -> Vec<ModelConfig> {
    if !master_enabled() {
        return vec![];
    }
    ids()
        .iter()
        .filter_map(|id| {
            with(id, || {
                if load_config().ok()?.enabled {
                    model_config()
                } else {
                    None
                }
            })
        })
        .collect()
}

pub async fn start_all() -> Result<(), String> {
    let registry = registry()?;
    let legacy = !registry_path().exists();
    save_registry(&registry)?;
    if legacy && path_for(SMART_ROUTER_INTERNAL_ID).exists() {
        with(SMART_ROUTER_INTERNAL_ID, || {
            let _guard = config_lock().lock().map_err(|e| e.to_string())?;
            let mut config = load_config_unlocked()?;
            // The previous switch becomes the master; the migrated group can resume with it.
            config.enabled = true;
            save_config_unlocked(&config)
        })?;
    }
    let mut servers = servers().lock().await;
    for id in registry.ids {
        if registry.enabled && with(&id, load_config)?.enabled {
            match with(&id, start_server) {
                Ok(server) => {
                    servers.insert(id, server);
                }
                Err(error) => log::error!("[SmartRouter] {id}: {error}"),
            }
        }
    }
    Ok(())
}
pub async fn set_group_enabled(id: &str, enabled: bool) -> Result<PublicConfig, String> {
    let mut servers = servers().lock().await;
    check_id(id)?;
    let started = if enabled && master_enabled() && !servers.contains_key(id) {
        Some(with(id, start_server)?)
    } else {
        None
    };
    let saved = with(id, || {
        let _guard = config_lock().lock().map_err(|e| e.to_string())?;
        let mut config = load_config_unlocked()?;
        config.enabled = enabled;
        save_config_unlocked(&config)
    });
    if let Err(error) = saved {
        if let Some(server) = started {
            server.stop().await;
        }
        return Err(error);
    }
    if let Some(server) = started {
        servers.insert(id.into(), server);
    }
    if !enabled {
        if let Some(server) = servers.remove(id) {
            server.stop().await;
        }
    }
    with(id, get_public_config)
}
pub async fn set_master_enabled(enabled: bool) -> Result<PublicConfig, String> {
    let mut servers = servers().lock().await;
    let mut registry = registry()?;
    let mut started = HashMap::new();
    if enabled {
        let configs = registry
            .ids
            .iter()
            .map(|id| with(id, load_config))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, config) in registry.ids.iter().zip(configs) {
            if !servers.contains_key(id) && config.enabled {
                match with(id, start_server) {
                    Ok(server) => {
                        started.insert(id.clone(), server);
                    }
                    Err(error) => {
                        for (_, server) in started {
                            server.stop().await;
                        }
                        return Err(error);
                    }
                }
            }
        }
    }
    registry.enabled = enabled;
    if let Err(error) = save_registry(&registry) {
        for (_, server) in started {
            server.stop().await;
        }
        return Err(error);
    }
    servers.extend(started);
    if !enabled {
        for server in servers.values() {
            server.cancel();
        }
        for (_, server) in servers.drain() {
            server.stop().await;
        }
    }
    with(&registry.ids[0], get_public_config)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub id: Option<String>,
    pub name: String,
    pub model_id: String,
    pub port: u16,
    pub api_key: String,
}
pub fn settings(id: String) -> Result<Settings, String> {
    check_id(&id)?;
    with(&id, || {
        let c = load_config()?;
        let api_key = model_manager::decrypt_key_for_use(&c.api_key);
        if !c.api_key.is_empty() && api_key.is_empty() {
            return Err("Smart router API key could not be decrypted".into());
        }
        Ok(Settings {
            id: Some(id.clone()),
            name: c.name,
            model_id: c.model_id,
            port: port(),
            api_key,
        })
    })
}
fn validate(settings: &Settings, registry: &Registry) -> Result<(), String> {
    if settings.name.trim().is_empty() || settings.model_id.trim().is_empty() || settings.port == 0
    {
        return Err("Name, model ID and a port between 1 and 65535 are required".into());
    }
    if settings.api_key.trim().is_empty() {
        return Err("Smart router API key is required".into());
    }
    if registry
        .ids
        .iter()
        .filter(|id| Some(id.as_str()) != settings.id.as_deref())
        .any(|id| with(id, port) == settings.port)
    {
        return Err(PORT_IN_USE.into());
    }
    Ok(())
}
pub async fn save(settings: Settings) -> Result<PublicConfig, String> {
    let mut servers = servers().lock().await;
    let mut registry = registry()?;
    validate(&settings, &registry)?;
    if let Some(id) = &settings.id {
        check_id(id)?;
    }
    let id = settings
        .id
        .clone()
        .unwrap_or_else(|| format!("smart-router-{}", uuid::Uuid::new_v4()));
    let (previous, config, listener) = with(&id, || -> Result<_, String> {
        let _guard = config_lock().lock().map_err(|e| e.to_string())?;
        let previous = if settings.id.is_some() {
            Some(load_config_unlocked()?)
        } else {
            None
        };
        let changed_port = previous
            .as_ref()
            .is_none_or(|c| c.port.unwrap_or(SMART_ROUTER_PORT) != settings.port);
        // Validate the port before changing the file or stopping a listener.
        let listener = if changed_port || !servers.contains_key(&id) {
            let listener = std::net::TcpListener::bind(("127.0.0.1", settings.port))
                .map_err(port_bind_error)?;
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            Some(listener)
        } else {
            None
        };
        let mut config = previous.clone().unwrap_or_default();
        config.name = settings.name.trim().into();
        config.model_id = settings.model_id.trim().into();
        config.port = Some(settings.port);
        config.api_key = crate::services::secret_store::encrypt(&settings.api_key)?;
        if model_manager::decrypt_key_for_use(&config.api_key) != settings.api_key {
            return Err("Smart router API key could not be encrypted".into());
        }
        save_config_unlocked(&config)?;
        Ok((previous, config, listener))
    })?;
    let started = match listener {
        Some(listener) if registry.enabled && config.enabled => {
            with(&id, || server::start(listener)).map(Some)
        }
        _ => Ok(None),
    };
    // A disabled group only reserves/validates the port while saving, without listening.
    if let Ok(Some(server)) = started {
        if let Some(old) = servers.insert(id.clone(), server) {
            old.stop().await;
        }
        set_running(&id, true);
    } else if let Err(error) = started {
        if let Some(previous) = &previous {
            with(&id, || {
                let _guard = config_lock().lock().map_err(|e| e.to_string())?;
                let mut current = load_config_unlocked()?;
                current.name = previous.name.clone();
                current.model_id = previous.model_id.clone();
                current.port = previous.port;
                current.api_key = previous.api_key.clone();
                save_config_unlocked(&current)
            })?;
        } else {
            let _ = fs::remove_file(path_for(&id));
        }
        return Err(error);
    }
    if settings.id.is_none() {
        registry.ids.push(id.clone());
        if let Err(error) = save_registry(&registry) {
            if let Some(server) = servers.remove(&id) {
                server.stop().await;
            }
            let _ = fs::remove_file(path_for(&id));
            return Err(error);
        }
    }
    with(&id, get_public_config)
}
pub async fn delete(id: String) -> Result<(), String> {
    let mut servers = servers().lock().await;
    let mut registry = registry()?;
    check_id(&id)?;
    if registry.ids.len() <= 1 {
        return Err("Keep at least one smart router".into());
    }
    registry.ids.retain(|candidate| candidate != &id);
    save_registry(&registry)?;
    if let Some(server) = servers.remove(&id) {
        server.stop().await;
    }
    // Native clients and candidate model definitions are never deleted with a group.
    // The registry is authoritative; a leftover private file cannot re-enable a deleted group.
    if let Err(error) = fs::remove_file(path_for(&id)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("[SmartRouter] delete {id}: {error}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn available_port_excludes_configured_ports_without_saving_or_leaving_a_listener() {
        let fixture = Fixture(
            std::env::temp_dir().join(format!("echobird-router-ports-{}", uuid::Uuid::new_v4())),
        );
        FIXTURE_ROOT.sync_scope(fixture.0.clone(), || {
            let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let occupied_port = occupied.local_addr().unwrap().port();
            let port = available_port(Some(occupied_port)).unwrap();
            assert_ne!(port, occupied_port);
            assert_ne!(port, SMART_ROUTER_PORT);
            let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
            assert!(!root().exists());
            drop(listener);

            fs::create_dir_all(root().join("config")).unwrap();
            let path = path_for(SMART_ROUTER_INTERNAL_ID);
            let config = serde_json::to_vec(&serde_json::json!({ "port": port })).unwrap();
            fs::write(&path, &config).unwrap();
            assert!(all_ports().contains(&port));
            let next = available_port(None).unwrap();
            assert_ne!(next, port);
            assert_eq!(fs::read(path).unwrap(), config);
            let _listener = std::net::TcpListener::bind(("127.0.0.1", next)).unwrap();
            assert!(!registry_path().exists());
        });
    }

    #[test]
    fn port_binding_errors_use_short_codes() {
        assert_eq!(
            port_bind_error(std::io::Error::from(std::io::ErrorKind::AddrInUse)),
            PORT_IN_USE
        );
        assert_eq!(
            port_bind_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            PORT_UNAVAILABLE
        );
    }

    #[test]
    fn loading_legacy_empty_keys_generates_and_persists_a_default_key() {
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "echobird-router-empty-key-{}",
            uuid::Uuid::new_v4()
        )));
        FIXTURE_ROOT.sync_scope(fixture.0.clone(), || {
            fs::create_dir_all(root().join("config")).unwrap();
            let path = path_for(SMART_ROUTER_INTERNAL_ID);
            fs::write(
                &path,
                br#"{"apiKey":"","apiKeyInitialized":true,"port":53683}"#,
            )
            .unwrap();
            let config = with(SMART_ROUTER_INTERNAL_ID, load_config).unwrap();
            let key = model_manager::decrypt_key_for_use(&config.api_key);
            assert!(key.starts_with("eb-local-"));
            let stored = fs::read(&path).unwrap();
            let config = with(SMART_ROUTER_INTERNAL_ID, load_config).unwrap();
            assert_eq!(model_manager::decrypt_key_for_use(&config.api_key), key);
            assert_eq!(fs::read(path).unwrap(), stored);
        });
    }

    #[test]
    fn migration_and_independent_listeners_preserve_flags_secrets_and_failed_edits() {
        let fixture = Fixture(
            std::env::temp_dir().join(format!("echobird-router-groups-{}", uuid::Uuid::new_v4())),
        );
        tauri::async_runtime::block_on(FIXTURE_ROOT.scope(fixture.0.clone(), async {
            fs::create_dir_all(root().join("config")).unwrap();
            let legacy_port_file = root().join("config/smart-router-port.json");
            let legacy_port_bytes = br#"{"port":61234,"pending_migrations":[]}"#;
            fs::write(&legacy_port_file, legacy_port_bytes).unwrap();
            fs::write(path_for(SMART_ROUTER_INTERNAL_ID), br#"{"enabled":false,"candidateIds":["legacy-candidate"],"disabledCandidateIds":["legacy-candidate"],"apiKey":"legacy-secret"}"#).unwrap();
            start_all().await.unwrap();
            let migrated = list().unwrap().remove(0);
            assert!(!migrated.master_enabled);
            assert!(migrated.group_enabled);
            assert_eq!(migrated.port, 61234);
            assert_eq!(migrated.candidate_ids, ["legacy-candidate"]);
            assert_eq!(migrated.disabled_candidate_ids, ["legacy-candidate"]);
            assert_eq!(settings(migrated.id.clone()).unwrap().api_key, "legacy-secret");

            let first_socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let second_socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let first_port = first_socket.local_addr().unwrap().port();
            let second_port = second_socket.local_addr().unwrap().port();
            drop((first_socket, second_socket));
            let first = Settings { id: Some(migrated.id.clone()), name: "First".into(), model_id: "first-model".into(), port: first_port, api_key: "first-secret".into() };
            save(first.clone()).await.unwrap();
            let second_settings = Settings { id: None, name: "Second".into(), model_id: "second-model".into(), port: second_port, api_key: "second-secret".into() };
            let second = save(second_settings.clone()).await.unwrap();
            assert!(!second.running);
            assert_eq!(save(Settings { port: first_port, ..second_settings.clone() }).await.err().as_deref(), Some(PORT_IN_USE));
            assert!(save(Settings { port: 0, ..second_settings }).await.is_err());
            assert_eq!(list().unwrap().len(), 2);
            assert!(!String::from_utf8(fs::read(path_for(&second.id)).unwrap()).unwrap().contains("second-secret"));
            assert!(!serde_json::to_string(&list().unwrap()).unwrap().contains("apiKey"));

            set_master_enabled(true).await.unwrap();
            assert!(list().unwrap().iter().all(|group| group.running && group.enabled));
            let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).pool_max_idle_per_host(0).build().unwrap();
            let first_url = format!("http://127.0.0.1:{first_port}/v1/models");
            let second_url = format!("http://127.0.0.1:{second_port}/v1/models");
            for (url, key, model) in [(&first_url, "first-secret", "first-model"), (&second_url, "second-secret", "second-model")] {
                let response = client.get(url).bearer_auth(key).send().await.unwrap();
                assert_eq!(response.status(), 200);
                let body: serde_json::Value = response.json().await.unwrap();
                assert_eq!(body["data"][0]["id"], model);
            }
            assert_eq!(client.get(&first_url).bearer_auth("second-secret").send().await.unwrap().status(), 401);
            assert_eq!(client.get(&second_url).bearer_auth("first-secret").send().await.unwrap().status(), 401);

            let before_invalid_save = fs::read(path_for(&migrated.id)).unwrap();
            for api_key in ["", " \t\r\n"] {
                assert_eq!(save(Settings { api_key: api_key.into(), ..first.clone() }).await.err().as_deref(), Some("Smart router API key is required"));
                assert_eq!(fs::read(path_for(&migrated.id)).unwrap(), before_invalid_save);
                assert_eq!(client.get(&first_url).bearer_auth("first-secret").send().await.unwrap().status(), 200);
                assert_eq!(client.get(&first_url).send().await.unwrap().status(), 401);
            }
            assert!(save(Settings { model_id: " \t".into(), ..first.clone() }).await.is_err());
            assert_eq!(fs::read(path_for(&migrated.id)).unwrap(), before_invalid_save);
            assert_eq!(list().unwrap().len(), 2);

            let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let before = fs::read(path_for(&migrated.id)).unwrap();
            assert_eq!(save(Settings { port: occupied.local_addr().unwrap().port(), ..first.clone() }).await.err().as_deref(), Some(PORT_IN_USE));
            assert_eq!(fs::read(path_for(&migrated.id)).unwrap(), before);
            assert_eq!(client.get(&first_url).bearer_auth("first-secret").send().await.unwrap().status(), 200);
            let mut changed = first.clone(); changed.model_id = "renamed-model".into(); changed.api_key = "changed-secret".into();
            save(changed.clone()).await.unwrap();
            let body: serde_json::Value = client.get(&first_url).bearer_auth("changed-secret").send().await.unwrap().json().await.unwrap();
            assert_eq!(body["data"][0]["id"], "renamed-model");
            assert_eq!(settings(migrated.id.clone()).unwrap().api_key, "changed-secret");
            assert_eq!(client.get(&first_url).bearer_auth("first-secret").send().await.unwrap().status(), 401);
            assert_eq!(client.get(&first_url).send().await.unwrap().status(), 401);

            let new_socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            changed.port = new_socket.local_addr().unwrap().port(); drop(new_socket);
            let moved = save(changed).await.unwrap();
            assert!(moved.running);
            assert!(client.get(&first_url).send().await.is_err());
            assert_eq!(client.get(format!("{}/models", moved.base_url)).bearer_auth("changed-secret").send().await.unwrap().status(), 200);
            assert_eq!(client.get(&second_url).bearer_auth("second-secret").send().await.unwrap().status(), 200);

            set_group_enabled(&second.id, false).await.unwrap();
            set_master_enabled(false).await.unwrap();
            assert!(list().unwrap().iter().all(|group| !group.running));
            set_master_enabled(true).await.unwrap();
            let groups = list().unwrap();
            assert!(groups[0].running);
            assert!(!groups[1].group_enabled && !groups[1].running);
            assert!(client.get(&second_url).send().await.is_err());

            set_master_enabled(false).await.unwrap();
            let unavailable = std::net::TcpListener::bind(("127.0.0.1", groups[0].port)).unwrap();
            assert!(set_master_enabled(true).await.is_err());
            assert!(!master_enabled());
            assert!(list().unwrap().iter().all(|group| !group.running));
            drop(unavailable);
            set_master_enabled(true).await.unwrap();
            set_master_enabled(false).await.unwrap();
            with(&second.id, || {
                let mut config = load_config().unwrap(); config.candidate_ids.push("shared".into()); save_config_unlocked(&config).unwrap();
            });
            assert!(with(&migrated.id, || candidate_used_elsewhere("shared")));
            delete(migrated.id).await.unwrap();
            assert_eq!(list().unwrap()[0].id, second.id);
            assert!(delete(second.id.clone()).await.is_err());
            assert!(settings("../outside".into()).is_err());
            assert_eq!(list().unwrap().len(), 1);
            assert!(list().unwrap()[0].candidate_ids.contains(&"shared".to_string()));
            assert_eq!(fs::read(legacy_port_file).unwrap(), legacy_port_bytes);
        }));
    }
}
