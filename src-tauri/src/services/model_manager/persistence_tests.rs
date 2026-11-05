use super::*;

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
