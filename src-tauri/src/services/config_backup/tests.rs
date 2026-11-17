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
        json!([{"internalId":"model-fixture","name":"Fixture","baseUrl":"https://example.invalid/v1","apiKey":"enc:v1:S:fixture-api-key"}]),
    );
    write(
        root,
        "commandcode-accounts.json",
        json!({"accounts":[{"id":"account-fixture","email":"fixture@example.test","plan":null,"remaining":null,"total":null,"active":true,"key":"enc:v1:S:fixture-account-key"}]}),
    );
    write(
        root,
        "antigravity-accounts.json",
        json!({"anti-fixture":{"email":"other@example.test","credential":"enc:v1:S:serialized-account-credential","plan":null,"quotas":[]}}),
    );
    write(
        root,
        "cursor-accounts.json",
        json!({"native-fixture":{"session":"os-bound-ciphertext","applied":true,"email":"native@example.test","usage":null}}),
    );
    write(
        root,
        "config/smart-router.json",
        json!({"name":"Fixture route","enabled":true,"candidateIds":["model-fixture"],"disabledCandidateIds":[],"apiKey":"enc:v1:S:router-access-key"}),
    );
}
#[test]
fn migrations_reseal_secrets_skip_foreign_vaults_and_do_not_activate_clients() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    let source_before = fs::read(source.path().join("commandcode-accounts.json")).unwrap();
    let backup = collect(
        source.path(),
        "source-device".into(),
        BTreeMap::from([("echobird_background".into(), "none".into())]),
        &decrypt,
    )
    .unwrap();
    assert_eq!(backup.records.len(), 6);
    assert_eq!(backup.records.iter().filter(|r| r.device_bound).count(), 1);
    let (plan, outcome) = prepare(
        target.path(),
        backup.clone(),
        "target-device",
        &encrypt,
        &decrypt,
    )
    .unwrap();
    assert_eq!(outcome.records, 5);
    assert_eq!(outcome.skipped, 1);
    assert_eq!(outcome.device_bound, 1);
    assert!(
        !target.path().join("settings.json").exists(),
        "planning wrote user data"
    );
    publish(&plan).unwrap();
    let models = records::json_at(&target.path().join("config/models.json"))
        .unwrap()
        .unwrap();
    assert_eq!(models[0]["apiKey"], "enc:v1:T:fixture-api-key");
    let accounts = records::json_at(&target.path().join("commandcode-accounts.json"))
        .unwrap()
        .unwrap();
    assert_eq!(
        accounts["accounts"][0]["key"],
        "enc:v1:T:fixture-account-key"
    );
    assert_eq!(accounts["accounts"][0]["active"], false);
    assert!(!target.path().join("cursor-accounts.json").exists());
    let router = records::json_at(&target.path().join("config/smart-router.json"))
        .unwrap()
        .unwrap();
    assert_eq!(router["enabled"], false);
    assert_eq!(router["apiKey"], "enc:v1:T:router-access-key");
    let registry = records::json_at(&target.path().join("config/smart-routers.json"))
        .unwrap()
        .unwrap();
    assert_eq!(registry["enabled"], false);
    assert!(!source.path().join("config/.encryption_key").exists());
    assert!(!target.path().join("config/.encryption_key").exists());
    assert_eq!(
        fs::read(source.path().join("commandcode-accounts.json")).unwrap(),
        source_before
    );
    let account_bytes = fs::read(target.path().join("commandcode-accounts.json")).unwrap();
    let (again, repeat) =
        prepare(target.path(), backup, "target-device", &encrypt, &decrypt).unwrap();
    publish(&again).unwrap();
    assert!(repeat.duplicates >= 4);
    assert_eq!(
        fs::read(target.path().join("commandcode-accounts.json")).unwrap(),
        account_bytes
    );
}
#[test]
fn email_duplicates_keep_destination_credentials_and_rebind_account_models() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    write(
        target.path(),
        "commandcode-accounts.json",
        json!({"accounts":[{"id":"destination-account","email":"FIXTURE@example.test","plan":null,"remaining":null,"total":null,"active":true,"key":"enc:v1:T:keep-this-key"}]}),
    );
    write(
        source.path(),
        "config/models.json",
        json!([{"internalId":"account-model","name":"Account model","baseUrl":"https://example.invalid/v1","apiKey":"","accountSource":{"provider":"commandcode","accountId":"account-fixture"}}]),
    );
    let before = fs::read(target.path().join("commandcode-accounts.json")).unwrap();
    let backup = collect(source.path(), "source".into(), BTreeMap::new(), &decrypt).unwrap();
    let (plan, outcome) = prepare(target.path(), backup, "target", &encrypt, &decrypt).unwrap();
    publish(&plan).unwrap();
    assert!(outcome.duplicates >= 1);
    assert_eq!(
        fs::read(target.path().join("commandcode-accounts.json")).unwrap(),
        before
    );
    let models = records::json_at(&target.path().join("config/models.json"))
        .unwrap()
        .unwrap();
    assert_eq!(
        models[0]["accountSource"]["accountId"],
        "destination-account"
    );
}
#[test]
fn unsupported_or_malformed_records_are_rejected_before_any_secret_or_file_write() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    let backup = collect(source.path(), "source".into(), BTreeMap::new(), &decrypt).unwrap();
    let mut corrupt = backup.clone();
    corrupt.records.push(Record {
        kind: "account:codex".into(),
        id: "../../escape".into(),
        data: json!({}),
        device_bound: false,
    });
    let calls = std::cell::Cell::new(0);
    assert!(prepare(
        target.path(),
        corrupt,
        "target",
        &|_| {
            calls.set(calls.get() + 1);
            Ok("should not run".into())
        },
        &decrypt
    )
    .is_err());
    assert_eq!(calls.get(), 0);
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    let mut corrupt = backup.clone();
    corrupt.records.push(Record {
        kind: "account:commandcode".into(),
        id: "bad".into(),
        data: json!({"id":"bad"}),
        device_bound: false,
    });
    assert!(prepare(target.path(), corrupt, "target", &encrypt, &decrypt).is_err());
    let mut future = backup;
    future.version = 2;
    assert!(prepare(target.path(), future, "target", &encrypt, &decrypt).is_err());
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
#[test]
fn same_device_snapshots_are_preserved_and_symlink_targets_are_rejected() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    let backup = collect(
        source.path(),
        "same-device".into(),
        BTreeMap::new(),
        &decrypt,
    )
    .unwrap();
    let (plan, outcome) =
        prepare(target.path(), backup, "same-device", &encrypt, &decrypt).unwrap();
    publish(&plan).unwrap();
    assert_eq!(outcome.skipped, 0);
    let row = records::json_at(&target.path().join("cursor-accounts.json"))
        .unwrap()
        .unwrap();
    assert_eq!(row["native-fixture"]["session"], "os-bound-ciphertext");
    assert_eq!(row["native-fixture"]["applied"], false);
    #[cfg(unix)]
    {
        let other = tempfile::tempdir().unwrap();
        let file = other.path().join("settings.json");
        fs::write(&file, b"outside").unwrap();
        let link = target.path().join("symbolic.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(add_plan(target.path(), link, &json!({}), &mut vec![]).is_err());
        assert_eq!(fs::read(file).unwrap(), b"outside");
    }
}

#[test]
fn foreign_device_bound_duplicate_preserves_existing_account() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    fixture(source.path());
    write(
        target.path(),
        "cursor-accounts.json",
        json!({"local-id":{"session":"destination-native-session","applied":true,"email":"native@example.test","usage":null}}),
    );
    let before = fs::read(target.path().join("cursor-accounts.json")).unwrap();
    let backup = collect(
        source.path(),
        "source-device".into(),
        BTreeMap::new(),
        &decrypt,
    )
    .unwrap();
    let (plan, outcome) =
        prepare(target.path(), backup, "target-device", &encrypt, &decrypt).unwrap();
    assert_eq!(outcome.device_bound, 0);
    assert_eq!(outcome.skipped, 0);
    assert_eq!(outcome.duplicates, 1);
    publish(&plan).unwrap();
    assert_eq!(
        fs::read(target.path().join("cursor-accounts.json")).unwrap(),
        before
    );
}
