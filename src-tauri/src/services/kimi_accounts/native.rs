use super::{account_id, Account, Region, Saved, Token};
use crate::services::native_account_files::Transaction;
use serde_json::Value;
use std::{fs, path::Path};
use toml_edit::{value, Array, DocumentMut, InlineTable, Item, Table};

const PROVIDER: &str = "managed:kimi-code";

fn token_at(dir: &Path, region: Region) -> Option<Token> {
    let name = region.key().strip_prefix("oauth/")?.to_string();
    super::super::cursor_auth::read(&dir.join("credentials").join(format!("{name}.json"))).ok()
}

fn token_id(token: &Token, region: Region) -> Option<String> {
    let claims = super::super::cursor_auth::claims(&token.access_token).ok()?;
    let user = claims["user_id"]
        .as_str()
        .or_else(|| claims["sub"].as_str())?;
    Some(account_id(region, user))
}

pub(super) fn token_for(dir: &Path, account: &Account) -> Option<Token> {
    let token = token_at(dir, account.region)?;
    (token_id(&token, account.region).as_deref() == Some(account.id.as_str())).then_some(token)
}

/// Keeps the client's copy valid after EchoBird spends a single-use refresh token.
pub(super) fn store_token(dir: &Path, region: Region, token: &Token) -> Result<(), String> {
    let name = region.key().trim_start_matches("oauth/").to_string();
    let mut transaction = Transaction::new();
    transaction.replace(
        &dir.join("credentials").join(format!("{name}.json")),
        &serde_json::to_vec_pretty(token).map_err(|_| "accountError.format")?,
    )?;
    transaction.commit();
    Ok(())
}

pub(super) fn active_id(dir: &Path) -> Option<String> {
    let document = fs::read_to_string(dir.join("config.toml"))
        .ok()?
        .parse::<DocumentMut>()
        .ok()?;
    let model = document.get("default_model")?.as_str()?;
    if document
        .get("models")?
        .get(model)?
        .get("provider")?
        .as_str()?
        != PROVIDER
    {
        return None;
    }
    let provider = document.get("providers")?.get(PROVIDER)?;
    let base = provider.get("base_url")?.as_str()?;
    let region = [Region::Cn, Region::Global]
        .into_iter()
        .find(|r| r.base() == base.trim_end_matches('/'))?;
    let key = provider.get("oauth")?.get("key")?.as_str()?;
    if key != region.key() {
        return None;
    }
    token_id(&token_at(dir, region)?, region)
}

pub(super) fn apply_at(dir: &Path, saved: &Saved, catalog: &Value) -> Result<(), String> {
    let path = dir.join("config.toml");
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|_| "accountError.read")?
    } else {
        String::new()
    };
    let mut document = raw
        .parse::<DocumentMut>()
        .map_err(|_| "accountError.format")?;
    for key in ["models", "providers", "services"] {
        if document
            .get(key)
            .is_some_and(|item| item.as_table_like().is_none())
        {
            return Err("accountError.format".into());
        }
    }
    let models = catalog["data"]
        .as_array()
        .filter(|m| !m.is_empty())
        .ok_or("accountError.authResponse")?;
    let mut keys = Vec::new();
    for model in models {
        let id = model["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("accountError.authResponse")?;
        let context = model["limit"]["context"]
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or("accountError.authResponse")?;
        let protocol = model["protocol"].as_str().unwrap_or("openai");
        if !matches!(protocol, "openai" | "anthropic") {
            return Err("accountError.authResponse".into());
        }
        let key = format!("kimi-code/{id}");
        let item = &mut document["models"][&key];
        if item.is_none() {
            *item = Item::Table(Table::new());
        }
        if item.as_table_like_mut().is_none() {
            return Err("accountError.format".into());
        }
        item["provider"] = value(PROVIDER);
        item["model"] = value(id);
        item["max_context_size"] = value(context);
        item["protocol"] = value(protocol);
        let mut capabilities = Array::new();
        if model["supports_tool_use"] != false {
            capabilities.push("tool_use");
        }
        let thinking = matches!(model["reasoning"]["type"].as_str(), Some("only" | "both"));
        if thinking {
            capabilities.push("thinking");
        }
        if model["reasoning"]["type"] == "only" {
            capabilities.push("always_thinking");
        }
        for (modality, capability) in [("image", "image_in"), ("video", "video_in")] {
            if model["modalities"]["input"]
                .as_array()
                .is_some_and(|inputs| inputs.iter().any(|v| v == modality))
            {
                capabilities.push(capability);
            }
        }
        item["capabilities"] = value(capabilities);
        item["adaptive_thinking"] = value(protocol == "anthropic" && thinking);
        if protocol == "anthropic" {
            item["beta_api"] = value(true);
        } else if let Some(table) = item.as_table_like_mut() {
            table.remove("beta_api");
        }
        if let Some(name) = model["display_name"].as_str() {
            item["display_name"] = value(name);
        }
        keys.push(key);
    }
    let selected = document
        .get("default_model")
        .and_then(Item::as_str)
        .filter(|k| keys.iter().any(|m| m == k))
        .unwrap_or(&keys[0])
        .to_string();
    document["default_model"] = value(selected);
    let region = saved.account.region;
    let mut oauth = InlineTable::new();
    oauth.insert("storage", "file".into());
    oauth.insert("key", region.key().into());
    if region == Region::Global {
        oauth.insert("oauth_host", region.host().into());
    }
    let provider = &mut document["providers"][PROVIDER];
    if provider.is_none() {
        *provider = Item::Table(Table::new());
    }
    if provider.as_table_like_mut().is_none() {
        return Err("accountError.format".into());
    }
    provider["type"] = value("kimi");
    provider["base_url"] = value(region.base());
    provider["api_key"] = value("");
    provider["oauth"] = value(oauth.clone());
    for (service, path) in [("moonshot_search", "search"), ("moonshot_fetch", "fetch")] {
        let entry = &mut document["services"][service];
        if entry.is_none() {
            *entry = Item::Table(Table::new());
        }
        if entry.as_table_like_mut().is_none() {
            return Err("accountError.format".into());
        }
        entry["base_url"] = value(format!("{}/{path}", region.base()));
        entry["api_key"] = value("");
        entry["oauth"] = value(oauth.clone());
    }
    let name = region.key().trim_start_matches("oauth/").to_string();
    let mut transaction = Transaction::new();
    transaction.replace(
        &dir.join("credentials").join(format!("{name}.json")),
        &serde_json::to_vec_pretty(&saved.token).map_err(|_| "accountError.format")?,
    )?;
    transaction.replace_private_file(&path, document.to_string().as_bytes())?;
    transaction.commit();
    Ok(())
}

pub(super) fn discover(dir: &Path, region: Region) -> Option<Token> {
    token_at(dir, region)
}
