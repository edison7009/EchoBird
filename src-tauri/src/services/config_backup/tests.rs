use super::*;
fn write(root: &Path, path: &str, value: Value) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn decrypt(s: &str) -> Result<String, String> {
    Ok(s.strip_prefix("enc:v1:S:")
        .or_else(|| s.strip_prefix("enc:v1:T:"))
        .unwrap_or(s)
        .to_owned())
}
fn encrypt(s: &str) -> Result<String, String> {
    Ok(format!("enc:v1:T:{s}"))
}
fn fixture(root: &Path) {
    write(
        root,
        "settings.json",
        json!({"themeMode":"dark","locale":"zh-Hans"}),
    );
    write(
        root,
        "config/models.json",
        json!([
            {"internalId":"model-fixture","name":"Fixture","modelId":"fixture-model","baseUrl":"https://example.invalid/v1","anthropicUrl":"https://example.invalid","apiKey":"enc:v1:S:fixture-api-key"},
            {"internalId":"account-model","name":"Account model","baseUrl":"account://echobird","apiKey":"enc:v1:S:do-not-decrypt-account","accountSource":{"provider":"codex","accountId":"native-account"}}
        ]),
    );
    write(
        root,
        "config/model-order.json",
        json!(["model-fixture", "account-model"]),
    );
    write(
        root,
        "config/smart-router.json",
        json!({"name":"Fixture route","icon":"rocket","enabled":true,"candidateIds":["model-fixture","account-model"],"disabledCandidateIds":["account-model"],"apiKey":"enc:v1:S:router-access-key"}),
    );
    // These are deliberately invalid: neither export nor restore may inspect account stores.
    fs::create_dir_all(root.join("codex-accounts")).unwrap();
    fs::write(
        root.join("codex-accounts/native-account.json"),
        b"OS-bound native account",
    )
    .unwrap();
    fs::write(
        root.join("codex-accounts/native-account.quota.json"),
        b"partial quota cache",
    )
    .unwrap();
    fs::write(
        root.join("commandcode-accounts.json"),
        b"destination account secrets",
    )
    .unwrap();
}
#[test]
fn api_models_round_trip_across_devices_without_reading_or_writing_login_accounts() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    fs::write(
        target.path().join("commandcode-accounts.json"),
        b"keep destination login",
    )
    .unwrap();
    let backup = collect(
        source.path(),
        "portable-api-models".into(),
        BTreeMap::from([("echobird_background".into(), "none".into())]),
        &|value| {
            assert!(!value.contains("do-not-decrypt-account"));
            decrypt(value)
        },
    )
    .unwrap();
    assert_eq!(backup.records.len(), 4);
    assert!(backup
        .records
        .iter()
        .all(|record| !record.kind.starts_with("account:") && !record.device_bound));
    let serialized = serde_json::to_vec(&backup).unwrap();
    assert!(!String::from_utf8_lossy(&serialized).contains("native-account"));
    assert!(!String::from_utf8_lossy(&serialized).contains("do-not-decrypt-account"));
    let encrypted = format::encode(&serialized, "fixture-password").unwrap();
    assert!(!String::from_utf8_lossy(&encrypted).contains("fixture-api-key"));
    let restored =
        serde_json::from_slice(&format::decode(&encrypted, "fixture-password").unwrap()).unwrap();
    let (plan, outcome) = prepare(target.path(), restored, &encrypt, &decrypt).unwrap();
    assert_eq!(outcome.records, 4);
    assert_eq!(outcome.device_bound, 0);
    assert!(
        !target.path().join("settings.json").exists(),
        "planning wrote user data"
    );
    publish(&plan).unwrap();
    let models = records::json_at(&target.path().join("config/models.json"))
        .unwrap()
        .unwrap();
    assert_eq!(models.as_array().unwrap().len(), 1);
    assert_eq!(models[0]["modelId"], "fixture-model");
    assert_eq!(models[0]["baseUrl"], "https://example.invalid/v1");
    assert_eq!(models[0]["anthropicUrl"], "https://example.invalid");
    assert_eq!(models[0]["apiKey"], "enc:v1:T:fixture-api-key");
    let router = records::json_at(&target.path().join("config/smart-router.json"))
        .unwrap()
        .unwrap();
    assert_eq!(router["enabled"], false);
    assert_eq!(router["icon"], "rocket");
    assert_eq!(router["apiKey"], "enc:v1:T:router-access-key");
    assert_eq!(router["candidateIds"], json!(["model-fixture"]));
    assert_eq!(router["disabledCandidateIds"], json!([]));
    assert_eq!(
        records::json_at(&target.path().join("config/model-order.json")).unwrap(),
        Some(json!(["model-fixture"]))
    );
    assert!(!target.path().join("codex-accounts").exists());
    assert_eq!(
        fs::read(target.path().join("commandcode-accounts.json")).unwrap(),
        b"keep destination login"
    );
    assert_eq!(
        fs::read(
            source
                .path()
                .join("codex-accounts/native-account.quota.json")
        )
        .unwrap(),
        b"partial quota cache"
    );
    assert!(!source.path().join("config/.encryption_key").exists());
    assert!(!target.path().join("config/.encryption_key").exists());
    let (again, repeat) = prepare(target.path(), backup, &encrypt, &decrypt).unwrap();
    publish(&again).unwrap();
    assert_eq!(repeat.duplicates, 4);
    assert_eq!(repeat.records, 0);
    assert!(!repeat.restart_required);
    assert!(again.is_empty());
    assert_eq!(
        fs::read(target.path().join("commandcode-accounts.json")).unwrap(),
        b"keep destination login"
    );
}
#[test]
fn legacy_backups_skip_all_login_records_before_unsealing_them() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    let mut backup = collect(
        source.path(),
        "same-device".into(),
        BTreeMap::new(),
        &decrypt,
    )
    .unwrap();
    backup.records.extend([
        Record {kind:"account:codex".into(),id:"../../ignored-login".into(),data:json!({"secret":{"$echobirdSecret":"never-open-account"}}),device_bound:false},
        Record {kind:"account:cursor".into(),id:"native-account".into(),data:json!({"session":"foreign-vault"}),device_bound:true},
        Record {kind:"model".into(),id:"legacy-account-model".into(),data:json!({"internalId":"legacy-account-model","name":"Legacy account","baseUrl":"account://echobird","apiKey":{"$echobirdSecret":"never-open-account"},"accountSource":{"provider":"codex","accountId":"native-account"}}),device_bound:false},
    ]);
    let account_router = format!("smart-router-{}", uuid::Uuid::new_v4());
    backup.records.push(Record {kind:"router".into(),id:account_router.clone(),data:json!({"candidateIds":["legacy-account-model"],"disabledCandidateIds":[],"apiKey":{"$echobirdSecret":"never-open-account"}}),device_bound:false});
    backup
        .records
        .iter_mut()
        .find(|r| r.kind == "modelOrder")
        .unwrap()
        .data["ids"] = json!(["model-fixture", "legacy-account-model"]);
    let (plan, outcome) = prepare(
        target.path(),
        backup,
        &|secret| {
            assert_ne!(secret, "never-open-account");
            encrypt(secret)
        },
        &decrypt,
    )
    .unwrap();
    assert_eq!(outcome.skipped, 4);
    assert!(plan
        .iter()
        .all(|item| !item.path.to_string_lossy().contains("accounts")));
    publish(&plan).unwrap();
    assert!(!target.path().join("codex-accounts").exists());
    assert!(!target.path().join("cursor-accounts.json").exists());
    assert!(!target
        .path()
        .join("config")
        .join(format!("{account_router}.json"))
        .exists());
    let models = records::json_at(&target.path().join("config/models.json"))
        .unwrap()
        .unwrap();
    assert_eq!(models.as_array().unwrap().len(), 1);
}
#[test]
fn malformed_portable_models_and_unsupported_versions_are_rejected_without_writes() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    let backup = collect(source.path(), "source".into(), BTreeMap::new(), &decrypt).unwrap();
    let mut corrupt = backup.clone();
    corrupt.records.push(Record {
        kind: "model".into(),
        id: "bad".into(),
        data: json!({"internalId":"bad"}),
        device_bound: false,
    });
    let calls = std::cell::Cell::new(0);
    assert!(prepare(
        target.path(),
        corrupt,
        &|_| {
            calls.set(calls.get() + 1);
            Ok("should not run".into())
        },
        &decrypt
    )
    .is_err());
    assert_eq!(calls.get(), 0);
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    let mut future = backup.clone();
    future.version = 2;
    assert!(prepare(target.path(), future, &encrypt, &decrypt).is_err());
    let mut oversized = backup;
    oversized.records = vec![
        Record {
            kind: "account:codex".into(),
            id: "ignored".into(),
            data: json!({}),
            device_bound: false
        };
        10_001
    ];
    assert!(prepare(target.path(), oversized, &encrypt, &decrypt).is_err());
    write(
        source.path(),
        "config/models.json",
        json!([{"internalId":"broken-portable"}]),
    );
    assert!(collect(source.path(), "source".into(), BTreeMap::new(), &decrypt).is_err());
}
#[test]
fn account_only_models_and_routes_are_not_exported() {
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    write(
        source.path(),
        "config/models.json",
        json!([{"internalId":"account-model","accountSource":{"provider":"future-provider","accountId":"ignored"},"apiKey":"enc:v1:S:never-read"}]),
    );
    write(
        source.path(),
        "config/smart-router.json",
        json!({"candidateIds":["account-model"],"disabledCandidateIds":[],"apiKey":"enc:v1:S:never-read"}),
    );
    let backup = collect(source.path(), "source".into(), BTreeMap::new(), &|_| {
        panic!("account credentials must not be read")
    })
    .unwrap();
    assert!(backup
        .records
        .iter()
        .all(|record| !matches!(record.kind.as_str(), "model" | "router")));
}
#[test]
fn guarded_publication_rolls_back_failure_and_preserves_concurrent_changes() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.json");
    fs::write(&first, b"original").unwrap();
    fs::write(root.path().join("blocked"), b"not a directory").unwrap();
    let plan = vec![
        Planned {
            path: first.clone(),
            before: Some(b"original".to_vec()),
            after: b"new".to_vec(),
        },
        Planned {
            path: root.path().join("blocked/file.json"),
            before: None,
            after: b"new".to_vec(),
        },
    ];
    assert!(publish(&plan).is_err());
    assert_eq!(fs::read(&first).unwrap(), b"original");
    fs::write(&first, b"concurrent user edit").unwrap();
    assert!(publish(&plan[..1]).is_err());
    assert_eq!(fs::read(&first).unwrap(), b"concurrent user edit");
}

