//! Model configuration for kimicode.

use super::{
    ensure_parent, toml_write_table_value, toml_write_table_value_raw, toml_write_top, ApplyResult,
    ModelInfo,
};
use std::fs;
use std::path::PathBuf;

// ════════════════════════════════════════════════════════════════
//  Kimi Code (MoonshotAI/kimi-code) — TOML config at ~/.kimi-code/config.toml
//   Schema (https://moonshotai.github.io/kimi-code/en/configuration/config-files):
//     default_model = "<alias>"              (top-level scalar)
//     [providers.<name>]   type / base_url / api_key / custom_headers / env
//     [models.<alias>]     provider / model / max_context_size / ...
//   Provider type maps from protocol: anthropic → "anthropic", else "openai".
//   We register a single "echobird" provider + "echobird" model alias, point
//   default_model at it, and write the api_key into the provider entry (the
//   CLI reads credentials ONLY from config — no shell-env fallback, so we
//   cannot inject via env). Surgical string-level edits (toml_write_top /
//   toml_write_table_value[_raw]) preserve the user's comments, thinking,
//   loop_control, permission, hooks, and any unrelated providers/models.
//   KIMI_CODE_HOME env overrides the whole data dir. CLI and Desktop share
//   this file, so changing the selected model in either surface affects both.
// ════════════════════════════════════════════════════════════════

const DEFAULT_CONTEXT_SIZE: &str = "1000000";

