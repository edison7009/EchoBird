// Codex model-catalog generation — per-vendor capability templates, embedded
// at compile time via `include_str!` and matched to the selected provider by
// its base_url domain.
//
// Why this exists: when Codex talks to a third-party Responses endpoint
// directly, `apply_codex` writes
// the provider's REAL base_url + REAL model id into `~/.codex/config.toml`.
// For those direct connections Codex needs a model catalog
// (`model_catalog_json = "<path>"` → a JSON file declaring the model's context
// window, reasoning levels, tool capabilities, and base prompt); without it
// Codex doesn't know the model — it mis-sizes the context window and can't
// register the model's tools.
//
// Published catalogs declare exact model capabilities; legacy capability
// templates remain the fallback for previously supported vendors' unknown
// models. `template_for_model` selects by native provider URL and exact ID,
// then `build_catalog` stamps the selected identity/custom name and emits a
// single-entry `{"models":[...]}`. Unknown versions retain the existing
// conservative behavior. Never choose a native catalog by model brand alone:
// a reseller may have different context windows or tool capabilities.
//
// Vendors we do NOT bundle keep the current behavior: no `model_catalog_json`
// line, Codex talks to the upstream directly with the real id (its own default
// catalog applies). If a vendor's model doesn't support the Responses protocol
// at all, it cannot be used by Codex through EchoBird.
// Official documentation URLs, review date, and upstream catalog hashes are
// recorded in assets/codex-catalogs/sources.json for future updates.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use url::Url;

/// DeepSeek's Codex capability template. `base_instructions` /
/// `model_messages.instructions_template` carry the full model-agnostic Codex
/// agent prompt framework + `apply_patch_tool_type: "freeform"` (extracted
/// verbatim from DeepSeek's official setup script). Model identity and the
/// selected model's image capability are stamped by `build_catalog`.
pub const DEEPSEEK_TEMPLATE: &str = include_str!("../../assets/codex-catalogs/deepseek.json");

/// MiniMax Codex capability template (adaptive thinking, 1M window, text +
/// image). `base_instructions` uses a `{model}` placeholder filled with the
/// selected display name.
pub const MINIMAX_TEMPLATE: &str = include_str!("../../assets/codex-catalogs/minimax.json");

/// Xiaomi MiMo Codex capability template (1M window, NO web_search tool —
/// MiMo rejects it with a hard 400). Model identity and the selected model's
/// image capability are stamped by `build_catalog`.
pub const MIMO_TEMPLATE: &str = include_str!("../../assets/codex-catalogs/mimo.json");

// Published model-specific catalogs. Legacy templates above remain the
// fallback for undocumented MiniMax/MiMo IDs; they must not inherit a new
// model's vision, reasoning, or Responses Lite capabilities by name alone.
const MINIMAX_MODELS: &str = include_str!("../../assets/codex-catalogs/minimax-models.json");
const MIMO_MODELS: &str = include_str!("../../assets/codex-catalogs/mimo-models.json");
const GLM_CN_MODELS: &str = include_str!("../../assets/codex-catalogs/glm-cn-models.json");
const GLM_GLOBAL_MODELS: &str = include_str!("../../assets/codex-catalogs/glm-global-models.json");
const QWEN_PLAN_MODELS: &str = include_str!("../../assets/codex-catalogs/qwen-plan-models.json");
const QWEN_PAYG_MODELS: &str = include_str!("../../assets/codex-catalogs/qwen-payg-models.json");
const STEP_PLAN_MODELS: &str = include_str!("../../assets/codex-catalogs/stepfun-plan-models.json");

pub fn is_step_plan_url(base_url: &str) -> bool {
    url_matches_domain(base_url, "stepfun.com")
        && Url::parse(base_url)
            .ok()
            .is_some_and(|url| url.path().trim_end_matches('/') == "/step_plan/v1")
}

