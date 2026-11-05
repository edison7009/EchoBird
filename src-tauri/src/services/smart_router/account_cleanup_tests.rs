use super::*;
use crate::models::model::{AccountProvider, AccountSource};
use serde_json::json;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "echobird-deleted-account-routes-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn model(id: &str, source: Option<&AccountSource>, scope: ModelScope) -> ModelConfig {
    serde_json::from_value(json!({
        "internalId":id, "name":id, "modelId":"fixture-model",
        "baseUrl":"https://fixture.test/v1", "apiKey":"fixture-key",
        "accountSource":source, "scope":scope
    }))
    .unwrap()
}

#[test]
fn deleting_an_account_removes_its_routes_and_only_router_owned_models_for_every_provider() {
    use AccountProvider::*;
    for provider in [
        ClaudeCode,
        Codex,
        WorkBuddy,
        WorkBuddyAi,
        Antigravity,
        Copilot,
        Kilo,
        Trae,
        Traecn,
        Minimax,
        Qoder,
        QoderCn,
        Grok,
        Cline,
        Kiro,
        Zcode,
        Kimi,
        OpenDesign,
        CodeArts,
    ] {
        let fixture = Fixture::new();
        let path = fixture.0.join("smart-router.json");
        let source = AccountSource {
            provider,
            account_id: "deleted-account".into(),
        };
        let other_account = AccountSource {
            account_id: "other-account".into(),
            ..source.clone()
        };
        let other_provider = AccountSource {
            provider: if provider == CodeArts {
                Codex
            } else {
                CodeArts
            },
            ..source.clone()
        };
        let models = vec![
            model("deleted-one", Some(&source), ModelScope::SmartRouter),
            model("api-model", None, ModelScope::ModelCenter),
            model(
                "other-account",
                Some(&other_account),
                ModelScope::SmartRouter,
            ),
            model("deleted-two", Some(&source), ModelScope::SmartRouter),
            model(
                "other-provider",
                Some(&other_provider),
                ModelScope::SmartRouter,
            ),
            model("original-model", Some(&source), ModelScope::ModelCenter),
            model("unselected", Some(&source), ModelScope::SmartRouter),
        ];
        let original = StoredConfig {
            candidate_ids: [
                "deleted-one",
                "api-model",
                "other-account",
                "deleted-two",
                "other-provider",
                "original-model",
                "local-server",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            disabled_candidate_ids: vec!["deleted-two".into(), "api-model".into()],
            api_key: "fixture-router-key".into(),
            ..StoredConfig::default()
        };
        save_config_at(&path, &original).unwrap();
        let mut deleted_models = Vec::new();
        let remaining = remove_account_candidates_at(&path, &models, &source, |id| {
            deleted_models.push(id.to_owned());
            Ok(())
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            remaining,
            [
                "api-model",
                "other-account",
                "other-provider",
                "local-server"
            ]
        );
        let stored: StoredConfig = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored.candidate_ids, remaining);
        assert_eq!(stored.disabled_candidate_ids, ["api-model"]);
        assert_eq!(stored.api_key, original.api_key);
        assert_eq!(stored.enabled, original.enabled);
        assert_eq!(deleted_models, ["deleted-one", "deleted-two"]);
        let before = fs::read(&path).unwrap();
        assert!(
            remove_account_candidates_at(&path, &models, &source, |_| panic!(
                "repeated cleanup must not delete models"
            ))
            .unwrap()
            .is_none()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

#[test]
fn missing_or_unreadable_router_state_never_deletes_models_or_initializes_defaults() {
    let fixture = Fixture::new();
    let path = fixture.0.join("smart-router.json");
    let source = AccountSource {
        provider: AccountProvider::CodeArts,
        account_id: "fixture".into(),
    };
    let models = [model("fixture", Some(&source), ModelScope::SmartRouter)];
    assert!(
        remove_account_candidates_at(&path, &models, &source, |_| panic!("no routes exist"))
            .unwrap()
            .is_none()
    );
    assert!(!path.exists());
    fs::write(&path, b"invalid config").unwrap();
    assert!(
        remove_account_candidates_at(&path, &models, &source, |_| panic!(
            "preserve models on read failure"
        ))
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"invalid config");
}
