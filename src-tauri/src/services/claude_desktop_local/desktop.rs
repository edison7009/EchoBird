//! Desktop task bootstrap responses. Execution remains in the native client.
use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::sync::Mutex;

pub fn prompt_configuration(prompt: Option<&str>) -> Value {
    match prompt.filter(|value| !value.trim().is_empty()) {
        Some(prompt) => json!({"cowork_system_prompt":{"value":{"prompt":prompt},
            "on":true,"off":false,"source":"defaultValue","ruleId":null}}),
        None => Value::Null,
    }
}

pub struct State {
    path: PathBuf,
    registration: Mutex<()>,
}

impl State {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            registration: Mutex::new(()),
        }
    }

    async fn registered(&self) -> Result<Value, String> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            super::super::native_account_files::read_optional(&path)?
                .map(|bytes| {
                    serde_json::from_slice(&bytes)
                        .map_err(|_| "Invalid local device registration".into())
                })
                .transpose()
                .map(|value| value.unwrap_or(Value::Null))
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn devices(&self) -> Result<Value, String> {
        let value = self.registered().await?;
        let values = if value.is_null() {
            Vec::new()
        } else {
            vec![value]
        };
        Ok(json!({"devices":values,"rows":values,"items":values,"data":values}))
    }

    pub async fn register(&self, mut value: Value) -> Result<Value, String> {
        let _guard = self.registration.lock().await;
        let object = value.as_object_mut().ok_or("Invalid device registration")?;
        let previous = self.registered().await?;
        let id = previous["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        object.insert("id".into(), id.into());
        if !object.contains_key("k") {
            if let Some(key) = ["public_key", "enclave_public_key", "key", "device_key"]
                .iter()
                .find_map(|key| object.get(*key).cloned())
            {
                object.insert("k".into(), key);
            }
        }
        if object.get("k").is_none_or(Value::is_null) {
            return Err("A device public key is required".into());
        }
        let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            super::super::native_account_files::write(&path, &bytes)
        })
        .await
        .map_err(|error| error.to_string())??;
        Ok(value)
    }
}

pub(super) fn authorized(request: &Request<Body>, token: &str) -> bool {
    request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {token}").as_str())
        || request
            .headers()
            .get("cookie")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|cookie| {
                cookie
                    .split(';')
                    .any(|pair| pair.trim().strip_prefix("sessionKey=") == Some(token))
            })
}

async fn body(request: Request<Body>) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let encoding = request
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("identity")
        .to_ascii_lowercase();
    let bytes = axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .map_err(|_| "Desktop request is too large")?;
    let reader: Box<dyn Read + Send> = match encoding.as_str() {
        "identity" => return Ok(bytes.to_vec()),
        "gzip" => Box::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes))),
        "deflate" => Box::new(flate2::read::ZlibDecoder::new(std::io::Cursor::new(bytes))),
        _ => return Err("Unsupported desktop request encoding".into()),
    };
    tokio::task::spawn_blocking(move || {
        let mut output = Vec::new();
        reader
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut output)
            .map_err(|_| "Invalid compressed desktop request")?;
        if output.len() > 2 * 1024 * 1024 {
            return Err("Desktop request is too large".into());
        }
        Ok(output)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub async fn device(request: Request<Body>, context: super::Context) -> Response<Body> {
    if !authorized(&request, &context.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if request.method() == "GET" {
        return match context.desktop.devices().await {
            Ok(value) => Json(value).into_response(),
            Err(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":message}})),
            )
                .into_response(),
        };
    }
    if !matches!(request.method().as_str(), "POST" | "PUT") {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let result = async {
        let bytes = body(request).await?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid device registration")?;
        context.desktop.register(value).await
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(message) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":message}})),
        )
            .into_response(),
    }
}

pub async fn title(request: Request<Body>, context: super::Context) -> Response<Body> {
    if request.method() != "POST" {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if !authorized(&request, &context.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let branch = request.uri().path().ends_with("/generate_title_and_branch");
    let result = async {
        let bytes = body(request).await?;
        serde_json::from_slice::<Value>(&bytes).map_err(|_| "Invalid title request".to_owned())
    }
    .await;
    let value = match result {
        Ok(value) => value,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"message":message}})),
            )
                .into_response()
        }
    };
    let source = ["first_session_message", "message", "prompt"]
        .iter()
        .find_map(|field| value[*field].as_str())
        .unwrap_or("New task");
    let title = source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(60)
        .collect::<String>();
    let mut result = json!({"title":title});
    if branch {
        let slug = title
            .to_ascii_lowercase()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let slug = slug.trim_matches('-');
        if !slug.is_empty() {
            result["branch_name"] = format!("claude/{slug}").into();
        }
    }
    Json(result).into_response()
}