/// Match a provider base_url to the bundled capability template that applies.
/// Domain-only, never by model brand. Returns the template JSON string, or
/// `None` for vendors we don't bundle (keep the existing no-catalog path).
pub fn url_matches_domain(base_url: &str, domain: &str) -> bool {
    let Some(host) = Url::parse(base_url).ok().and_then(|url| {
        url.host_str()
            .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
    }) else {
        return false;
    };
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub fn template_for_url(base_url: &str) -> Option<&'static str> {
    if url_matches_domain(base_url, "deepseek.com") {
        Some(DEEPSEEK_TEMPLATE)
    } else if url_matches_domain(base_url, "minimax.cn")
        || url_matches_domain(base_url, "minimaxi.com")
        || url_matches_domain(base_url, "minimax.io")
    {
        Some(MINIMAX_TEMPLATE)
    } else if url_matches_domain(base_url, "xiaomimimo.com") {
        Some(MIMO_TEMPLATE)
    } else {
        None
    }
}

/// Resolve documented capabilities by both native provider and exact model.
/// A reseller's GLM/DeepSeek model must retain the reseller's catalog values.
pub fn template_for_model(base_url: &str, model_id: &str) -> Option<Value> {
    let published = if url_matches_domain(base_url, "xiaomimimo.com") {
        Some(MIMO_MODELS)
    } else if url_matches_domain(base_url, "minimax.cn")
        || url_matches_domain(base_url, "minimaxi.com")
        || url_matches_domain(base_url, "minimax.io")
    {
        Some(MINIMAX_MODELS)
    } else if url_matches_domain(base_url, "bigmodel.cn") {
        Some(GLM_CN_MODELS)
    } else if url_matches_domain(base_url, "z.ai") {
        Some(GLM_GLOBAL_MODELS)
    } else if is_step_plan_url(base_url) {
        Some(STEP_PLAN_MODELS)
    } else if url_matches_domain(base_url, "maas.qianwenaiapi.com")
        || url_matches_domain(base_url, "maas.aliyuncs.com")
    {
        let is_plan = Url::parse(base_url).ok().is_some_and(|url| {
            url.host_str()
                .is_some_and(|host| host.starts_with("token-plan."))
        });
        Some(if is_plan {
            QWEN_PLAN_MODELS
        } else {
            QWEN_PAYG_MODELS
        })
    } else {
        None
    };
    if let Some(catalog) = published.and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        if let Some(entry) = catalog["models"].as_array().and_then(|models| {
            models
                .iter()
                .find(|entry| entry["slug"].as_str() == Some(model_id))
        }) {
            let mut entry = entry.clone();
            if url_matches_domain(base_url, "xiaomimimo.com") {
                // Preserve MiMo's existing no-search guard, including when
                // the user explicitly enables web search in EchoBird.
                entry["supports_search_tool"] = json!(false);
            }
            return Some(entry);
        }
    }
    template_for_url(base_url).and_then(|s| serde_json::from_str(s).ok())
}