#[cfg(unix)]
#[test]
fn symlink_targets_are_rejected_without_modifying_their_contents() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let file = other.path().join("settings.json");
    fs::write(&file, b"outside").unwrap();
    let link = root.path().join("symbolic.json");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert!(add_plan(root.path(), link, &json!({}), &mut vec![]).is_err());
    assert_eq!(fs::read(file).unwrap(), b"outside");
}

#[test]
fn shared_api_keys_keep_distinct_model_ids_and_scopes_when_restored() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    write(
        source.path(),
        "config/models.json",
        json!([
            {"internalId":"one","name":"First","modelId":"model-one","baseUrl":"https://example.invalid/v1","apiKey":"enc:v1:S:shared-key"},
            {"internalId":"two","name":"Second","modelId":"model-two","baseUrl":"https://example.invalid/v1","apiKey":"enc:v1:S:shared-key"},
            {"internalId":"routed-one","name":"Routed","modelId":"model-one","baseUrl":"https://example.invalid/v1","apiKey":"enc:v1:S:shared-key","scope":"smartRouter"}
        ]),
    );
    write(
        source.path(),
        "config/smart-router.json",
        json!({"name":"API routes","candidateIds":["one","two","routed-one"],"disabledCandidateIds":[],"apiKey":"enc:v1:S:route-key"}),
    );
    let backup = collect(source.path(), "source".into(), BTreeMap::new(), &decrypt).unwrap();
    let (plan, _) = prepare(target.path(), backup.clone(), &encrypt, &decrypt).unwrap();
    publish(&plan).unwrap();
    let models = records::json_at(&target.path().join("config/models.json"))
        .unwrap()
        .unwrap();
    assert_eq!(models.as_array().unwrap().len(), 3);
    assert_eq!(models[0]["modelId"], "model-one");
    assert_eq!(models[1]["modelId"], "model-two");
    assert_eq!(models[2]["scope"], "smartRouter");
    let router = records::json_at(&target.path().join("config/smart-router.json"))
        .unwrap()
        .unwrap();
    assert_eq!(router["candidateIds"], json!(["one", "two", "routed-one"]));
    let (again, _) = prepare(target.path(), backup, &encrypt, &decrypt).unwrap();
    publish(&again).unwrap();
    assert_eq!(
        records::json_at(&target.path().join("config/models.json")).unwrap(),
        Some(models)
    );
}

