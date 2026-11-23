use super::*;

#[test]
fn detached_voice_models_and_their_keys_are_removed_without_touching_other_scopes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let add = |scope, key: &str| {
        add_model_at(
            &path,
            AddModelInput {
                scope,
                api_key: Some(key.into()),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let kept = add(ModelScope::VoiceInput, "active-voice-key");
    add(ModelScope::VoiceInput, "legacy-orphan-key");
    let removed = add(ModelScope::VoiceInput, "detached-key");
    let general = add(ModelScope::ModelCenter, "general-key");
    let router = add(ModelScope::SmartRouter, "router-key");

    prune_voice_models_at(&path, &HashSet::from([kept.internal_id.as_str()])).unwrap();
    let models: Vec<ModelConfig> = super::super::json_store::read(&path).unwrap();
    assert_eq!(
        models
            .iter()
            .map(|model| &model.internal_id)
            .collect::<Vec<_>>(),
        [&kept.internal_id, &general.internal_id, &router.internal_id]
    );
    assert_eq!(models[0].api_key, kept.api_key);
    let saved = fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("legacy-orphan-key"));
    assert!(!saved.contains(&removed.api_key));
    prune_voice_models_at(&path, &HashSet::from([kept.internal_id.as_str()])).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn imports_distinct_models_and_endpoints_with_a_shared_key() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let input = AddModelInput {
        name: Some("original".into()),
        base_url: Some("https://first.example/v1".into()),
        model_id: Some("first-model".into()),
        api_key: Some("synthetic-key".into()),
        ..Default::default()
    };
    let protect = |key: &str| Ok(key.to_owned());
    assert!(import_model_at_with(&path, input.clone(), protect).unwrap());
    let before = fs::read(&path).unwrap();
    let mut other = input;
    other.name = Some("another source".into());
    other.base_url = Some("https://first.example/v1/".into());
    assert!(!import_model_at_with(&path, other.clone(), protect).unwrap());
    assert_eq!(fs::read(&path).unwrap(), before);
    other.model_id = Some("second-model".into());
    assert!(import_model_at_with(&path, other.clone(), protect).unwrap());
    other.base_url = Some("https://second.example/v1".into());
    assert!(import_model_at_with(&path, other.clone(), protect).unwrap());
    assert!(!import_model_at_with(&path, other, protect).unwrap());
    let models: Vec<ModelConfig> = super::super::json_store::read(&path).unwrap();
    assert_eq!(models.len(), 3);
    assert_eq!(models[0].name, "original");
    fs::write(&path, b"malformed {").unwrap();
    assert!(import_model_at_with(
        &path,
        AddModelInput {
            api_key: Some("fixture".into()),
            ..Default::default()
        },
        protect
    )
    .is_err());
    assert_eq!(fs::read(&path).unwrap(), b"malformed {");
}

#[test]
fn gemini_text_urls_share_the_compatible_endpoint_but_keys_and_voice_stay_separate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let native = "https://generativelanguage.googleapis.com/v1beta";
    let compatible = "https://generativelanguage.googleapis.com/v1beta/openai";
    let mut input = AddModelInput {
        base_url: Some(native.into()),
        model_id: Some("models/gemini-fixture".into()),
        api_key: Some("key-a".into()),
        ..Default::default()
    };
    let protect = |key: &str| Ok(key.to_owned());
    assert!(import_model_at_with(&path, input.clone(), protect).unwrap());
    input.base_url = Some(format!("{compatible}/chat/completions"));
    input.model_id = Some("gemini-fixture".into());
    assert!(!import_model_at_with(&path, input.clone(), protect).unwrap());
    input.api_key = Some("key-b".into());
    assert!(import_model_at_with(&path, input.clone(), protect).unwrap());

    let mut saved: Vec<ModelConfig> = super::super::json_store::read(&path).unwrap();
    assert_eq!(saved.len(), 2);
    assert_ne!(saved[0].internal_id, saved[1].internal_id);
    assert_eq!(saved[0].api_key, "key-a");
    assert_eq!(saved[1].api_key, "key-b");
    assert!(saved
        .iter()
        .all(|model| model.base_url == compatible
            && model.model_id.as_deref() == Some("gemini-fixture")));

    saved[0].base_url = native.into();
    saved[0].model_id = Some("models/gemini-fixture".into());
    let legacy = normalize_text_models(saved);
    assert_eq!(legacy[0].base_url, compatible);
    assert_eq!(legacy[0].model_id.as_deref(), Some("gemini-fixture"));
    let updated = update_model_at(
        &path,
        &legacy[0].internal_id,
        UpdateModelInput {
            base_url: Some(format!("{native}/models")),
            model_id: Some("models/gemini-next".into()),
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(updated.base_url, compatible);
    assert_eq!(updated.model_id.as_deref(), Some("gemini-next"));
    assert_eq!(updated.openai_responses, Some(false));

    let relay = update_model_at(
        &path,
        &legacy[0].internal_id,
        UpdateModelInput {
            base_url: Some("https://relay.example/v1".into()),
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(relay.openai_responses, None);

    input.scope = ModelScope::VoiceInput;
    input.base_url = Some(native.into());
    let voice = new_model_from(input, "voice".into());
    assert_eq!(normalize_text_models(vec![voice])[0].base_url, native);
}

#[test]
fn model_crud_is_durable_and_rejects_corrupt_or_unwritable_storage() {
    let root = std::env::temp_dir().join(format!("echobird-model-crud-{}", uuid::Uuid::new_v4()));
    let path = root.join("models.json");
    let model = add_model_at(
        &path,
        AddModelInput {
            name: Some("first".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(update_model_at(
        &path,
        &model.internal_id,
        UpdateModelInput {
            name: Some("changed".into()),
            ..Default::default()
        }
    )
    .unwrap()
    .is_some());
    let saved: Vec<ModelConfig> = super::super::json_store::read(&path).unwrap();
    assert_eq!(saved[0].name, "changed");
    assert!(delete_model_at(&path, &model.internal_id).unwrap());
    assert!(!delete_model_at(&path, &model.internal_id).unwrap());
    fs::write(&path, b"broken {").unwrap();
    assert!(add_model_at(&path, AddModelInput::default()).is_err());
    assert!(delete_model_at(&path, &model.internal_id).is_err());
    assert!(update_model_at(&path, &model.internal_id, UpdateModelInput::default()).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"broken {");
    assert!(add_model_at(&path.join("blocked"), AddModelInput::default()).is_err());
    fs::remove_dir_all(root).unwrap();
}
