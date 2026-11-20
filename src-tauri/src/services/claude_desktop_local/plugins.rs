//! Native 1P plugin routes backed by the public catalog and explicitly enabled packages.
use super::{desktop, Context, ORGANIZATION};
use crate::services::{
    json_store,
    native_account_files::{DirectoryLock, Transaction},
};
use axum::{
    body::{self, Body},
    http::{header, Method, Request, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tokio::sync::Mutex;
use url::Url;

mod package;
use package::{name_component, Repository};

const DIRECTORY_URL: &str = "https://api.anthropic.com/api/directory/plugins";
const MARKET: &str = "anthropic-plugin-directory";
const MARKET_ID: &str = "marketplace_echobird_directory";
const MAX_DIRECTORY_BYTES: usize = 4 * 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;

type Installed = BTreeMap<String, Value>;

#[derive(Debug)]
pub(super) enum ArchiveError {
    BadRequest,
    Unauthorized,
    Unsupported,
    NotFound,
    Upstream,
    Invalid,
    Storage,
}

impl IntoResponse for ArchiveError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Invalid plugin request.",
            ),
            Self::Unauthorized => (
                StatusCode::FORBIDDEN,
                "permission_error",
                "The local Claude account did not authorize this plugin change.",
            ),
            Self::Unsupported => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_supported_error",
                "This plugin has no supported reviewed source package for local mode.",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "plugin_not_found",
                "Plugin not found.",
            ),
            Self::Upstream => (
                StatusCode::BAD_GATEWAY,
                "api_error",
                "Could not load the Claude plugin directory or source package.",
            ),
            Self::Invalid => (
                StatusCode::BAD_GATEWAY,
                "api_error",
                "The plugin source package is invalid or exceeds the local size limit.",
            ),
            Self::Storage => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "Could not read or save local plugin state.",
            ),
        };
        (
            status,
            Json(json!({"type":"error","error":{
                "type":code,"message":message,"details":{"error_code":code}
            }})),
        )
            .into_response()
    }
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    url: impl reqwest::IntoUrl,
    limit: usize,
) -> Result<Option<Vec<u8>>, ArchiveError> {
    // These are anonymous requests. Never pass through client headers or cookies.
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| ArchiveError::Upstream)?;
    match response.status() {
        StatusCode::NOT_FOUND => return Ok(None),
        StatusCode::OK => {}
        _ => return Err(ArchiveError::Upstream),
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(ArchiveError::Invalid);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ArchiveError::Upstream)? {
        if bytes.len() + chunk.len() > limit {
            return Err(ArchiveError::Invalid);
        }
        bytes.extend(&chunk);
    }
    Ok(Some(bytes))
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("plugin_").is_some_and(|part| {
        !part.is_empty()
            && part.len() <= 64
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

pub(super) struct State {
    dir: PathBuf,
    client: reqwest::Client,
    mutation: Mutex<()>,
}

impl State {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            dir,
            client,
            mutation: Mutex::new(()),
        })
    }

    async fn installed(&self) -> Result<Installed, ArchiveError> {
        let path = self.dir.join("installed.json");
        tokio::task::spawn_blocking(move || json_store::read(&path))
            .await
            .map_err(|_| ArchiveError::Storage)?
            .map_err(|_| ArchiveError::Storage)
    }

    async fn directory(&self, url: Url) -> Result<Option<Value>, ArchiveError> {
        fetch(&self.client, url, MAX_DIRECTORY_BYTES)
            .await?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| ArchiveError::Upstream))
            .transpose()
    }

    async fn by_id(&self, id: &str) -> Result<Value, ArchiveError> {
        let mut url = directory_url();
        url.path_segments_mut()
            .expect("plugin directory URL")
            .push(id);
        let row = self.directory(url).await?.ok_or(ArchiveError::NotFound)?;
        if row["id"] != id {
            return Err(ArchiveError::NotFound);
        }
        if !is_installable(&row) {
            return Err(ArchiveError::Unsupported);
        }
        Ok(row)
    }

    async fn by_name(&self, name: &str) -> Result<Value, ArchiveError> {
        if !name_component(name) {
            return Err(ArchiveError::NotFound);
        }
        let mut url = directory_url();
        url.query_pairs_mut().append_pair("limit", "2").append_pair(
            "filter",
            &format!("manifest_name == '{name}' && source.marketplace_slug == '{MARKET}'"),
        );
        let result = self.directory(url).await?.ok_or(ArchiveError::NotFound)?;
        if let Some(entry) = unique_entry(&result["data"], |entry| entry["manifest_name"] == name) {
            return Ok(entry.clone());
        }

        // Older desktop links can use the manifest's readable name instead of the catalog ID.
        let mut url = directory_url();
        url.query_pairs_mut()
            .append_pair("limit", "32")
            .append_pair("q", name)
            .append_pair("filter", &format!("source.marketplace_slug == '{MARKET}'"));
        let result = self.directory(url).await?.ok_or(ArchiveError::NotFound)?;
        if result["next_page"]
            .as_str()
            .is_some_and(|page| !page.is_empty())
        {
            return Err(ArchiveError::NotFound);
        }
        legacy_name(&result["data"], name)
            .cloned()
            .ok_or(ArchiveError::NotFound)
    }

    async fn change(&self, id: &str, enable: bool) -> Result<Value, ArchiveError> {
        let _lock = self.mutation.lock().await;
        let installed = self.installed().await?;
        if enable {
            if let Some(entry) = installed.get(id) {
                let file = self.package_path(id, entry)?;
                if std::fs::symlink_metadata(file).is_ok_and(|metadata| metadata.is_file()) {
                    return Ok(plugin_record(entry, true));
                }
            }
            let entry = self.by_id(id).await?;
            let source = Repository::from_entry(&entry).ok_or(ArchiveError::Unsupported)?;
            let bytes = source.download(&self.client).await?;
            self.save(id, Some((entry.clone(), bytes))).await?;
            Ok(plugin_record(&entry, true))
        } else {
            let entry = installed.get(id).ok_or(ArchiveError::NotFound)?;
            self.save(id, None).await?;
            Ok(plugin_record(entry, false))
        }
    }

    fn package_path(&self, id: &str, entry: &Value) -> Result<PathBuf, ArchiveError> {
        let commit = Repository::from_entry(entry)
            .ok_or(ArchiveError::Storage)?
            .commit;
        if !valid_id(id) || entry["id"] != id {
            return Err(ArchiveError::Storage);
        }
        Ok(self.dir.join(format!("{id}-{commit}.zip")))
    }

    async fn package(&self, id: &str, installed: &Installed) -> Result<Response, ArchiveError> {
        let entry = installed.get(id).ok_or(ArchiveError::NotFound)?;
        let path = self.package_path(id, entry)?;
        let bytes = tokio::task::spawn_blocking(move || {
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| ArchiveError::Storage)?;
            if !metadata.is_file() || metadata.len() > MAX_PACKAGE_BYTES as u64 {
                return Err(ArchiveError::Storage);
            }
            std::fs::read(path).map_err(|_| ArchiveError::Storage)
        })
        .await
        .map_err(|_| ArchiveError::Storage)??;
        Ok(([(header::CONTENT_TYPE, "application/zip")], bytes).into_response())
    }

    async fn save(&self, id: &str, next: Option<(Value, Vec<u8>)>) -> Result<(), ArchiveError> {
        let path = self.dir.join("installed.json");
        let next = next
            .map(|(entry, bytes)| {
                self.package_path(id, &entry)
                    .map(|path| (path, entry, bytes))
            })
            .transpose()?;
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let _lock = DirectoryLock::acquire(&path).map_err(|_| ArchiveError::Storage)?;
            let mut bundle = Transaction::guarded();
            let old = json_store::mutate(&path, |installed: &mut Installed| {
                let old = if let Some((file, entry, bytes)) = &next {
                    bundle.replace(file, bytes)?;
                    installed.insert(id.clone(), entry.clone())
                } else {
                    installed.remove(&id)
                };
                Ok((old, true))
            })
            .map_err(|_| ArchiveError::Storage)?;
            bundle.commit();
            // A crash can leave an unreferenced package on disk. The installed
            // index must never point at a package we have already deleted.
            if let Some(source) = old.as_ref().and_then(Repository::from_entry) {
                let file = path.with_file_name(format!("{id}-{}.zip", source.commit));
                if next.as_ref().is_none_or(|(next, _, _)| *next != file) {
                    match std::fs::remove_file(file) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => log::warn!(
                            "[ClaudeLocal] Could not remove an unused plugin package: {error}"
                        ),
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| ArchiveError::Storage)?
    }
}

