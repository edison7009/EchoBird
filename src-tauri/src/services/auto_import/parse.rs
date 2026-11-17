use super::super::model_manager::AddModelInput;
use serde_json::{json, Value};

pub(super) enum Account {
    Codex(Value),
    Claude(Value, Value),
    Antigravity(String),
}

#[derive(Default)]
pub(super) struct Parsed {
    pub models: Vec<AddModelInput>,
    pub accounts: Vec<Account>,
    pub issues: Vec<&'static str>,
}

fn text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
    })
}

pub(super) fn parse(source: &str, document: &Value) -> Parsed {
    let mut parsed = Parsed::default();
    visit(source, document, &mut parsed, 0);
    parsed
}

fn visit(source: &str, value: &Value, parsed: &mut Parsed, depth: usize) {
    if depth > 12 {
        parsed.issues.push("unsupported");
        return;
    }
    if let Some(items) = value.as_array() {
        for item in items {
            visit(source, item, parsed, depth + 1);
        }
        return;
    }
    if !value.is_object() {
        return;
    }
    let before = (
        parsed.models.len(),
        parsed.accounts.len(),
        parsed.issues.len(),
    );
    if source == "CLIProxyAPI" {
        if let Some(groups) = value.get("api-keys").and_then(Value::as_object) {
            for (provider, groups) in groups {
                if let Some(groups) = groups.as_array() {
                    for group in groups {
                        let mut group = group.clone();
                        if matches!(
                            provider.as_str(),
                            "claude" | "codex" | "openai-compatibility"
                        ) {
                            if group.get("base-url").is_none() {
                                let default = if provider == "claude" {
                                    "https://api.anthropic.com"
                                } else {
                                    "https://api.openai.com/v1"
                                };
                                group["base-url"] = json!(default);
                            }
                            if provider == "claude" {
                                group["protocol"] = json!("anthropic");
                            }
                            visit(source, &group, parsed, depth + 1);
                        } else {
                            parsed.issues.push("unsupported");
                        }
                    }
                }
            }
            return;
        }
    }
    if let Some(details) = value.get("providerSpecificData").and_then(Value::as_object) {
        let mut normalized = value.clone();
        normalized
            .as_object_mut()
            .unwrap()
            .remove("providerSpecificData");
        for (key, detail) in details {
            normalized[key] = detail.clone();
        }
        visit(source, &normalized, parsed, depth + 1);
        return;
    }
    if source == "9router"
        && text(value, &["apiKey"]).is_some()
        && text(value, &["baseUrl", "base_url"]).is_none()
    {
        let provider = text(value, &["provider"]).unwrap_or_default();
        let default = match provider {
            "openai" => Some("https://api.openai.com/v1"),
            "anthropic" => Some("https://api.anthropic.com"),
            "deepseek" => Some("https://api.deepseek.com/v1"),
            _ => None,
        };
        if let Some(default) = default {
            let mut normalized = value.clone();
            normalized["baseUrl"] = json!(default);
            if provider == "anthropic" {
                normalized["protocol"] = json!("anthropic");
            }
            visit(source, &normalized, parsed, depth + 1);
            return;
        }
    }
    if account(value, parsed) {
        return;
    }
    if let Some(contents) = text(value, &["authContents"]) {
        match serde_json::from_str::<Value>(contents) {
            Ok(auth) if account(&auth, parsed) => (),
            Ok(_) => (),
            Err(_) => parsed.issues.push("invalidConfiguration"),
        }
    }
    if let Some(relay) = value.get("relay") {
        visit(source, relay, parsed, depth + 1);
    }
    if let Some(auth) = value.get("auth").filter(|auth| auth.is_object()) {
        let mut wrapped = auth.clone();
        if let Some(profile) = value.get("profile") {
            wrapped["oauthAccount"] = profile.clone();
        }
        if account(&wrapped, parsed) {
            return;
        }
    }
    let env = value.get("env").unwrap_or(value);
    let name = text(value, &["name", "id", "provider", "user"]).unwrap_or(source);
    let key = text(
        env,
        &[
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
        ],
    )
    .or_else(|| {
        text(
            value,
            &[
                "api_key",
                "apiKey",
                "apikey",
                "key",
                "openai_api_key",
                "api-key",
            ],
        )
    });
    let anthropic = text(env, &["ANTHROPIC_BASE_URL"])
        .or_else(|| text(value, &["anthropic"]))
        .or_else(|| {
            text(env, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"])
                .map(|_| "https://api.anthropic.com")
        });
    let base = text(
        value,
        &[
            "upstreamBaseUrl",
            "api_base_url",
            "api_base",
            "base_url",
            "baseUrl",
            "api_base_url",
            "base-url",
            "chat",
            "responses",
        ],
    )
    .or_else(|| text(env, &["OPENAI_BASE_URL"]))
    .or_else(|| text(env, &["OPENAI_API_KEY"]).map(|_| "https://api.openai.com/v1"));
    let protocol = text(value, &["protocol", "baseAPI"]);
    let anthropic = anthropic.or({
        if matches!(protocol, Some("anthropic" | "claude")) {
            base
        } else {
            None
        }
    });
    let model = text(env, &["ANTHROPIC_MODEL", "OPENAI_MODEL"])
        .or_else(|| text(value, &["model", "testModel", "modelId", "defaultModel"]))
        .or_else(|| {
            value
                .get("models")
                .and_then(Value::as_array)
                .and_then(|models| models.first())
                .and_then(Value::as_str)
        });
    if let Some(key) = key {
        add_key(
            parsed,
            value,
            text(value, &["keyProtocol"]),
            (name, base, anthropic, key, model),
        );
    }
    for key_field in ["apiKeys", "keys", "credentials", "api-key-entries"] {
        if let Some(keys) = value.get(key_field).and_then(Value::as_array) {
            for key_value in keys {
                if let Some(key) = key_value.as_str().or_else(|| {
                    text(
                        key_value,
                        &["key", "apiKey", "api_key", "apikey", "api-key"],
                    )
                }) {
                    add_key(
                        parsed,
                        value,
                        text(key_value, &["protocol"]),
                        (name, base, anthropic, key, model),
                    );
                }
            }
        }
    }
    if let Some(config) = text(value, &["config", "configContents"]) {
        codex_config(value, config, name, parsed);
    }
    // Only traverse documented configuration containers, never arbitrary cached data.
    for field in [
        "providers",
        "Providers",
        "providerConnections",
        "relayProfiles",
        "claudeRelayProfiles",
        "grokRelayProfiles",
        "accounts",
        "stored",
        "settingsConfig",
        "settings_config",
    ] {
        if let Some(child) = value.get(field) {
            if child.is_array() {
                visit(source, child, parsed, depth + 1);
            } else if let Some(items) = child.as_object() {
                if child.get("env").is_some() || child.get("auth").is_some() {
                    visit(source, child, parsed, depth + 1);
                } else {
                    for item in items.values() {
                        visit(source, item, parsed, depth + 1);
                    }
                }
            }
        }
    }
    if source == "CLIProxyAPI" {
        for field in ["openai-compatibility", "claude-api-key", "codex-api-key"] {
            if let Some(items) = value.get(field) {
                visit(source, items, parsed, depth + 1);
            }
        }
        if let Some(groups) = value.get("api-keys").and_then(Value::as_object) {
            for (provider, groups) in groups {
                if matches!(
                    provider.as_str(),
                    "claude" | "codex" | "openai-compatibility"
                ) {
                    visit(source, groups, parsed, depth + 1);
                } else {
                    parsed.issues.push("unsupported");
                }
            }
        }
    }
    if before
        == (
            parsed.models.len(),
            parsed.accounts.len(),
            parsed.issues.len(),
        )
        && ["id", "email", "ciphertext", "token_encrypted", "authType"]
            .iter()
            .any(|field| value.get(field).is_some())
    {
        parsed.issues.push("unsupported");
    }
}

fn add_model(
    parsed: &mut Parsed,
    name: &str,
    base: Option<&str>,
    anthropic: Option<&str>,
    key: &str,
    model: Option<&str>,
) {
    let Some(base) = base else {
        parsed.issues.push("invalidConfiguration");
        return;
    };
    let Ok(url) = reqwest::Url::parse(base) else {
        parsed.issues.push("invalidConfiguration");
        return;
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        parsed.issues.push("invalidConfiguration");
        return;
    }
    // Importing a manager's gateway would make EchoBird depend on the source app.
    if url.host_str().is_some_and(|host| {
        matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1") || host.ends_with(".localhost")
    }) {
        parsed.issues.push("localProxy");
        return;
    }
    if key.trim().is_empty() || key.starts_with('$') || key.starts_with("enc:") {
        parsed.issues.push("unsupported");
        return;
    }
    parsed.models.push(AddModelInput {
        name: Some(name.to_owned()),
        base_url: Some(base.trim_end_matches('/').to_owned()),
        anthropic_url: anthropic.map(str::to_owned),
        api_key: Some(key.trim().to_owned()),
        model_id: model.map(str::to_owned),
        ..Default::default()
    });
}

fn add_key(
    parsed: &mut Parsed,
    provider: &Value,
    protocol: Option<&str>,
    fields: (&str, Option<&str>, Option<&str>, &str, Option<&str>),
) {
    let (name, base, anthropic, key, model) = fields;
    let (base, anthropic) = match protocol {
        Some("anthropic") => (anthropic.or(base), anthropic.or(base)),
        Some("chat") => (text(provider, &["chat"]).or(base), None),
        Some("responses") => (text(provider, &["responses"]).or(base), None),
        None => (base.or(anthropic), anthropic),
        _ => {
            parsed.issues.push("unsupported");
            return;
        }
    };
    add_model(parsed, name, base, anthropic, key, model);
}

fn codex_config(value: &Value, config: &str, name: &str, parsed: &mut Parsed) {
    let Ok(document) = config.parse::<toml_edit::DocumentMut>() else {
        parsed.issues.push("invalidConfiguration");
        return;
    };
    let model = document.get("model").and_then(toml_edit::Item::as_str);
    let active_provider = document
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .unwrap_or("openai");
    let auth = value.get("auth");
    let legacy_auth = text(value, &["authContents"])
        .and_then(|contents| serde_json::from_str::<Value>(contents).ok());
    let key = auth
        .and_then(|auth| text(auth, &["OPENAI_API_KEY"]))
        .or_else(|| text(value, &["apiKey", "api_key"]))
        .or_else(|| {
            legacy_auth
                .as_ref()
                .and_then(|auth| text(auth, &["OPENAI_API_KEY"]))
        });
    if let Some(providers) = document
        .get("model_providers")
        .and_then(toml_edit::Item::as_table_like)
    {
        for (id, provider) in providers.iter() {
            let base = provider.get("base_url").and_then(toml_edit::Item::as_str);
            let key = provider
                .get("experimental_bearer_token")
                .and_then(toml_edit::Item::as_str)
                .or(if id == active_provider { key } else { None });
            if let Some(key) = key {
                add_model(parsed, name, base, None, key, model);
            } else if provider.get("env_key").is_some() {
                parsed.issues.push("unsupported");
            }
        }
    } else if let Some(key) = key {
        add_model(
            parsed,
            name,
            Some("https://api.openai.com/v1"),
            None,
            key,
            model,
        );
    }
}

fn account(value: &Value, parsed: &mut Parsed) -> bool {
    let kind = text(value, &["type", "provider", "agent"]).unwrap_or_default();
    if kind == "antigravity" && text(value, &["refresh_token", "refreshToken"]).is_some() {
        if let Some(id) = text(value, &["id_token", "idToken"]) {
            parsed.accounts.push(Account::Antigravity(
                json!({"id_token":id,"token":{
                    "refresh_token":text(value,&["refresh_token","refreshToken"]),
                    "access_token":text(value,&["access_token","accessToken"]),
                    "expiry":text(value,&["expiry","expired"])
                }})
                .to_string(),
            ));
        } else {
            parsed.issues.push("loginRequired");
        }
        return true;
    }
    if matches!(kind, "claude" | "claude-code" | "anthropic") {
        if let Some(access) = text(value, &["access_token", "accessToken"]) {
            if let Some(email) = text(value, &["email", "user"]) {
                parsed.accounts.push(Account::Claude(
                    json!({"claudeAiOauth":{
                        "accessToken":access,
                        "refreshToken":text(value,&["refresh_token","refreshToken"]),
                        "expiresAt":value.get("expiresAt").cloned().unwrap_or(Value::Null),
                        "scopes":value.get("scopes").cloned().unwrap_or(Value::Null)
                    }}),
                    json!({"oauthAccount":{
                        "emailAddress":email,
                        "accountUuid":text(value,&["accountUuid","account_id"]),
                        "organizationUuid":text(value,&["organizationUuid","organization_id"])
                    }}),
                ));
            } else {
                parsed.issues.push("loginRequired");
            }
            return true;
        }
    }
    if value
        .get("tokens")
        .and_then(|t| text(t, &["access_token"]))
        .is_some()
    {
        parsed
            .accounts
            .push(Account::Codex(json!({"tokens":value["tokens"]})));
        return true;
    }
    if let Some(oauth) = value.get("claudeAiOauth") {
        let profile = value.get("oauthAccount").cloned().unwrap_or(Value::Null);
        if profile.is_null() {
            parsed.issues.push("loginRequired");
        } else {
            parsed.accounts.push(Account::Claude(
                json!({"claudeAiOauth":oauth}),
                json!({"oauthAccount":profile}),
            ));
        }
        return true;
    }
    let kind = text(value, &["type", "provider", "agent"]).unwrap_or_default();
    if matches!(kind, "codex" | "chatgpt")
        && text(value, &["access_token", "accessToken"]).is_some()
    {
        parsed.accounts.push(Account::Codex(json!({"tokens":{
            "access_token":text(value,&["access_token","accessToken"]),
            "refresh_token":text(value,&["refresh_token","refreshToken"]),
            "id_token":text(value,&["id_token","idToken"]),
            "account_id":text(value,&["account_id","accountId"])
        }})));
        return true;
    }
    if value
        .get("token")
        .and_then(|t| t.get("refresh_token"))
        .is_some()
    {
        let token = &value["token"];
        if let Some(id) = text(token, &["id_token"]).or_else(|| text(value, &["id_token"])) {
            parsed.accounts.push(Account::Antigravity(
                json!({"id_token":id,"token":token}).to_string(),
            ));
        } else {
            parsed.issues.push("loginRequired");
        }
        return true;
    }
    if text(
        value,
        &[
            "accessToken",
            "access_token",
            "refreshToken",
            "refresh_token",
        ],
    )
    .is_some()
    {
        parsed.issues.push("loginRequired");
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_restricted_keys_keep_their_own_endpoints() {
        let parsed = parse(
            "magpie",
            &json!({"chat":"https://chat.example/v1","responses":"https://responses.example/v1","anthropic":"https://anthropic.example","key":"primary","keyProtocol":"chat","keys":[{"key":"messages-only","protocol":"anthropic"},{"key":"responses-only","protocol":"responses"}]}),
        );
        assert_eq!(parsed.models.len(), 3);
        assert_eq!(parsed.models[0].anthropic_url, None);
        assert_eq!(
            parsed.models[1].base_url.as_deref(),
            Some("https://anthropic.example")
        );
        assert_eq!(
            parsed.models[2].base_url.as_deref(),
            Some("https://responses.example/v1")
        );
        assert_eq!(parsed.models[2].anthropic_url, None);
    }

    #[test]
    fn imports_configured_keys_from_manager_formats() {
        for (source, document, expected) in [
            (
                "9router",
                json!({"providerConnections":[{"provider":"openai-compatible-test","apiKey":"fixture-key","providerSpecificData":{"baseUrl":"https://api.example/v1"}}]}),
                1,
            ),
            (
                "cc-switch",
                json!({"env":{"ANTHROPIC_AUTH_TOKEN":"fixture-key","ANTHROPIC_BASE_URL":"https://api.example"}}),
                1,
            ),
            (
                "CodexPlusPlus",
                json!({"providers":[{"relay":{"upstreamBaseUrl":"https://api.example/v1","apiKeys":[{"apiKey":"fixture-key"},{"apiKey":"fixture-key-2"}]}}]}),
                2,
            ),
            (
                "magpie",
                json!({"providers":[{"id":"test","chat":"https://api.example/v1","key":"fixture-key","keys":[{"key":"fixture-key-2"}]}]}),
                2,
            ),
            (
                "claude-code-router",
                json!({"Providers":[{"name":"test","api_base_url":"https://api.example/v1","api_key":"fixture-key","credentials":[{"api_key":"fixture-key-2"}]}]}),
                2,
            ),
            (
                "CLIProxyAPI",
                json!({"api-keys":{"openai-compatibility":[{"name":"test","base-url":"https://api.example/v1","keys":[{"api-key":"fixture-key"}]}]}}),
                1,
            ),
            (
                "cockpit-tools",
                json!({"auth_mode":"apikey","openai_api_key":"fixture-key","api_base_url":"https://api.example/v1","tokens":{"access_token":""}}),
                1,
            ),
        ] {
            let parsed = parse(source, &document);
            assert_eq!(parsed.models.len(), expected, "{source}");
            assert!(parsed.accounts.is_empty(), "{source}");
            assert!(parsed.issues.is_empty(), "{source}");
        }
    }

    #[test]
    fn account_snapshots_are_not_treated_as_api_keys() {
        let token = json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","id_token":"fixture-id"});
        for source in [
            "9router",
            "cockpit-tools",
            "CLIProxyAPI",
            "CodexPlusPlus",
            "magpie",
        ] {
            let document = if source == "CodexPlusPlus" {
                json!({"authContents":json!({"tokens":token}).to_string()})
            } else if source == "magpie" {
                json!([{"agent":"codex","auth":{"tokens":token}}])
            } else {
                json!({"tokens":token})
            };
            let parsed = parse(source, &document);
            assert_eq!(parsed.accounts.len(), 1, "{source}");
            assert!(parsed.models.is_empty());
        }
        let parsed = parse("Antigravity-Manager", &json!({"token":token}));
        assert_eq!(parsed.accounts.len(), 1);
    }

    #[test]
    fn saved_claude_profile_and_credentials_remain_paired() {
        let parsed = parse(
            "magpie",
            &json!([{"agent":"claude","auth":{"claudeAiOauth":{"accessToken":"fixture"}},"profile":{"emailAddress":"a@example.test","organizationUuid":"org"}}]),
        );
        let Account::Claude(credentials, config) = &parsed.accounts[0] else {
            panic!("wrong account type")
        };
        assert_eq!(credentials["claudeAiOauth"]["accessToken"], "fixture");
        assert_eq!(config["oauthAccount"]["organizationUuid"], "org");
        let incomplete = parse(
            "magpie",
            &json!({"claudeAiOauth":{"accessToken":"fixture"}}),
        );
        assert!(incomplete.accounts.is_empty());
        assert_eq!(incomplete.issues, ["loginRequired"]);
    }

    #[test]
    fn codex_toml_and_auth_key_are_read_together() {
        let parsed = parse(
            "cc-switch",
            &json!({"config":"model = 'fixture-model'\nmodel_provider = 'fixture'\n[model_providers.fixture]\nbase_url = 'https://api.example/v1'\nwire_api = 'responses'", "auth":{"OPENAI_API_KEY":"fixture-key"}}),
        );
        assert_eq!(parsed.models.len(), 1);
        assert_eq!(parsed.models[0].model_id.as_deref(), Some("fixture-model"));
        let builtin = parse(
            "cc-switch",
            &json!({"config":"model = 'fixture-model'", "auth":{"OPENAI_API_KEY":"fixture-key"}}),
        );
        assert_eq!(
            builtin.models[0].base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        let builtin = parse(
            "cc-switch",
            &json!({"env":{"ANTHROPIC_API_KEY":"fixture-key"}}),
        );
        assert_eq!(
            builtin.models[0].anthropic_url.as_deref(),
            Some("https://api.anthropic.com")
        );
    }

    #[test]
    fn shared_codex_key_is_bound_only_to_the_selected_provider() {
        let parsed = parse(
            "cc-switch",
            &json!({"config":"model_provider = 'selected'\n[model_providers.aaa]\nbase_url = 'https://other.example/v1'\n[model_providers.selected]\nbase_url = 'https://selected.example/v1'\n[model_providers.separate]\nbase_url = 'https://separate.example/v1'\nexperimental_bearer_token = 'separate-key'", "auth":{"OPENAI_API_KEY":"selected-key"}}),
        );
        assert_eq!(parsed.models.len(), 2);
        let selected = parsed
            .models
            .iter()
            .find(|model| model.api_key.as_deref() == Some("selected-key"))
            .unwrap();
        assert_eq!(
            selected.base_url.as_deref(),
            Some("https://selected.example/v1")
        );
        assert!(parsed
            .models
            .iter()
            .all(|model| model.base_url.as_deref() != Some("https://other.example/v1")));
    }

    #[test]
    fn local_gateways_and_unknown_credentials_are_visible_skips() {
        let parsed = parse(
            "magpie",
            &json!({"providers":[{"chat":"http://127.0.0.1:1234/v1","key":"fixture"},{"chat":"https://api.example","key":"enc:encrypted"},{"chat":"https://api.example","key":"valid-fixture"}]}),
        );
        assert_eq!(parsed.models.len(), 1);
        assert_eq!(parsed.issues, ["localProxy", "unsupported"]);
    }
}
