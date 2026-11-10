//! Stable client selectors for the enabled models of one router.
use super::*;
use sha2::{Digest, Sha256};

pub(crate) const SELECTOR_PREFIX: &str = "claude-fable-echobird-";

#[derive(Clone, Debug)]
pub(crate) struct SwitchModel {
    pub id: String,
    pub name: String,
    pub codex_catalog: Option<serde_json::Value>,
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
                },
                SwitchModel {
                    id: selector("first"),
                    name: "First model".into(),
                    codex_catalog: None,
                },
                SwitchModel {
                    id: selector("second"),
                    name: "Second model".into(),
                    codex_catalog: None,
                },
            ],
        ),
        run,
    )
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

pub(crate) fn models_for_url(base_url: &str) -> Result<Vec<SwitchModel>, String> {
    #[cfg(test)]
    if let Ok((url, models)) = TEST_MODELS.try_with(Clone::clone) {
        if url == base_url.trim_end_matches('/') {
            return Ok(models);
        }
    }
    let Some(id) = profiles::ids()
        .into_iter()
        .find(|id| profiles::with(id, || is_loopback_url_on_port(base_url, port())))
    else {
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
    }];
    models.extend(ids.iter().filter_map(|id| {
        let model = candidates
            .iter()
            .find(|m| m.internal_id == *id && model_is_routable(m))?;
        Some(SwitchModel {
            id: selector(id),
            name: display_name(model.model_id.as_deref().unwrap_or_default(), &model.name),
            // Account gateways and resellers must not inherit native-vendor
            // capabilities merely because their model has the same name.
            codex_catalog: model
                .account_source
                .is_none()
                .then(|| {
                    crate::services::codex_catalog::template_for_model(
                        &model.base_url,
                        model.model_id.as_deref().unwrap_or_default(),
                    )
                    .filter(|template| {
                        template["slug"].as_str() == model.model_id.as_deref()
                            || (crate::services::codex_catalog::url_matches_domain(
                                &model.base_url,
                                "deepseek.com",
                            ) && matches!(
                                model.model_id.as_deref(),
                                Some("deepseek-flash" | "deepseek-v4-pro")
                            ))
                    })
                    .map(|template| {
                        let context = template["context_window"].as_u64().unwrap_or(
                            match model.model_id.as_deref() {
                                Some("deepseek-flash" | "deepseek-v4-pro") => 1_048_576,
                                _ => 1_000_000,
                            },
                        );
                        crate::services::codex_catalog::build_catalog(
                            &template,
                            model.model_id.as_deref().unwrap_or_default(),
                            &model.name,
                            context,
                        )["models"][0]
                            .clone()
                    })
                })
                .flatten(),
        })
    }));
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
        assert_eq!(entries[0]["context_window"], 204_800);
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