fn directory_url() -> Url {
    DIRECTORY_URL
        .parse()
        .expect("fixed Claude plugin directory URL")
}

fn is_installable(entry: &Value) -> bool {
    entry["id"].as_str().is_some_and(valid_id)
        && entry["manifest_name"].as_str().is_some_and(name_component)
        && entry["source"]["marketplace_slug"] == MARKET
        && Repository::from_entry(entry).is_some()
}

fn unique_entry(rows: &Value, matches: impl Fn(&Value) -> bool) -> Option<&Value> {
    let mut matches = rows
        .as_array()?
        .iter()
        .filter(|entry| is_installable(entry) && matches(entry));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn legacy_name<'a>(rows: &'a Value, name: &str) -> Option<&'a Value> {
    let same_slug = |entry: &Value| {
        entry["slug"]
            .as_str()
            .and_then(|slug| slug.rsplit('/').next())
            == Some(name)
    };
    unique_entry(rows, |entry| {
        same_slug(entry)
            && entry["name"]
                .as_str()
                .is_some_and(|display| display.eq_ignore_ascii_case(name))
    })
    .or_else(|| unique_entry(rows, same_slug))
}

fn marketplace() -> Value {
    json!({
        "id":MARKET_ID,"name":MARKET,"display_name":"Anthropic Plugin Directory",
        "source":"directory","is_default":true,"is_visible_for_org":true,"sync_status":"success"
    })
}