pub fn response(path: &str, method: &str) -> Option<Response<Body>> {
    if method != "GET" {
        return None;
    }
    let value = if path == "/api/directory/servers" {
        json!({"servers":[],"next_cursor":null})
    } else if path.starts_with("/api/organizations/")
        && path.contains("/activation/")
        && path.ends_with("/tasks")
    {
        json!({"tasks":[]})
    } else if path.ends_with("/cowork_plugin_metadata") {
        json!({})
    } else if path == "/api/desktop/features" {
        // Computer control and background-mode overrides have separate scopes.
        json!({"features":{}})
    } else if path.ends_with("/skills/list-skills") {
        json!({"skills":[]})
    } else if path.ends_with("/mcp/remote_servers_with_connection") {
        json!([])
    } else if path.ends_with("/memory/settings") {
        json!({"enabled_saffron":null,"enabled_saffron_search":false,"enabled_melange":null,
            "memory_mode":"melange","melange_available":false,"classic_mode_available":false,"melange_imported_at":null})
    } else if path.ends_with("/experiences/claude_web") {
        json!({"experiences":[],"rules":{"global":{"rate_limit":null,"cooldown":null},"placements":{},"tiers":{}}})
    } else if path.ends_with("/kyc_status") {
        json!({"status":"not_required","should_poll":false})
    } else {
        return None;
    };
    Some((StatusCode::OK, Json(value)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cowork_prompt_readiness_requires_a_nonempty_nested_prompt() {
        assert!(prompt_configuration(None).is_null());
        assert!(prompt_configuration(Some("  ")).is_null());
        let config = prompt_configuration(Some("Bundled native workspace instructions"));
        assert_eq!(
            config["cowork_system_prompt"]["value"]["prompt"],
            "Bundled native workspace instructions"
        );
        assert_eq!(config["cowork_system_prompt"]["on"], true);
    }

    #[tokio::test]
    async fn native_device_registration_is_private_durable_and_passive_until_explicit_write() {
        let root = std::env::temp_dir().join(format!("echobird-device-{}", uuid::Uuid::new_v4()));
        let path = root.join("device.json");
        let state = State::new(path.clone());
        assert_eq!(state.devices().await.unwrap()["devices"], json!([]));
        assert!(!root.exists());
        let registered = state
            .register(json!({"public_key":"fixture-key"}))
            .await
            .unwrap();
        assert_eq!(registered["k"], "fixture-key");
        let restored = State::new(path.clone());
        assert_eq!(restored.devices().await.unwrap()["devices"][0], registered);
        let next = restored.register(json!({"k":"next-key"})).await.unwrap();
        assert_eq!(next["id"], registered["id"]);
        let previous = std::fs::read(&path).unwrap();
        assert!(restored.register(json!({})).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn cowork_bootstrap_responses_match_the_native_bff_shapes() {
        for (path, field) in [
            ("/api/directory/servers", "servers"),
            ("/api/organizations/org/activation/device/tasks", "tasks"),
        ] {
            let response = response(path, "GET").unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()[field],
                json!([])
            );
            assert!(super::response(path, "POST").is_none());
        }
    }
}
