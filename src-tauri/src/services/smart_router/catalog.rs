//! Stable client selectors for the enabled models of one router.
use super::*;
use sha2::{Digest, Sha256};

pub(crate) const SELECTOR_PREFIX: &str = "claude-fable-echobird-";

#[derive(Clone, Debug)]
pub(crate) struct SwitchModel {
    pub id: String,
    pub name: String,
    pub codex_catalog: Option<serde_json::Value>,
    pub claude_context_window: Option<u64>,
}

impl SwitchModel {
    pub fn supports_claude_1m(&self) -> bool {
        self.claude_context_window
            .is_some_and(|window| window >= 1_000_000)
    }
}

#[cfg(test)]
tokio::task_local! { static TEST_MODELS: (String, Vec<SwitchModel>); }

#[cfg(test)]
pub(crate) fn with_test_models<T>(run: impl FnOnce() -> T) -> T {
    with_test_models_at("http://127.0.0.1:53683/v1", run)
}

#[cfg(test)]
pub(crate) fn with_test_models_at<T>(base_url: &str, run: impl FnOnce() -> T) -> T {
    TEST_MODELS.sync_scope(
        (
            base_url.trim_end_matches('/').into(),
            vec![
                SwitchModel {
                    id: "auto".into(),
                    name: "Auto Router".into(),
                    codex_catalog: None,
                    claude_context_window: None,
                },
                SwitchModel {
                    id: selector("first"),
                    name: "First model".into(),
                    codex_catalog: None,
                    claude_context_window: Some(1_048_576),
                },
                SwitchModel {
                    id: selector("second"),
                    name: "Second model".into(),
                    codex_catalog: None,
                    claude_context_window: Some(204_800),
                },
            ],
        ),
        run,
    )
}

#[cfg(test)]
pub(crate) async fn with_test_models_async<T>(run: impl std::future::Future<Output = T>) -> T {
    let catalog = with_test_models(|| TEST_MODELS.with(Clone::clone));
    TEST_MODELS.scope(catalog, run).await
}

pub(crate) fn selector(internal_id: &str) -> String {
    format!(
        "{SELECTOR_PREFIX}{}",
        &hex::encode(Sha256::digest(internal_id.as_bytes()))[..24]
    )
}

pub(super) fn display_name(model_id: &str, title: &str) -> String {
    format!("{model_id} · {title}")
}

/// "<upstream model> · <provider host>" for a client's model subtitle.
pub(super) fn description(model_id: &str, base_url: &str) -> String {
    let host = base_url
        .split("://")
        .last()
        .unwrap_or_default()
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    if host.is_empty() {
        model_id.to_string()
    } else {
        format!("{model_id} · {host}")
    }
}

pub(crate) fn profile_id_for_url(base_url: &str) -> Option<String> {
    let endpoint_port = loopback_port(base_url)?;
    profiles::ids()
        .into_iter()
        .find(|id| profiles::with(id, port) == endpoint_port)
}

pub(crate) fn models_for_url(base_url: &str) -> Result<Vec<SwitchModel>, String> {
    #[cfg(test)]
    if let Ok((url, models)) = TEST_MODELS.try_with(Clone::clone) {
        if url == base_url.trim_end_matches('/') {
            return Ok(models);
        }
    }
    let Some(id) = profile_id_for_url(base_url) else {
        return Ok(Vec::new());
    };
    profiles::with(&id, || {
        let config = load_config()?;
        let active = config.active_candidate_ids();
        Ok(models(
            &config.model_id,
            &config.name,
            &active,
            &get_candidate_models(),
        ))
    })
}

// Account gateways and resellers may offer different limits for a model
// with the same name. Only exact native vendor entries have known limits.
fn native_template(
    base_url: &str,
    model_id: &str,
    account_route: bool,
) -> Option<serde_json::Value> {
    if account_route {
        return None;
    }
    let mut template = crate::services::codex_catalog::template_for_model(base_url, model_id)?;
    let window =
        crate::services::codex_catalog::documented_context_window(&template, base_url, model_id)?;
    template["context_window"] = serde_json::json!(window);
    Some(template)
}

fn context_window(template: &serde_json::Value) -> Option<u64> {
    template["context_window"]
        .as_u64()
        .filter(|window| *window > 0)
}

fn known_context_window(base_url: &str, model_id: &str, account_route: bool) -> Option<u64> {
    context_window(&native_template(base_url, model_id, account_route)?)
}