pub(crate) fn build_router_catalog(
    models: &[crate::services::smart_router::catalog::SwitchModel],
    web_search: bool,
) -> Value {
    // Reuse the established Codex prompt and wire capability schema. Upstream
    // capabilities are heterogeneous; keep auto and unknown rows on the legacy
    // text-only fallback. Known explicit rows retain their native capabilities.
    let mut template: Value =
        serde_json::from_str(MINIMAX_TEMPLATE).expect("bundled Codex template");
    template["input_modalities"] = json!(["text"]);
    template["supports_image_detail_original"] = json!(false);
    template["supports_reasoning_summaries"] = json!(false);
    template["supports_search_tool"] = json!(web_search);
    template["supports_parallel_tool_calls"] = json!(false);
    template["use_responses_lite"] = json!(false);
    template["supported_reasoning_levels"] = json!([
        {"effort":"low","description":"Low"},
        {"effort":"medium","description":"Medium"},
        {"effort":"high","description":"High"}
    ]);
    let entries: Vec<_> = models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let mut entry = model.codex_catalog.clone().unwrap_or_else(|| {
                build_catalog(&template, &model.id, &model.name, 1_000_000)["models"][0].clone()
            });
            entry["slug"] = json!(model.id);
            entry["display_name"] = json!(model.name);
            entry["description"] = json!(format!("{} via EchoBird", model.name));
            // The local router does not implement the separate compact API.
            entry["use_responses_lite"] = json!(false);
            entry["prefer_websockets"] = json!(false);
            entry["supports_search_tool"] =
                json!(web_search && entry["supports_search_tool"].as_bool().unwrap_or(false));
            entry["priority"] = json!(index);
            entry
        })
        .collect();
    let mut entries = entries;
    if entries.len() > 1 {
        let context = entries[1..]
            .iter()
            .filter_map(|entry| entry["context_window"].as_u64())
            .min()
            .unwrap_or(1_000_000);
        entries[0]["context_window"] = json!(context);
        entries[0]["max_context_window"] = json!(context);
        let levels = entries[0]["supported_reasoning_levels"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        entries[0]["supported_reasoning_levels"] = json!(levels
            .into_iter()
            .filter(|level| {
                entries[1..].iter().all(|entry| {
                    entry["supported_reasoning_levels"]
                        .as_array()
                        .is_some_and(|levels| {
                            levels
                                .iter()
                                .any(|candidate| candidate["effort"] == level["effort"])
                        })
                })
            })
            .collect::<Vec<_>>());
        let default = entries[0]["default_reasoning_level"].clone();
        if !entries[0]["supported_reasoning_levels"]
            .as_array()
            .is_some_and(|levels| levels.iter().any(|level| level["effort"] == default))
        {
            if let Some(effort) = entries[0]["supported_reasoning_levels"][0]["effort"]
                .as_str()
                .map(String::from)
            {
                entries[0]["default_reasoning_level"] = json!(effort);
            } else {
                entries[0]
                    .as_object_mut()
                    .unwrap()
                    .remove("default_reasoning_level");
            }
        }
        entries[0]["supports_search_tool"] = json!(
            web_search
                && entries[1..]
                    .iter()
                    .all(|entry| entry["supports_search_tool"] == true)
        );
    }
    json!({"models":entries})
}

/// Build a single-entry model catalog for the SELECTED model from a vendor
/// capability template. The template's model-agnostic fields (base_instructions
/// framework, tool types, reasoning levels, truncation policy) are preserved;
/// the model identity (slug / display_name / description / context_window /
/// priority) is stamped from the caller. Unknown model versions need no asset
/// update for text use; capabilities that are unsafe to guess are opt-in.
pub fn build_catalog(
    template: &Value,
    model_id: &str,
    display_name: &str,
    context_window: u64,
) -> Value {
    let documented_model = template["slug"].as_str() == Some(model_id);
    let mut entry = template.clone();
    entry["slug"] = json!(model_id);
    entry["display_name"] = json!(display_name);
    entry["description"] = json!(format!("{display_name} via EchoBird"));
    entry["context_window"] = json!(context_window);
    entry["max_context_window"] = json!(context_window);
    entry["priority"] = json!(0);

    // The vendor catalogs are model-specific here: DeepSeek Flash and MiMo
    // v2.5 accept images, while DeepSeek V4 Pro and MiMo v2.5 Pro are text
    // only. Keep the bundled templates conservative and opt in only the
    // model IDs documented by the vendors.
    let supports_image = matches!(model_id, "deepseek-flash" | "mimo-v2.5");
    if !documented_model && (model_id.starts_with("deepseek-") || model_id.starts_with("mimo-")) {
        entry["input_modalities"] = if supports_image {
            json!(["text", "image"])
        } else {
            json!(["text"])
        };
        entry["supports_image_detail_original"] = json!(supports_image);
    }
    if !documented_model && model_id.starts_with("deepseek-") {
        entry["supports_search_tool"] = json!(model_id == "deepseek-flash");
    }
    // MiniMax's base_instructions carries a `{model}` placeholder; substitute
    // the selected display name so the prompt names the actual model. Vendors
    // whose prompt is model-agnostic (DeepSeek) are unaffected.
    if let Some(bi) = entry["base_instructions"].as_str() {
        if bi.contains("{model}") {
            entry["base_instructions"] = json!(bi.replace("{model}", display_name));
        }
    }
    json!({ "models": [entry] })
}