fn kimicode_dir() -> PathBuf {
    // KIMI_CODE_HOME overrides the whole data dir; the file is always
    // config.toml regardless (per the docs).
    if let Ok(home) = std::env::var("KIMI_CODE_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    dirs::home_dir().unwrap_or_default().join(".kimi-code")
}

fn kimicode_config_path() -> PathBuf {
    kimicode_dir().join("config.toml")
}

pub(super) fn apply_kimicode(model_info: &ModelInfo) -> ApplyResult {
    apply_kimi(model_info, "Kimi CLI")
}

pub(super) fn apply_kimidesktop(model_info: &ModelInfo) -> ApplyResult {
    apply_kimi(model_info, "Kimi Desktop")
}

fn apply_kimi(model_info: &ModelInfo, surface: &str) -> ApplyResult {
    apply_kimi_at(&kimicode_config_path(), model_info, surface)
}

fn apply_kimi_at(
    config_path: &std::path::Path,
    model_info: &ModelInfo,
    surface: &str,
) -> ApplyResult {
    let model_id = model_info
        .model
        .as_deref()
        .or(model_info.name.as_deref())
        .unwrap_or("");
    if model_id.is_empty() {
        return ApplyResult {
            success: false,
            message: format!("Model ID is empty, cannot apply {surface} config"),
        };
    }

    let provider_type = if model_info.protocol.as_deref() == Some("anthropic") {
        "anthropic"
    } else {
        "openai"
    };
    let endpoint = if provider_type == "anthropic" {
        model_info
            .anthropic_url
            .as_deref()
            .or(model_info.base_url.as_deref())
    } else {
        model_info.base_url.as_deref()
    };
    let mut base_url = endpoint
        .unwrap_or(if provider_type == "anthropic" {
            "https://api.anthropic.com"
        } else {
            "https://api.openai.com/v1"
        })
        .trim_end_matches('/')
        .to_string();
    // Kimi's Anthropic provider appends /v1/messages. Accept either the base
    // URL or a complete endpoint from Model Nexus without producing /v1/v1.
    if provider_type == "anthropic" {
        if let Some(base) = base_url.strip_suffix("/v1/messages") {
            base_url = base.trim_end_matches('/').to_string();
        } else if let Some(base) = base_url.strip_suffix("/v1") {
            base_url = base.trim_end_matches('/').to_string();
        }
    }

    let api_key = model_info.api_key.as_deref().unwrap_or("");
    let provider = "echobird";
    let alias = "echobird"; // model alias == default_model value
    let providers_table = format!("providers.{}", provider);
    let models_table = format!("models.{}", alias);

    ensure_parent(config_path);
    let mut content = fs::read_to_string(config_path).unwrap_or_default();

    // Top-level default_model → our alias.
    content = toml_write_top(&content, "default_model", alias);

    // [providers.echobird]
    content = toml_write_table_value(&content, &providers_table, "type", provider_type);
    content = toml_write_table_value(&content, &providers_table, "base_url", &base_url);
    content = toml_write_table_value(&content, &providers_table, "api_key", api_key);

    // Match Kimi Desktop's provider form defaults. Both supplied protocol
    // samples use a 1M context with tool use + thinking enabled.
    content = toml_write_table_value(&content, &models_table, "provider", provider);
    content = toml_write_table_value(&content, &models_table, "model", model_id);
    content = toml_write_table_value_raw(
        &content,
        &models_table,
        "max_context_size",
        DEFAULT_CONTEXT_SIZE,
    );
    content = toml_write_table_value_raw(
        &content,
        &models_table,
        "capabilities",
        "[\"tool_use\", \"thinking\"]",
    );
    content = toml_write_table_value_raw(&content, &models_table, "adaptive_thinking", "true");

    if let Err(e) = fs::write(config_path, &content) {
        return ApplyResult {
            success: false,
            message: format!("{surface} config error: {e}"),
        };
    }

    log::info!(
        "[ToolConfigManager] {surface} configured: provider={}, alias={}, model={}, type={}",
        provider,
        alias,
        model_id,
        provider_type
    );
    ApplyResult {
        success: true,
        message: format!(
            "Model \"{}\" configured for {surface}. Kimi CLI and Desktop share this selection; restart the active client to load it.",
            model_info.name.as_deref().unwrap_or(model_id),
        ),
    }
}

pub(super) fn read_kimicode() -> Option<ModelInfo> {
    read_kimi_at(&kimicode_config_path())
}

pub(super) fn read_kimidesktop() -> Option<ModelInfo> {
    read_kimicode()
}

fn read_kimi_at(config_path: &std::path::Path) -> Option<ModelInfo> {
    let content = fs::read_to_string(config_path).ok()?;
    if content.trim().is_empty() {
        return None;
    }

    // Collect tables as (header, [(key, value), ...]) + track the top-level
    // default_model scalar. Values are stripped of inline comments and
    // surrounding quotes — sufficient for the simple scalars we own.
    let mut tables: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut cur: Option<(String, Vec<(String, String)>)> = None;
    let mut top_default_model: Option<String> = None;
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(t) = cur.take() {
                tables.push(t);
            }
            let header = line.trim_matches(|c| c == '[' || c == ']').to_string();
            cur = Some((header, Vec::new()));
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim().to_string();
        let v = v
            .split('#')
            .next()
            .unwrap_or(v)
            .trim()
            .trim_matches('"')
            .to_string();
        if let Some((_, kvs)) = cur.as_mut() {
            kvs.push((k, v));
        } else if k == "default_model" {
            top_default_model = Some(v);
        }
    }
    if let Some(t) = cur.take() {
        tables.push(t);
    }

    // Only round-trip when our echobird alias is the active default. If the
    // user switched to a managed/native model via /model, return None so the
    // model-picker shows nothing selected (correct — not an EchoBird model).
    let alias = top_default_model?;
    if alias != "echobird" {
        return None;
    }

    let prov = tables.iter().find(|(h, _)| h == "providers.echobird")?;
    let model = tables.iter().find(|(h, _)| h == "models.echobird")?;
    let model_id = model
        .1
        .iter()
        .find(|(k, _)| k == "model")
        .map(|(_, v)| v.clone())?;
    if model_id.is_empty() {
        return None;
    }

    let provider_type = prov
        .1
        .iter()
        .find(|(k, _)| k == "type")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let base_url = prov
        .1
        .iter()
        .find(|(k, _)| k == "base_url")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let api_key = prov
        .1
        .iter()
        .find(|(k, _)| k == "api_key")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let (base, anthro, protocol) = if provider_type == "anthropic" {
        (None, Some(base_url), "anthropic")
    } else {
        (Some(base_url), None, "openai")
    };
    Some(ModelInfo {
        name: Some(model_id.clone()),
        model: Some(model_id),
        base_url: base,
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        anthropic_url: anthro,
        protocol: Some(protocol.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        one_m_context: None,
    })
}

pub(super) fn restore_kimicode_to_official() -> ApplyResult {
    restore_kimi("Kimi CLI")
}

pub(super) fn restore_kimidesktop_to_official() -> ApplyResult {
    restore_kimi("Kimi Desktop")
}

fn restore_kimi(surface: &str) -> ApplyResult {
    restore_kimi_at(&kimicode_config_path(), surface)
}

fn restore_kimi_at(config_path: &std::path::Path, surface: &str) -> ApplyResult {
    let content = fs::read_to_string(config_path).unwrap_or_default();
    if content.trim().is_empty() {
        return ApplyResult {
            success: true,
            message: format!("{surface} config not found — already at official."),
        };
    }

    // Remove our [providers.echobird] + [models.echobird] table blocks and the
    // default_model line we set. Preserve everything else (user providers,
    // managed:kimi-code OAuth entry, thinking, hooks, permission, …). On next
    // launch Kimi Code falls back to /login (OAuth or Moonshot platform key).
    let new_content = remove_toml_table(&content, "providers.echobird");
    let new_content = remove_toml_table(&new_content, "models.echobird");
    let new_content = remove_toml_top_key_if_value(&new_content, "default_model", "echobird");

    if new_content != content {
        if let Err(e) = fs::write(config_path, &new_content) {
            return ApplyResult {
                success: false,
                message: format!("{surface} restore error: {e}"),
            };
        }
    }

    ApplyResult {
        success: true,
        message: format!("{surface} restored — EchoBird's shared provider/model was removed without changing a model selected later in Kimi."),
    }
}

/// Remove a `[table]` block (header + its key=value lines, up to the next
/// section header or EOF). Returns content unchanged if the table is absent.
fn remove_toml_table(content: &str, table: &str) -> String {
    let header = format!("[{}]", table);
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let start = lines.iter().position(|l| l.trim() == header.as_str());
    let Some(start) = start else {
        return content.to_string();
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(i, l)| {
            let t = l.trim();
            if t.starts_with('[') && t.ends_with(']') {
                Some(i)
            } else {
                None
            }
        })
        .unwrap_or(lines.len());
    lines.drain(start..end);
    let mut joined = lines.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// Remove a top-level `key = ...` line (before any section header).
fn remove_toml_top_key_if_value(content: &str, key: &str, expected: &str) -> String {
    let mut first_section: Option<usize> = None;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut remove: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if first_section.is_none() && t.starts_with('[') {
            first_section = Some(i);
        }
        if first_section.is_some() {
            break;
        }
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some((k, value)) = t.split_once('=') {
            if k.trim() == key && value.trim().trim_matches('"') == expected {
                remove = Some(i);
                break;
            }
        }
    }
    if let Some(i) = remove {
        lines.remove(i);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("echobird-kimi-config-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path.join("config.toml"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    fn model(protocol: &str, base_url: &str) -> ModelInfo {
        let mut value = serde_json::json!({
            "name": "Test model",
            "model": "vendor/model",
            "apiKey": "test-only",
            "protocol": protocol
        });
        value[if protocol == "anthropic" {
            "anthropicUrl"
        } else {
            "baseUrl"
        }] = serde_json::Value::String(base_url.to_string());
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn writes_openai_shape_used_by_cli_and_desktop() {
        let f = Fixture::new();
        assert!(
            apply_kimi_at(
                &f.0,
                &model("openai", "https://example.com/v1/"),
                "Kimi CLI"
            )
            .success
        );
        let content = fs::read_to_string(&f.0).unwrap();
        assert!(content.contains("type = \"openai\""));
        assert!(content.contains("base_url = \"https://example.com/v1\""));
        assert!(content.contains("max_context_size = 1000000"));
        assert!(content.contains("capabilities = [\"tool_use\", \"thinking\"]"));
        assert!(content.contains("adaptive_thinking = true"));
        assert_eq!(
            read_kimi_at(&f.0).unwrap().protocol.as_deref(),
            Some("openai")
        );
    }

    #[test]
    fn writes_anthropic_and_normalizes_complete_endpoint() {
        let f = Fixture::new();
        assert!(
            apply_kimi_at(
                &f.0,
                &model("anthropic", "https://example.com/anthropic/v1/messages/"),
                "Kimi Desktop"
            )
            .success
        );
        let content = fs::read_to_string(&f.0).unwrap();
        assert!(content.contains("type = \"anthropic\""));
        assert!(content.contains("base_url = \"https://example.com/anthropic\""));
        let selected = read_kimi_at(&f.0).unwrap();
        assert_eq!(selected.protocol.as_deref(), Some("anthropic"));
        assert_eq!(
            selected.anthropic_url.as_deref(),
            Some("https://example.com/anthropic")
        );
    }

    #[test]
    fn restore_only_clears_the_echobird_default() {
        for (default_model, should_remain) in [("echobird", false), ("kimi-code/k3", true)] {
            let f = Fixture::new();
            fs::write(
                &f.0,
                format!(
                    "default_model = \"{default_model}\"\n\n[providers.echobird]\ntype = \"openai\"\n\n[models.echobird]\nprovider = \"echobird\"\n\n[providers.official]\ntype = \"kimi\"\n"
                ),
            )
            .unwrap();
            assert!(restore_kimi_at(&f.0, "Kimi CLI").success);
            let content = fs::read_to_string(&f.0).unwrap();
            assert!(!content.contains("[providers.echobird]"));
            assert!(!content.contains("[models.echobird]"));
            assert_eq!(
                content.contains(&format!("default_model = \"{default_model}\"")),
                should_remain
            );
            assert!(content.contains("[providers.official]"));
        }
    }
}