pub(super) fn models(
    model_id: &str,
    name: &str,
    ids: &[String],
    candidates: &[ModelConfig],
) -> Vec<SwitchModel> {
    let mut models = vec![SwitchModel {
        id: model_id.into(),
        name: name.into(),
        codex_catalog: None,
        claude_context_window: None,
    }];
    models.extend(ids.iter().filter_map(|id| {
        let model = candidates
            .iter()
            .find(|m| m.internal_id == *id && model_is_routable(m))?;
        let model_id = model.model_id.as_deref()?.trim();
        let account_route = model.account_source.is_some();
        let template = native_template(&model.base_url, model_id, account_route);
        let claude_context_window = match model.anthropic_url.as_deref().map(str::trim) {
            Some(url) if !url.is_empty() && !super::is_router_url(url) && url != model.base_url => {
                known_context_window(url, model_id, account_route)
            }
            _ => template.as_ref().and_then(context_window),
        };
        Some(SwitchModel {
            id: selector(id),
            name: display_name(model_id, &model.name),
            codex_catalog: template.and_then(|template| {
                Some(
                    crate::services::codex_catalog::build_catalog(
                        &template,
                        model_id,
                        &model.name,
                        context_window(&template)?,
                    )["models"][0]
                        .clone(),
                )
            }),
            claude_context_window,
        })
    }));
    if models.len() > 1 && models.len() - 1 == ids.len() {
        models[0].claude_context_window = models[1..]
            .iter()
            .map(|model| model.claude_context_window)
            .try_fold(u64::MAX, |minimum, window| {
                window.map(|window| minimum.min(window))
            });
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capabilities_follow_candidate_identity_not_display_name() {
        let candidate = |id: &str, model: &str, base: &str| {
            serde_json::from_value::<ModelConfig>(serde_json::json!({
            "internalId":id,"name":"Same label","modelId":model,"baseUrl":base,"apiKey":"fixture"
        })).unwrap()
        };
        let candidates = vec![
            candidate("small", "glm-5-turbo", "https://open.bigmodel.cn/api/v1"),
            candidate("vision", "deepseek-flash", "https://api.deepseek.com/v1"),
            candidate("reseller", "deepseek-flash", "https://reseller.invalid/v1"),
            candidate("paused", "deepseek-flash", "https://api.deepseek.com/v1"),
        ];
        let listed = models(
            "auto",
            "Router",
            &["small".into(), "vision".into(), "reseller".into()],
            &candidates,
        );
        assert_eq!(listed.len(), 4);
        assert_eq!(listed[1].id, selector("small"));
        assert!(listed[1].codex_catalog.is_some());
        assert!(listed[3].codex_catalog.is_none());
        let catalog = crate::services::codex_catalog::build_router_catalog(&listed, false);
        let entries = &catalog["models"];
        assert_eq!(entries[1]["context_window"], 204_800);
        assert_eq!(entries[2]["context_window"], 1_048_576);
        assert!(entries[0]["context_window"].is_null());
        assert!(entries[3]["context_window"].is_null());
        assert!(entries[3]["max_context_window"].is_null());
        assert_eq!(
            crate::services::codex_catalog::build_router_catalog(&listed[..3], false)["models"][0]
                ["context_window"],
            204_800
        );
        assert!(!listed[0].supports_claude_1m());
        assert!(!listed[1].supports_claude_1m());
        assert!(listed[2].supports_claude_1m());
        assert!(!listed[3].supports_claude_1m());
        assert_eq!(
            entries[2]["input_modalities"],
            serde_json::json!(["text", "image"])
        );
        assert_eq!(entries[0]["input_modalities"], serde_json::json!(["text"]));
        assert_eq!(entries[3]["input_modalities"], serde_json::json!(["text"]));
        assert_eq!(entries[2]["slug"], selector("vision"));
        assert!(entries[2]["supported_reasoning_levels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["effort"] == "max"));
        assert!(!entries[0]["supported_reasoning_levels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["effort"] == "medium"));
        for entry in entries.as_array().unwrap() {
            assert_eq!(entry["use_responses_lite"], false);
            assert_eq!(entry["supports_search_tool"], false);
        }
    }

    #[test]
    fn picker_contains_only_active_models_in_this_group_and_keeps_duplicate_upstream_ids_distinct()
    {
        let candidate = |id: &str| {
            serde_json::from_value::<ModelConfig>(serde_json::json!({
            "internalId":id,"name":id,"modelId":"same-model","baseUrl":"https://fixture.invalid/v1","apiKey":"fixture"
        })).unwrap()
        };
        let mut candidates = vec![
            candidate("first"),
            candidate("second"),
            candidate("paused"),
            candidate("other-group"),
        ];
        candidates[1].name = "哈哈哈哈".into();
        let active = vec!["second".into(), "first".into(), "deleted".into()];
        let listed = models("custom-auto", "My router", &active, &candidates);
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].id, "custom-auto");
        assert_eq!(listed[0].name, "My router");
        assert_eq!(listed[1].id, selector("second"));
        assert_eq!(listed[2].id, selector("first"));
        assert_ne!(listed[1].id, listed[2].id);
        assert_eq!(listed[1].name, "same-model · 哈哈哈哈");
        assert_eq!(listed[2].name, "same-model · first");

        let native = crate::services::codex_catalog::build_router_catalog(&listed, false);
        assert_eq!(native["models"][0]["display_name"], "My router");
        assert_eq!(native["models"][1]["display_name"], "same-model · 哈哈哈哈");
        assert_eq!(native["models"][1]["slug"], selector("second"));
    }

    #[test]
    fn description_names_upstream_model_and_host_without_credentials() {
        assert_eq!(
            description("glm-5", "https://user:secret@open.bigmodel.cn/api/v1?x=1"),
            "glm-5 · open.bigmodel.cn"
        );
        assert_eq!(description("glm-5", ""), "glm-5");
    }

    #[test]
    fn selectors_are_stable_and_do_not_depend_on_upstream_names() {
        assert_eq!(selector("one"), selector("one"));
        assert_ne!(selector("one"), selector("two"));
        assert!(selector("vendor/model[1m]").starts_with(SELECTOR_PREFIX));
        assert_eq!(selector("one").len(), SELECTOR_PREFIX.len() + 24);
    }
}