#[test]
fn colliding_destination_account_model_ids_are_preserved_without_decrypting_accounts() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    let existing = json!({"internalId":"model-fixture","name":"Local account model","baseUrl":"account://echobird","apiKey":"enc:v1:T:never-decrypt-account","accountSource":{"provider":"codex","accountId":"local-account"}});
    write(
        target.path(),
        "config/models.json",
        json!([existing.clone()]),
    );
    let backup = collect(source.path(), "source".into(), BTreeMap::new(), &decrypt).unwrap();
    let (plan, _) = prepare(target.path(), backup, &encrypt, &|value| {
        assert!(!value.contains("never-decrypt-account"));
        decrypt(value)
    })
    .unwrap();
    publish(&plan).unwrap();
    let models = records::json_at(&target.path().join("config/models.json"))
        .unwrap()
        .unwrap();
    assert_eq!(models.as_array().unwrap().len(), 2);
    assert_eq!(models[0], existing);
    assert_ne!(models[1]["internalId"], "model-fixture");
    assert_eq!(models[1]["modelId"], "fixture-model");
    let router = records::json_at(&target.path().join("config/smart-router.json"))
        .unwrap()
        .unwrap();
    assert_eq!(router["candidateIds"], json!([models[1]["internalId"]]));
}

#[test]
fn same_machine_restore_does_not_count_or_rewrite_unchanged_order_and_settings() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let backup = collect(
        directory.path(),
        "portable-api-models".into(),
        BTreeMap::new(),
        &decrypt,
    )
    .unwrap();
    let before_order = fs::read(directory.path().join("config/model-order.json")).unwrap();
    let before_settings = fs::read(directory.path().join("settings.json")).unwrap();
    let (plan, result) = prepare(directory.path(), backup.clone(), &encrypt, &decrypt).unwrap();
    assert!(plan.is_empty());
    assert_eq!(result.records, 0);
    assert_eq!(result.duplicates, backup.records.len());
    assert!(!result.restart_required);
    publish(&plan).unwrap();
    assert_eq!(
        fs::read(directory.path().join("config/model-order.json")).unwrap(),
        before_order
    );
    assert_eq!(
        fs::read(directory.path().join("settings.json")).unwrap(),
        before_settings
    );
    write(directory.path(), "settings.json", json!({"locale":"ja"}));
    let (plan, result) = prepare(directory.path(), backup, &encrypt, &decrypt).unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(result.records, 1);
    assert!(result.restart_required);
}