fn plugin_record(entry: &Value, enabled: bool) -> Value {
    let components: Vec<_> = entry["components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| value["type"].is_string() && value["name"].is_string())
        .collect();
    let described = |kind| {
        components
            .iter()
            .filter(|item| item["type"] == kind)
            .map(|item| json!({"name":item["name"],"description":item["description"]}))
            .collect::<Vec<Value>>()
    };
    let has_local = components
        .iter()
        .any(|item| item["access"]["type"] == "runs_on_your_computer" || item["type"] == "hook");
    let has_remote = components
        .iter()
        .any(|item| item["access"]["type"] == "sends_data_to");
    let reach = match (has_local, has_remote) {
        (true, true) => "remote_privileged",
        (true, false) => "privileged",
        (false, true) => "remote",
        (false, false) => "contained",
    };
    let mut skills = described("skill");
    for skill in &mut skills {
        skill["user_invocable"] = true.into();
    }
    json!({
        "id":entry["id"],"name":entry["manifest_name"],"display_name":entry["name"],
        "description":entry["summary"],"version":entry["release"]["version"],
        "marketplace_id":MARKET_ID,"marketplace_name":MARKET,"marketplace_kind":"anthropic_catalog",
        "partition_by":"marketplace","installation_preference":"available","enabled":enabled,"is_shared":false,
        "author":{"name":entry["publisher"]["name"]},"created_at":entry["added_time"],"updated_at":entry["updated_time"],
        "reach":reach,"upstream":{"repo_url":Repository::from_entry(entry).map(|source| source.url())},
        "mcp_servers":{},"skills":skills,"commands":described("command"),"agents":described("agent"),
        "hooks":components.iter().filter(|item| item["type"] == "hook").map(|item| json!({"event":item["name"],"matchers":[]})).collect::<Vec<Value>>(),
        "components":components.iter().map(|item| json!({"type":item["type"],"name":item["name"]})).collect::<Vec<Value>>(),
        "content_scan":null
    })
}

fn installed_directory_record(entry: &Value) -> Value {
    let mut entry = entry.clone();
    entry["installation"] = json!({"state":"enabled","actions":{"disable":true,"remove":true,"save_new_version":false},
        "admin_policy":{"type":"set","install_mode":"optional"}});
    entry
}

fn library_item(entry: &Value) -> Value {
    let mut row = plugin_record(entry, true);
    row["provenance"] = json!({"source":"default","marketplace_id":MARKET_ID,"marketplace_name":MARKET,"marketplace_kind":"anthropic_catalog"});
    row["plugin"] = json!({"backend_id":entry["id"],"partition_by":"marketplace"});
    row["viewer"] = json!({"added":true,"installed":{"enabled":true,"can_toggle":true,"can_remove":true,"shared_by_you":false}});
    row
}

enum Endpoint<'a> {
    Directory,
    Marketplace,
    MarketplacePlugins,
    Plugins,
    Preferences,
    Library,
    ByName(&'a str),
    Plugin(&'a str, &'a str),
}

impl<'a> Endpoint<'a> {
    fn parse(path: &'a str, query: &Query) -> Option<Self> {
        if path == "/api/directory/plugins" {
            return Some(Self::Directory);
        }
        let (org, route) = path.strip_prefix("/api/organizations/")?.split_once('/')?;
        if org != ORGANIZATION {
            return None;
        }
        match route {
            "directory/plugins" => Some(Self::Directory),
            "marketplaces/list-default-marketplaces" => Some(Self::Marketplace),
            "plugins/list-plugins" => Some(Self::Plugins),
            "plugins/enabled-state" => Some(Self::Preferences),
            "library/mine" if query.get("kind") == Some("plugin") => Some(Self::Library),
            _ => {
                let segments = route.split('/').collect::<Vec<_>>();
                match segments.as_slice() {
                    ["marketplaces", _, "plugins", "list-plugins" | "account-list-plugins"] => {
                        Some(Self::MarketplacePlugins)
                    }
                    ["plugins", "by-name", name] => Some(Self::ByName(name)),
                    ["plugins", id] if *id != "registry" => Some(Self::Plugin(id, "")),
                    ["plugins", id, action @ ("scan-status" | "versions" | "download" | "enabled")] => {
                        Some(Self::Plugin(id, action))
                    }
                    _ => None,
                }
            }
        }
    }
}

struct Query(Vec<(String, String)>);

impl Query {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

fn directory_request(query: &Query) -> Url {
    let mut url = directory_url();
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in &query.0 {
            if matches!(
                key.as_str(),
                "q" | "limit" | "filter" | "order_by" | "include"
            ) {
                pairs.append_pair(key, value);
            }
        }
        if let Some(page) = ["page", "next_page", "cursor", "page_token"]
            .into_iter()
            .filter_map(|key| query.get(key))
            .find(|value| !value.is_empty())
        {
            pairs.append_pair("page", page);
        }
    }
    url
}

async fn directory_list(
    state: &State,
    query: &Query,
    installed: &Installed,
) -> Result<Response, ArchiveError> {
    let filter = query.get("filter").unwrap_or_default();
    if filter.contains("installation.state")
        && (filter.contains("'enabled'") || filter.contains("'disabled'"))
    {
        let data: Vec<_> = installed.values().map(installed_directory_record).collect();
        return Ok(Json(json!({"data":data,"items":data,"pagination":{"has_more":false,"cursor":null},"has_more":false,"next_cursor":null})).into_response());
    }
    let mut result = state
        .directory(directory_request(query))
        .await?
        .ok_or(ArchiveError::Upstream)?;
    let mut data = result["data"]
        .as_array()
        .cloned()
        .ok_or(ArchiveError::Upstream)?;
    for entry in &mut data {
        if entry["id"]
            .as_str()
            .is_some_and(|id| installed.contains_key(id))
        {
            *entry = installed_directory_record(entry);
        }
    }
    let next = result["next_page"]
        .as_str()
        .or_else(|| result["next_cursor"].as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let has_more = next.is_some() || result["pagination"]["has_more"] == true;
    result["data"] = json!(data);
    result["items"] = result["data"].clone();
    result["has_more"] = has_more.into();
    result["next_cursor"] = json!(next);
    result["pagination"] = json!({"has_more":has_more,"cursor":next,"next_cursor":next});
    Ok(Json(result).into_response())
}

async fn read_endpoint(
    state: &State,
    endpoint: Endpoint<'_>,
    query: &Query,
) -> Result<Response, ArchiveError> {
    if matches!(endpoint, Endpoint::Marketplace) {
        return Ok(Json(json!({"marketplaces":[marketplace()]})).into_response());
    }
    if matches!(endpoint, Endpoint::MarketplacePlugins) {
        return Ok(Json(json!({"plugins":[],"has_more":false})).into_response());
    }
    let installed = state.installed().await?;
    let result = match endpoint {
        Endpoint::Directory => return directory_list(state, query, &installed).await,
        Endpoint::Plugins => {
            let entries: Vec<_> = installed
                .values()
                .filter(|_| query.get("enabled_only") == Some("true"))
                .map(|entry| plugin_record(entry, true))
                .collect();
            json!({"plugins":entries,"installed_plugin_ids":installed.keys().collect::<Vec<_>>(),"has_more":false})
        }
        Endpoint::Preferences => {
            let preferences: Vec<_> = installed.values().map(|entry| {
                json!({"plugin_name":entry["manifest_name"],"marketplace_id":MARKET_ID,"marketplace_name":MARKET,"enabled":true})
            }).collect();
            json!({"preferences":preferences})
        }
        Endpoint::Library => {
            json!({"items":installed.values().map(library_item).collect::<Vec<_>>(),"next_cursor":null})
        }
        Endpoint::ByName(name) => {
            if query
                .get("marketplace_name")
                .is_some_and(|market| market != MARKET)
            {
                return Err(ArchiveError::NotFound);
            }
            let selected = installed
                .values()
                .find(|entry| entry["manifest_name"].as_str() == Some(name));
            let entry = match selected {
                Some(entry) => entry.clone(),
                None => state.by_name(name).await?,
            };
            let enabled = entry["id"]
                .as_str()
                .is_some_and(|id| installed.contains_key(id));
            plugin_record(&entry, enabled)
        }
        Endpoint::Plugin(id, action) if valid_id(id) => match action {
            "download" => return state.package(id, &installed).await,
            "versions" | "enabled" => return Err(ArchiveError::NotFound),
            "" | "scan-status" => {
                let entry = match installed.get(id) {
                    Some(entry) => entry.clone(),
                    None => state.by_id(id).await?,
                };
                if action == "scan-status" {
                    json!({"content_scan":null})
                } else {
                    plugin_record(&entry, installed.contains_key(id))
                }
            }
            _ => return Err(ArchiveError::NotFound),
        },
        _ => return Err(ArchiveError::NotFound),
    };
    Ok(Json(result).into_response())
}

async fn change_endpoint(
    state: &State,
    endpoint: Endpoint<'_>,
    request: Request<Body>,
    token: &str,
) -> Result<Response, ArchiveError> {
    let Endpoint::Plugin(id, "enabled") = endpoint else {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    };
    if !desktop::authorized(&request, token) {
        return Err(ArchiveError::Unauthorized);
    }
    if !valid_id(id) {
        return Err(ArchiveError::NotFound);
    }
    let id = id.to_owned();
    let bytes = body::to_bytes(request.into_body(), 16 * 1024)
        .await
        .map_err(|_| ArchiveError::BadRequest)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ArchiveError::BadRequest)?;
    let enabled = value["enabled"].as_bool().ok_or(ArchiveError::BadRequest)?;
    let updated = state.change(&id, enabled).await?;
    Ok(Json(updated).into_response())
}

pub(super) async fn response(
    request: Request<hyper::body::Incoming>,
    context: &Context,
) -> Result<Response, Request<hyper::body::Incoming>> {
    let path = request.uri().path().to_owned();
    if path != "/api/directory/plugins" && !path.starts_with("/api/organizations/") {
        return Err(request);
    }
    let query = Query(
        url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
            .into_owned()
            .collect(),
    );
    let Some(endpoint) = Endpoint::parse(&path, &query) else {
        return Err(request);
    };
    let origin = request.headers().get(header::ORIGIN).cloned();
    if origin.as_ref().is_some_and(|value| {
        !matches!(
            value.to_str(),
            Ok("https://claude.ai" | "https://www.claude.ai" | "https://claude.com")
        )
    }) {
        return Ok(ArchiveError::Unauthorized.into_response());
    }
    let requested_headers = request
        .headers()
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .cloned();
    let mut response = match *request.method() {
        Method::OPTIONS => StatusCode::NO_CONTENT.into_response(),
        Method::GET => read_endpoint(&context.plugins, endpoint, &query)
            .await
            .unwrap_or_else(IntoResponse::into_response),
        Method::PUT => change_endpoint(
            &context.plugins,
            endpoint,
            request.map(Body::new),
            &context.token,
        )
        .await
        .unwrap_or_else(IntoResponse::into_response),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    if let Some(origin) = origin {
        use axum::http::HeaderValue;
        let headers = response.headers_mut();
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, PUT, OPTIONS"),
        );
        if let Some(value) = requested_headers {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, value);
        }
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "plugin_fixture";

    fn entry(id: &str) -> Value {
        json!({
            "id":id,"name":"Fixture Plugin","manifest_name":"fixture","summary":"Fixture skill",
            "release":{"version":"1.2.3"},"source":{"type":"directory","marketplace_slug":MARKET,
            "repository":{"url":"https://github.com/fixture/plugin","reviewed_commit":"0123456789abcdef0123456789abcdef01234567"}},
            "components":[{"type":"hook","name":"SessionStart","access":{"type":"runs_on_your_computer"}}],
        })
    }

    #[tokio::test]
    async fn plugin_state_reads_are_passive_and_only_explicit_changes_remove_the_package() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("plugins");
        let state = State::new(dir.clone()).unwrap();
        let query = Query(vec![("enabled_only".into(), "true".into())]);
        let empty = read_endpoint(&state, Endpoint::Plugins, &query)
            .await
            .unwrap();
        let empty: Value =
            serde_json::from_slice(&body::to_bytes(empty.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(empty["plugins"], json!([]));
        assert!(!dir.exists());
        let unavailable = read_endpoint(&state, Endpoint::Plugin(ID, "download"), &query)
            .await
            .unwrap_err();
        assert!(matches!(unavailable, ArchiveError::NotFound));
        assert!(!dir.exists());

        let data = entry(ID);
        let file = state.package_path(ID, &data).unwrap();
        state
            .save(ID, Some((data, b"fixture-package".to_vec())))
            .await
            .unwrap();
        let restarted = State::new(dir.clone()).unwrap();
        let installed = restarted.installed().await.unwrap();
        assert!(installed.contains_key(ID));
        let ready = restarted.package(ID, &installed).await.unwrap();
        assert_eq!(ready.headers()[header::CONTENT_TYPE], "application/zip");
        assert_eq!(
            &body::to_bytes(ready.into_body(), 4096).await.unwrap()[..],
            b"fixture-package"
        );
        let denied = change_endpoint(
            &restarted,
            Endpoint::Plugin(ID, "enabled"),
            Request::new(Body::from(r#"{"enabled":false}"#)),
            "fixture-token",
        )
        .await
        .unwrap_err();
        assert!(matches!(denied, ArchiveError::Unauthorized));
        assert!(restarted.installed().await.unwrap().contains_key(ID));
        let invalid = change_endpoint(
            &restarted,
            Endpoint::Plugin(ID, "enabled"),
            Request::builder()
                .header("authorization", "Bearer fixture-token")
                .body(Body::from(r#"{"enable":false}"#))
                .unwrap(),
            "fixture-token",
        )
        .await
        .unwrap_err();
        assert!(matches!(invalid, ArchiveError::BadRequest));
        assert!(file.is_file());

        let response = change_endpoint(
            &restarted,
            Endpoint::Plugin(ID, "enabled"),
            Request::builder()
                .header("cookie", "sessionKey=fixture-token")
                .body(Body::from(r#"{"enabled":false}"#))
                .unwrap(),
            "fixture-token",
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(restarted.installed().await.unwrap().is_empty());
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn an_unremovable_cache_entry_cannot_keep_a_disabled_plugin_installed() {
        let root = tempfile::tempdir().unwrap();
        let state = State::new(root.path().join("plugins")).unwrap();
        let installed = entry(ID);
        let file = state.package_path(ID, &installed).unwrap();
        state
            .save(ID, Some((installed, b"fixture".to_vec())))
            .await
            .unwrap();
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        state.change(ID, false).await.unwrap();
        let restarted = State::new(state.dir.clone()).unwrap();
        assert!(restarted.installed().await.unwrap().is_empty());
        assert!(file.is_dir());
    }

    #[test]
    fn directory_uses_the_public_page_parameter_for_native_continuation_tokens() {
        for alias in ["page", "next_page", "cursor", "page_token"] {
            let url = directory_request(&Query(vec![
                ("limit".into(), "3".into()),
                (alias.into(), "opaque-token".into()),
                ("sort".into(), "unrecognized".into()),
            ]));
            assert_eq!(url.query(), Some("limit=3&page=opaque-token"));
        }
    }

    #[test]
    fn catalog_entries_require_a_reviewed_source_and_keep_native_confirmations() {
        let mut candidate = entry(ID);
        assert!(is_installable(&candidate));
        assert_eq!(plugin_record(&candidate, false)["reach"], "privileged");
        candidate["source"]["repository"]["reviewed_commit"] = "main".into();
        assert!(!is_installable(&candidate));
        candidate = entry(ID);
        candidate["source"]["repository"]["url"] =
            "https://github.com.invalid/fixture/plugin".into();
        assert!(!is_installable(&candidate));
        candidate = entry(ID);
        candidate["source"]["repository"]["path"] = "../another-plugin".into();
        assert!(!is_installable(&candidate));
        let duplicates = json!([entry(ID), entry("plugin_another")]);
        assert!(unique_entry(&duplicates, |entry| entry["manifest_name"] == "fixture").is_none());
        assert!(Endpoint::parse(
            "/api/organizations/official/plugins/list-plugins",
            &Query(vec![])
        )
        .is_none());
    }

    #[test]
    fn readable_names_disambiguate_identical_repository_slugs() {
        let mut primary = entry(ID);
        primary["name"] = "Fixture".into();
        primary["slug"] = "publisher/fixture".into();
        let mut alternative = entry("plugin_another");
        alternative["name"] = "Another Fixture".into();
        alternative["slug"] = "another/fixture".into();
        let rows = json!([primary, alternative]);
        assert_eq!(legacy_name(&rows, "fixture").unwrap()["id"], ID);
        let mut ambiguous = rows;
        ambiguous[1]["name"] = "Fixture".into();
        assert!(legacy_name(&ambiguous, "fixture").is_none());
    }

    #[tokio::test]
    #[ignore = "read-only live check of the public directory and reviewed source without executing plugin code"]
    async fn public_plugin_sources_match_both_url_lookup_forms_and_download() {
        let root = tempfile::tempdir().unwrap();
        let state = State::new(root.path().join("plugins")).unwrap();
        let mut query = Query(vec![("limit".into(), "3".into())]);
        let first = directory_list(&state, &query, &Installed::new())
            .await
            .unwrap();
        let first: Value = serde_json::from_slice(
            &body::to_bytes(first.into_body(), MAX_DIRECTORY_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        let next = first["next_cursor"].as_str().unwrap();
        assert_eq!(first["pagination"]["has_more"], true);
        query.0.push(("cursor".into(), next.into()));
        let second = directory_list(&state, &query, &Installed::new())
            .await
            .unwrap();
        let second: Value = serde_json::from_slice(
            &body::to_bytes(second.into_body(), MAX_DIRECTORY_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(second["data"][0]["id"].is_string());
        assert_ne!(first["data"][0]["id"], second["data"][0]["id"]);

        let entry = state.by_name("superpowers").await.unwrap();
        let id = entry["id"].as_str().unwrap();
        assert_eq!(state.by_id(id).await.unwrap()["id"], id);
        let name = entry["manifest_name"].as_str().unwrap();
        assert_eq!(state.by_name(name).await.unwrap()["id"], id);
        let source = Repository::from_entry(&entry).unwrap();
        assert!(!source.download(&state.client).await.unwrap().is_empty());
        assert!(!state.dir.exists());
    }
}