/// Absolute path to the catalog file Codex reads. Uses forward slashes on
/// every platform so the value stays valid inside config.toml's basic strings
/// on Windows (backslashes would need TOML escaping). Use the same directory
/// as config.toml, including isolated configuration-application fixtures.
pub fn models_json_path(codex_dir: &Path) -> PathBuf {
    let raw = codex_dir.join("models.json");
    let raw = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(&raw))
            .unwrap_or(raw)
    };
    PathBuf::from(raw.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_for_url_matches_deepseek_domain() {
        assert_eq!(
            template_for_url("https://api.deepseek.com/v1"),
            Some(DEEPSEEK_TEMPLATE)
        );
        assert_eq!(
            template_for_url("https://api.deepseek.com"),
            Some(DEEPSEEK_TEMPLATE)
        );
        assert_eq!(template_for_url("https://notdeepseek.com/v1"), None);
    }

    #[test]
    fn template_for_url_matches_minimax_domains() {
        assert_eq!(
            template_for_url("https://api.minimax.cn/v1"),
            Some(MINIMAX_TEMPLATE)
        );
        assert_eq!(
            template_for_url("https://api.minimaxi.com/v1"),
            Some(MINIMAX_TEMPLATE)
        );
        assert_eq!(
            template_for_url("https://api.minimax.io/v1"),
            Some(MINIMAX_TEMPLATE)
        );
    }

    #[test]
    fn template_for_url_matches_mimo_domains() {
        assert_eq!(
            template_for_url("https://api.xiaomimimo.com/v1"),
            Some(MIMO_TEMPLATE)
        );
        // Token-plan regional endpoints share the same domain.
        assert_eq!(
            template_for_url("https://token-plan-cn.xiaomimimo.com/v1"),
            Some(MIMO_TEMPLATE)
        );
        assert_eq!(template_for_url("https://notxiaomimimo.com/v1"), None);
    }

    #[test]
    fn template_for_url_returns_none_for_unbundled_vendors() {
        assert_eq!(
            template_for_url("https://ark.cn-beijing.volces.com/api/coding/v1"),
            None
        );
        assert_eq!(template_for_url("https://api.openai.com/v1"), None);
        assert_eq!(template_for_url("https://api.moonshot.cn/v1"), None);
    }

    #[test]
    fn bundled_templates_parse() {
        for template in [DEEPSEEK_TEMPLATE, MINIMAX_TEMPLATE, MIMO_TEMPLATE] {
            let v: Value = serde_json::from_str(template).expect("bundled template must parse");
            assert!(
                v.get("base_instructions")
                    .and_then(|x| x.as_str())
                    .is_some(),
                "template must carry base_instructions"
            );
            assert!(
                v.get("models").is_none(),
                "template must NOT be a model list — identity is stamped at build time"
            );
        }
    }

    #[test]
    fn published_catalogs_parse_and_declare_unique_models() {
        for source in [
            MINIMAX_MODELS,
            MIMO_MODELS,
            GLM_CN_MODELS,
            GLM_GLOBAL_MODELS,
            QWEN_PLAN_MODELS,
            QWEN_PAYG_MODELS,
            STEP_PLAN_MODELS,
        ] {
            let catalog: Value = serde_json::from_str(source).unwrap();
            let models = catalog["models"].as_array().unwrap();
            let mut ids = std::collections::HashSet::new();
            assert!(!models.is_empty());
            for model in models {
                assert!(ids.insert(model["slug"].as_str().unwrap()));
                assert!(model["base_instructions"].is_string());
                assert!(model["context_window"].as_u64().unwrap() > 0);
            }
        }
    }

    #[test]
    fn native_catalogs_select_documented_models_in_both_regions() {
        for (base_url, model, context, effort) in [
            (
                "https://api.minimax.cn/v1",
                "MiniMax-M3.1-Flash",
                524_288,
                "max",
            ),
            (
                "https://api.minimax.io/v1",
                "MiniMax-M3.1-Flash",
                524_288,
                "max",
            ),
            (
                "https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1",
                "deepseek-v4.1-flash-1017",
                1_000_000,
                "medium",
            ),
            (
                "https://api.minimax.cn/v1",
                "MiniMax-M3.1-Flash-Preview",
                524_288,
                "max",
            ),
            (
                "https://api.minimax.io/v1",
                "MiniMax-M3.1-Flash-Preview",
                524_288,
                "max",
            ),
            (
                "https://api.minimaxi.com/v1",
                "MiniMax-M3.1-Flash-Preview",
                524_288,
                "max",
            ),
            (
                "https://open.bigmodel.cn/api/v1",
                "glm-5.3",
                1_048_576,
                "max",
            ),
            ("https://api.z.ai/api/v1", "glm-5.3", 1_048_576, "max"),
            (
                "https://open.bigmodel.cn/api/v1",
                "glm-5-turbo",
                204_800,
                "max",
            ),
            (
                "https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1",
                "qwen3.8-max",
                983_616,
                "xhigh",
            ),
            (
                "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
                "qwen3.8-flash",
                983_616,
                "xhigh",
            ),
            (
                "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
                "qwen3.8-max",
                983_616,
                "xhigh",
            ),
            (
                "https://workspace.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
                "qwen3.8-max",
                983_616,
                "xhigh",
            ),
        ] {
            let template = template_for_model(base_url, model).unwrap();
            assert_eq!(template["slug"], model);
            assert_eq!(template["context_window"], context);
            assert_eq!(template["default_reasoning_level"], effort);
        }
    }

    #[test]
    fn mimo_lite_and_vision_capabilities_remain_model_specific() {
        for base_url in [
            "https://api.xiaomimimo.com/v1",
            "https://token-plan-cn.xiaomimimo.com/v1",
        ] {
            for (model, lite, vision) in [
                ("mimo-v2.6-pro", true, true),
                ("mimo-v2.6-flash", true, true),
                ("mimo-v2.6-pro-ultraspeed", true, true),
                ("mimo-v2.5-pro", false, false),
                ("mimo-v2.5", false, true),
            ] {
                let template = template_for_model(base_url, model).unwrap();
                let catalog = build_catalog(&template, model, "Custom name", 1_048_576);
                let entry = &catalog["models"][0];
                assert_eq!(entry["use_responses_lite"], lite);
                assert_eq!(
                    entry["input_modalities"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("image")),
                    vision
                );
                assert_eq!(entry["supports_search_tool"], false);
                assert_eq!(entry["shell_type"], "unified_exec");
                assert_eq!(
                    entry["supported_reasoning_levels"]
                        .as_array()
                        .unwrap()
                        .len(),
                    4
                );
                assert_eq!(entry["model_messages"], template["model_messages"]);
                if lite {
                    assert_eq!(entry["apply_patch_tool_type"], "freeform");
                    assert_eq!(entry["tool_mode"], "code_mode_only");
                }
            }
        }
    }

    #[test]
    fn qwen_hosted_models_keep_qwen_context_and_capabilities() {
        let base_url = "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1";
        for (model, context, vision) in [
            ("deepseek-v4-pro", 163_840, false),
            ("deepseek-v4.1-flash", 1_000_000, true),
            ("glm-5.3", 1_000_000, false),
        ] {
            let template = template_for_model(base_url, model).unwrap();
            let catalog = build_catalog(&template, model, model, context);
            let entry = &catalog["models"][0];
            assert_eq!(entry["context_window"], context);
            assert_eq!(entry["supports_reasoning_summaries"], false);
            assert_eq!(entry["supports_parallel_tool_calls"], false);
            assert_eq!(
                entry["input_modalities"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("image")),
                vision
            );
            assert!(entry.get("supports_search_tool").is_none());
        }
    }

    #[test]
    fn step_plan_capabilities_are_model_specific() {
        for (model, context, modalities) in [
            ("step-5-preview", 1_000_000, json!(["text", "image"])),
            ("step-3.7-flash", 256_000, json!(["text", "image"])),
            ("step-3.5-flash", 256_000, json!(["text"])),
        ] {
            let template =
                template_for_model("https://api.stepfun.com/step_plan/v1/", model).unwrap();
            let catalog = build_catalog(&template, model, model, context);
            let entry = &catalog["models"][0];
            assert_eq!(entry["context_window"], context);
            assert_eq!(entry["input_modalities"], modalities);
            assert_eq!(entry["default_reasoning_level"], "medium");
            assert_eq!(entry["supports_search_tool"], false);
            assert_eq!(entry["supports_parallel_tool_calls"], false);
            assert_eq!(entry["apply_patch_tool_type"], "freeform");
            assert_eq!(
                entry["supported_reasoning_levels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|level| level["effort"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["low", "medium", "high"]
            );
            assert!(entry["base_instructions"].as_str().unwrap().len() > 1000);
        }
    }

    #[test]
    fn undocumented_models_and_resellers_do_not_inherit_new_capabilities() {
        for (base_url, model) in [
            ("https://api.stepfun.com/v1", "step-5-preview"),
            ("https://api.stepfun.com/step_plan/v1", "step-future"),
            ("https://reseller.example/step_plan/v1", "step-5-preview"),
            (
                "https://api.stepfun.com.evil.test/step_plan/v1",
                "step-5-preview",
            ),
            ("https://reseller.example/v1", "glm-5.3"),
            ("https://reseller.example/v1", "mimo-v2.6-pro"),
            ("https://api.z.ai/api/v1", "glm-5-turbo"),
            ("https://open.bigmodel.cn/api/v1", "glm-future"),
            (
                "https://maas.qianwenaiapi.com/compatible-mode/v1",
                "qwen3.8-flash",
            ),
            (
                "https://token-plan.maas.aliyuncs.com.evil.test/v1",
                "qwen3.8-max",
            ),
            (
                "https://coding-intl.dashscope.aliyuncs.com/v1",
                "qwen3.7-plus",
            ),
        ] {
            assert!(template_for_model(base_url, model).is_none());
        }
        for (base_url, model) in [
            ("https://api.xiaomimimo.com/v1", "mimo-future"),
            ("https://api.minimax.io/v1", "MiniMax-M3"),
        ] {
            let template = template_for_model(base_url, model).unwrap();
            assert!(template.get("slug").is_none());
            assert!(template.get("use_responses_lite").is_none());
        }
    }

    #[test]
    fn deepseek_template_keeps_freeform_patch_framework() {
        let v: Value = serde_json::from_str(DEEPSEEK_TEMPLATE).unwrap();
        assert_eq!(
            v.get("apply_patch_tool_type").and_then(|x| x.as_str()),
            Some("freeform")
        );
        let instr = v
            .get("base_instructions")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert!(
            instr.len() > 1000,
            "base_instructions framework must ship verbatim"
        );
    }

    #[test]
    fn deepseek_and_mimo_templates_default_to_text_only() {
        // Unknown/new model IDs must not advertise image input until the
        // vendor documents it. `build_catalog` opts known vision models in.
        for template in [DEEPSEEK_TEMPLATE, MIMO_TEMPLATE] {
            let v: Value = serde_json::from_str(template).unwrap();
            let modalities = v
                .get("input_modalities")
                .and_then(|x| x.as_array())
                .expect("template must declare input_modalities");
            assert_eq!(modalities.len(), 1);
            assert_eq!(modalities[0].as_str(), Some("text"));
            assert_eq!(
                v.get("supports_image_detail_original")
                    .and_then(|x| x.as_bool()),
                Some(false)
            );
        }
    }

    #[test]
    fn vendor_model_capabilities_match_official_catalogs() {
        for (base_url, model_id, supports_image, original_detail, supports_search) in [
            (
                "https://api.deepseek.com",
                "deepseek-flash",
                true,
                true,
                true,
            ),
            (
                "https://api.deepseek.com",
                "deepseek-v4-pro",
                false,
                false,
                false,
            ),
            (
                "https://api.xiaomimimo.com/v1",
                "mimo-v2.5",
                true,
                true,
                false,
            ),
            (
                "https://api.xiaomimimo.com/v1",
                "mimo-v2.5-pro",
                false,
                true,
                false,
            ),
        ] {
            let template = template_for_model(base_url, model_id).unwrap();
            let catalog = build_catalog(&template, model_id, model_id, 1_048_576);
            let entry = &catalog["models"][0];
            let modalities = entry["input_modalities"].as_array().unwrap();
            assert_eq!(
                modalities
                    .iter()
                    .any(|value| value.as_str() == Some("image")),
                supports_image,
                "wrong image capability for {model_id}"
            );
            assert_eq!(
                entry["supports_image_detail_original"].as_bool(),
                Some(original_detail),
                "wrong original-image capability for {model_id}"
            );
            assert_eq!(
                entry["supports_search_tool"].as_bool(),
                Some(supports_search),
                "wrong search capability for {model_id}"
            );
        }
    }

    #[test]
    fn build_catalog_stamps_selected_model_identity() {
        // A new model version (v5) the bundled template has never heard of
        // must produce a catalog containing ONLY that model — no stale vendor
        // list, no accumulation.
        let tpl: Value = serde_json::from_str(DEEPSEEK_TEMPLATE).unwrap();
        let catalog = build_catalog(&tpl, "deepseek-v5-flash", "DeepSeek-V5-Flash", 1_048_576);
        let models = catalog.get("models").and_then(|m| m.as_array()).unwrap();
        assert_eq!(
            models.len(),
            1,
            "catalog must contain exactly the selected model"
        );
        let entry = &models[0];
        assert_eq!(
            entry.get("slug").and_then(|x| x.as_str()),
            Some("deepseek-v5-flash")
        );
        assert_eq!(
            entry.get("display_name").and_then(|x| x.as_str()),
            Some("DeepSeek-V5-Flash")
        );
        assert_eq!(
            entry.get("context_window").and_then(|x| x.as_u64()),
            Some(1_048_576)
        );
        // Capability framework survives the stamp.
        assert_eq!(
            entry.get("apply_patch_tool_type").and_then(|x| x.as_str()),
            Some("freeform")
        );
    }

    #[test]
    fn build_catalog_stamps_mimo_new_version() {
        // Same zero-maintenance promise as DeepSeek: mimo-v2.6 must produce a
        // catalog containing ONLY that model — the {date}/{week} placeholders
        // stay verbatim (Codex fills them at runtime), no {model} substitution
        // needed, no stale mimo-v2.5 / v2.5-pro entries.
        let tpl: Value = serde_json::from_str(MIMO_TEMPLATE).unwrap();
        let catalog = build_catalog(&tpl, "mimo-v2.6", "mimo-v2.6", 1_048_576);
        let models = catalog.get("models").and_then(|m| m.as_array()).unwrap();
        assert_eq!(models.len(), 1);
        let entry = &models[0];
        assert_eq!(
            entry.get("slug").and_then(|x| x.as_str()),
            Some("mimo-v2.6")
        );
        assert_eq!(
            entry.get("display_name").and_then(|x| x.as_str()),
            Some("mimo-v2.6")
        );
        assert_eq!(
            entry.get("context_window").and_then(|x| x.as_u64()),
            Some(1_048_576)
        );
        // Capability block (incl. no web_search for MiMo) survives.
        assert_eq!(
            entry.get("supports_search_tool").and_then(|x| x.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn build_catalog_substitutes_model_placeholder() {
        let tpl: Value = serde_json::from_str(MINIMAX_TEMPLATE).unwrap();
        let catalog = build_catalog(&tpl, "MiniMax-M5", "MiniMax-M5", 1_000_000);
        let entry = &catalog["models"][0];
        let bi = entry
            .get("base_instructions")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert!(
            bi.contains("MiniMax-M5") && !bi.contains("{model}"),
            "placeholder must be substituted: {bi}"
        );
    }

    #[test]
    fn build_catalog_applies_context_window_registry() {
        let tpl: Value = serde_json::from_str(DEEPSEEK_TEMPLATE).unwrap();
        let catalog = build_catalog(&tpl, "mini-model", "Mini Model", 204_800);
        assert_eq!(
            catalog["models"][0]["context_window"].as_u64(),
            Some(204_800)
        );
        assert_eq!(
            catalog["models"][0]["max_context_window"].as_u64(),
            Some(204_800)
        );
    }

    #[test]
    fn models_json_path_is_absolute_and_forward_slashed() {
        let codex_dir = std::env::temp_dir().join("echobird-codex-path-fixture");
        let p = models_json_path(&codex_dir);
        assert!(p.is_absolute());
        assert_eq!(p.parent(), Some(codex_dir.as_path()));
        let s = p.to_string_lossy();
        assert!(
            !s.contains('\\'),
            "Windows backslash must be forward-slashed: {s}"
        );
        assert!(
            s.ends_with("/models.json"),
            "must end with /models.json: {s}"
        );
        let relative = models_json_path(Path::new("echobird-codex-relative-fixture"));
        assert!(relative.is_absolute());
        assert_eq!(
            relative,
            std::env::current_dir()
                .unwrap()
                .join("echobird-codex-relative-fixture")
                .join("models.json")
        );
    }
}
