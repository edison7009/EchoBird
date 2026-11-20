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
fn imports_deduplicate_by_key_across_urls_and_preserve_existing_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let input = AddModelInput {
        name: Some("original".into()),
        base_url: Some("https://first.example/v1".into()),
        api_key: Some("synthetic-key".into()),
        ..Default::default()
    };
    let protect = |key: &str| Ok(key.to_owned());
    assert!(import_model_at_with(&path, input.clone(), protect).unwrap());
    let before = fs::read(&path).unwrap();
    let mut other = input;
    other.name = Some("another source".into());
    other.base_url = Some("https://second.example/v1".into());
    assert!(!import_model_at_with(&path, other.clone(), protect).unwrap());
    assert_eq!(fs::read(&path).unwrap(), before);
    other.api_key = Some("different-key".into());
    assert!(import_model_at_with(&path, other, protect).unwrap());
    let models: Vec<ModelConfig> = super::super::json_store::read(&path).unwrap();
    assert_eq!(models.len(), 2);
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
