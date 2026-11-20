use super::*;
use serde_json::json;

thread_local! {
    static MUTATION_STAGE: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn after_mutation_stage() {
    MUTATION_STAGE.with(|callback| {
        if let Some(callback) = callback.borrow().as_ref() {
            callback();
        }
    });
}

struct Fixture {
    dir: std::path::PathBuf,
    settings: std::path::PathBuf,
    relay: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("claude-config-safety-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self {
            settings: dir.join("settings.json"),
            relay: dir.join("relay.json"),
            dir,
        }
    }

    fn apply(&self, relay: bool) -> ApplyResult {
        apply_claudecode_at(&model(relay), &self.settings, &self.relay)
    }

    fn restore(&self) -> ApplyResult {
        restore_claudecode_at(&self.settings, &self.relay)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn model(relay: bool) -> ModelInfo {
    ModelInfo {
        name: Some("Fixture model".into()),
        model: Some("fixture-model".into()),
        base_url: Some("https://upstream.example/v1".into()),
        api_key: Some("fixture-key".into()),
        anthropic_url: None,
        protocol: Some("anthropic".into()),
        display_model: None,
        relay_mode: Some(relay),
        web_search: None,
        image_input: None,
        one_m_context: None,
    }
}

#[test]
fn web_search_settings_toggle_and_restore_only_the_owned_tool_rule() {
    for original in [
        json!({"env":{"KEEP":"yes"}}),
        json!({"permissions":{}}),
        json!({"permissions":{"deny":[]}}),
        json!({"permissions":{"allow":["WebSearch"],"ask":["Bash"],"deny":["Read(secret)","WebSearch","WebSearch","Edit(secret)"]}}),
    ] {
        let fixture = Fixture::new();
        write_json_file(&fixture.settings, &original).unwrap();
        for enabled in [false, false, true, false] {
            let mut info = model(true);
            info.web_search = Some(enabled);
            assert!(apply_claudecode_at(&info, &fixture.settings, &fixture.relay).success);
            let current = read_json_file(&fixture.settings).unwrap();
            let denied = current["permissions"]["deny"]
                .as_array()
                .is_some_and(|rules| rules.contains(&json!("WebSearch")));
            assert_eq!(denied, !enabled);
            assert_eq!(
                current["permissions"]["allow"],
                original["permissions"]["allow"]
            );
            assert_eq!(
                current["permissions"]["ask"],
                original["permissions"]["ask"]
            );
        }
        assert!(fixture.restore().success);
        assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
    }
}

#[test]
fn web_search_restore_preserves_new_denials_and_user_removal() {
    for user_removes_search in [false, true] {
        let fixture = Fixture::new();
        write_json_file(
            &fixture.settings,
            &json!({"permissions":{"deny":["Read(secret)"]}}),
        )
        .unwrap();
        let mut info = model(true);
        info.web_search = Some(false);
        assert!(apply_claudecode_at(&info, &fixture.settings, &fixture.relay).success);
        let mut current = read_json_file(&fixture.settings).unwrap();
        let deny = current["permissions"]["deny"].as_array_mut().unwrap();
        assert!(deny.contains(&json!("WebSearch")));
        deny.push(json!("Bash(rm *)"));
        if user_removes_search {
            deny.retain(|rule| rule != "WebSearch");
        }
        write_json_file(&fixture.settings, &current).unwrap();
        assert!(fixture.restore().success);
        assert_eq!(
            read_json_file(&fixture.settings).unwrap(),
            json!({"permissions":{"deny":["Read(secret)","Bash(rm *)"]}})
        );
    }
}

#[test]
fn web_search_invalid_permissions_are_not_overwritten() {
    for original in [
        json!({"permissions":[]}),
        json!({"permissions":{"deny":"keep"}}),
    ] {
        let fixture = Fixture::new();
        write_json_file(&fixture.settings, &original).unwrap();
        let mut info = model(true);
        info.web_search = Some(false);
        assert!(!apply_claudecode_at(&info, &fixture.settings, &fixture.relay).success);
        assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
        assert!(!fixture.relay.exists());
    }
}

#[test]
fn managed_config_restores_original_models_keys_and_enabled_cloud_switches() {
    let fixture = Fixture::new();
    let original = json!({"permissions":{"allow":["Read"]},"env":{
        "ANTHROPIC_BASE_URL":"https://original.example",
        "ANTHROPIC_API_KEY":"original-key","ANTHROPIC_AUTH_TOKEN":"original-token",
        "ANTHROPIC_MODEL":"original-model","ANTHROPIC_DEFAULT_OPUS_MODEL":"original-opus",
        "API_TIMEOUT_MS":12345,"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC":"0",
        "CLAUDE_CODE_USE_BEDROCK":"  YES ","CLAUDE_CODE_USE_VERTEX":"true",
        "CLAUDE_CODE_USE_FOUNDRY":"1","CLAUDE_CODE_USE_ANTHROPIC_AWS":"on",
        "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD":"TRUE","CLAUDE_CODE_USE_MANTLE":"1",
        "AWS_PROFILE":"keep-profile","GOOGLE_CLOUD_PROJECT":"keep-project","KEEP":null
    }});
    write_json_file(&fixture.settings, &original).unwrap();
    for mode in [false, false, true, true, false] {
        let result = fixture.apply(mode);
        assert!(result.success, "{}", result.message);
        let applied = read_json_file(&fixture.settings).unwrap();
        for key in [
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
            "CLAUDE_CODE_USE_ANTHROPIC_AWS",
            "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
            "CLAUDE_CODE_USE_MANTLE",
        ] {
            assert!(
                applied["env"].get(key).is_none(),
                "enabled switch remains: {key}"
            );
        }
        assert_eq!(applied["env"]["AWS_PROFILE"], "keep-profile");
        assert_eq!(applied["env"]["GOOGLE_CLOUD_PROJECT"], "keep-project");
    }
    assert!(fixture.restore().success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
}

#[test]
fn managed_config_preserves_disabled_cloud_switches() {
    for value in [
        json!("0"),
        json!("false"),
        json!("off"),
        json!("no"),
        json!(false),
        json!(0),
    ] {
        let fixture = Fixture::new();
        let original = json!({"env":{"CLAUDE_CODE_USE_BEDROCK":value,"KEEP":"yes"}});
        write_json_file(&fixture.settings, &original).unwrap();
        assert!(fixture.apply(false).success);
        assert_eq!(
            read_json_file(&fixture.settings).unwrap()["env"]["CLAUDE_CODE_USE_BEDROCK"],
            value
        );
        assert!(fixture.restore().success);
        assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
    }
}

#[test]
fn managed_config_inherited_provider_detection_is_read_only_and_names_the_conflict() {
    for key in CLOUD_PROVIDER_VARS {
        for value in ["1", " true ", "YeS", "ON"] {
            assert_eq!(
                inherited_provider(|name| (name == key).then(|| value.to_string()), None),
                Some(key)
            );
        }
        for value in ["", "0", "false", "off", "no"] {
            assert!(
                inherited_provider(|name| (name == key).then(|| value.to_string()), None).is_none()
            );
        }
    }
    assert!(inherited_provider(|_| None, None).is_none());
}

#[test]
fn managed_config_existing_disabled_overrides_mask_inherited_cloud_switches() {
    for key in CLOUD_PROVIDER_VARS {
        for value in ["", "0", "false", "off"] {
            let fixture = Fixture::new();
            let mut original = json!({"env":{"KEEP":"yes"}});
            original["env"][key] = json!(value);
            write_json_file(&fixture.settings, &original).unwrap();
            let result = apply_claudecode_with_environment(
                &model(false),
                &fixture.settings,
                &fixture.relay,
                |name| (name == key).then(|| "1".to_string()),
            );
            assert!(result.success, "{key}={value:?}: {}", result.message);
            assert_eq!(
                read_json_file(&fixture.settings).unwrap()["env"][key],
                value
            );
            assert!(fixture.restore().success);
            assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
        }
        let fixture = Fixture::new();
        let original = json!({"env":{"KEEP":"yes"}});
        write_json_file(&fixture.settings, &original).unwrap();
        let result = apply_claudecode_with_environment(
            &model(false),
            &fixture.settings,
            &fixture.relay,
            |name| (name == key).then(|| "1".to_string()),
        );
        assert!(!result.success);
        assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
        assert!(!fixture.relay.exists());
    }
}

#[test]
fn managed_config_restore_keeps_later_user_edits_and_removals() {
    let fixture = Fixture::new();
    write_json_file(
        &fixture.settings,
        &json!({"env":{"ANTHROPIC_MODEL":"old","ANTHROPIC_API_KEY":"old-key","KEEP":"yes"}}),
    )
    .unwrap();
    assert!(fixture.apply(true).success);
    let mut edited = read_json_file(&fixture.settings).unwrap();
    edited["env"]["ANTHROPIC_MODEL"] = json!("user-new-model");
    edited["env"]["ANTHROPIC_API_KEY"] = json!("user-new-key");
    edited["env"]["CLAUDE_CODE_USE_BEDROCK"] = json!("1");
    edited["env"]
        .as_object_mut()
        .unwrap()
        .remove("ANTHROPIC_BASE_URL");
    write_json_file(&fixture.settings, &edited).unwrap();
    assert!(fixture.restore().success);
    assert_eq!(
        read_json_file(&fixture.settings).unwrap(),
        json!({"env":{
            "ANTHROPIC_MODEL":"user-new-model","ANTHROPIC_API_KEY":"user-new-key",
            "CLAUDE_CODE_USE_BEDROCK":"1","KEEP":"yes"
        }})
    );
}

#[test]
fn managed_config_reapply_saves_a_user_override_before_taking_it_over_again() {
    let fixture = Fixture::new();
    write_json_file(
        &fixture.settings,
        &json!({"env":{"ANTHROPIC_MODEL":"old","KEEP":"yes"}}),
    )
    .unwrap();
    assert!(fixture.apply(true).success);
    let mut edited = read_json_file(&fixture.settings).unwrap();
    edited["env"]["ANTHROPIC_MODEL"] = json!("user-replacement");
    write_json_file(&fixture.settings, &edited).unwrap();
    assert!(fixture.apply(true).success);
    assert!(fixture.apply(false).success);
    assert!(fixture.restore().success);
    assert_eq!(
        read_json_file(&fixture.settings).unwrap(),
        json!({"env":{"ANTHROPIC_MODEL":"user-replacement","KEEP":"yes"}})
    );
}

#[test]
fn managed_config_missing_relay_does_not_erase_an_official_user_configuration() {
    let fixture = Fixture::new();
    let original =
        json!({"env":{"ANTHROPIC_MODEL":"own-model","ANTHROPIC_API_KEY":"own-key","KEEP":"yes"}});
    write_json_file(&fixture.settings, &original).unwrap();
    assert!(fixture.restore().success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
}

#[test]
fn managed_config_corrupt_relay_blocks_apply_and_restore_without_mutation() {
    for raw in [
        b"{\"baseUrl\":".as_slice(),
        b"[]",
        b"{\"managedSettings\":[]}",
    ] {
        let fixture = Fixture::new();
        let original = json!({"env":{"KEEP":"yes"}});
        write_json_file(&fixture.settings, &original).unwrap();
        fs::write(&fixture.relay, raw).unwrap();
        assert!(
            !fixture.apply(false).success,
            "apply accepted an unreadable ownership record"
        );
        assert!(
            !fixture.restore().success,
            "restore accepted an unreadable ownership record"
        );
        assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
        assert_eq!(fs::read(&fixture.relay).unwrap(), raw);
    }
}

#[test]
fn managed_config_failed_relay_write_leaves_the_entire_settings_file_unchanged() {
    let fixture = Fixture::new();
    let raw = b"{\n  \"env\": {\"ANTHROPIC_MODEL\": \"original\", \"KEEP\": \"yes\"}\n}\n";
    fs::write(&fixture.settings, raw).unwrap();
    fs::create_dir(&fixture.relay).unwrap();
    assert!(!fixture.apply(false).success);
    assert_eq!(fs::read(&fixture.settings).unwrap(), raw);
}

#[test]
fn managed_config_invalid_env_shape_is_never_replaced() {
    let fixture = Fixture::new();
    let original = json!({"env":["user-data"],"permissions":{"allow":["Read"]}});
    write_json_file(&fixture.settings, &original).unwrap();
    assert!(!fixture.apply(false).success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
    assert!(!fixture.relay.exists());
}

#[test]
fn managed_config_restore_does_not_recreate_a_settings_file_deleted_by_the_user() {
    let fixture = Fixture::new();
    assert!(fixture.apply(false).success);
    fs::remove_file(&fixture.settings).unwrap();
    assert!(fixture.restore().success);
    assert!(!fixture.settings.exists());
    assert!(!fixture.relay.exists());
}

#[test]
fn managed_config_restores_missing_env_without_removing_unrelated_settings() {
    let fixture = Fixture::new();
    let original = json!({"permissions":{"allow":["Read"]}});
    write_json_file(&fixture.settings, &original).unwrap();
    assert!(fixture.apply(false).success);
    assert!(fixture.restore().success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
    fs::remove_file(&fixture.settings).unwrap();
    assert!(fixture.apply(false).success);
    assert!(fixture.restore().success);
    assert!(!fixture.settings.exists());
}

#[test]
fn managed_config_legacy_restore_only_cleans_values_it_can_identify() {
    let fixture = Fixture::new();
    write_json_file(
        &fixture.relay,
        &json!({
            "baseUrl":"https://upstream.example","apiKey":"fixture-key",
            "actualModel":"fixture-model","relayMode":true
        }),
    )
    .unwrap();
    write_json_file(
        &fixture.settings,
        &json!({"env":{
            "ANTHROPIC_BASE_URL":"https://upstream.example","ANTHROPIC_AUTH_TOKEN":"fixture-key",
            "ANTHROPIC_MODEL":"fixture-model[1m]","ANTHROPIC_DEFAULT_OPUS_MODEL":"user-edit",
            "ANTHROPIC_API_KEY":"user-added-key","API_TIMEOUT_MS":"3000000","KEEP":"yes"
        }}),
    )
    .unwrap();
    assert!(fixture.restore().success);
    assert_eq!(
        read_json_file(&fixture.settings).unwrap(),
        json!({"env":{
            "ANTHROPIC_DEFAULT_OPUS_MODEL":"user-edit","ANTHROPIC_API_KEY":"user-added-key","KEEP":"yes"
        }})
    );
}

#[test]
fn managed_config_legacy_apply_does_not_snapshot_its_old_proxy_as_the_original() {
    let fixture = Fixture::new();
    let legacy = json!({"baseUrl":"https://upstream.example","apiKey":"fixture-key",
        "actualModel":"fixture-model","relayMode":false,
        "autoModeServerSettingsPath":fixture.settings.to_string_lossy()});
    write_json_file(&fixture.relay, &legacy).unwrap();
    write_json_file(
        &fixture.settings,
        &json!({"env":{
            "ANTHROPIC_BASE_URL":"http://127.0.0.1:55534/claudecode",
            "ANTHROPIC_AUTH_TOKEN":"echobird-local-proxy","API_TIMEOUT_MS":"3000000",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC":"1","CLAUDE_CODE_AUTO_MODE_SERVER":"0",
            "KEEP":"yes"
        }}),
    )
    .unwrap();
    assert!(fixture.apply(true).success);
    assert!(fixture.restore().success);
    assert_eq!(
        read_json_file(&fixture.settings).unwrap(),
        json!({"env":{"KEEP":"yes"}})
    );
}

#[test]
fn managed_config_missing_relay_cannot_report_a_live_local_bridge_as_restored() {
    let fixture = Fixture::new();
    let original = json!({"env":{
        "ANTHROPIC_BASE_URL":"http://127.0.0.1:55534/claudecode",
        "ANTHROPIC_AUTH_TOKEN":"echobird-local-proxy","KEEP":"yes"
    }});
    write_json_file(&fixture.settings, &original).unwrap();
    assert!(!fixture.apply(true).success);
    assert!(!fixture.restore().success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
    assert!(!fixture.relay.exists());
}

#[test]
fn managed_config_failed_restore_keeps_the_relay_for_repair() {
    let fixture = Fixture::new();
    assert!(fixture.apply(false).success);
    let saved = fs::read(&fixture.relay).unwrap();
    fs::write(&fixture.settings, b"{\"env\":").unwrap();
    assert!(!fixture.restore().success);
    assert_eq!(fs::read(&fixture.relay).unwrap(), saved);
    assert_eq!(fs::read(&fixture.settings).unwrap(), b"{\"env\":");
}

#[test]
fn failed_restore_preserves_settings_written_during_the_operation() {
    let fixture = Fixture::new();
    write_json_file(
        &fixture.settings,
        &json!({"env":{"ANTHROPIC_MODEL":"original"}}),
    )
    .unwrap();
    assert!(fixture.apply(true).success);
    let settings = fixture.settings.clone();
    let relay = fixture.relay.clone();
    MUTATION_STAGE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            let mut current = read_json_file(&settings).unwrap();
            current["language"] = json!("ja");
            write_json_file(&settings, &current).unwrap();
            let mut current = read_json_file(&relay).unwrap();
            current["externalEdit"] = json!(true);
            write_json_file(&relay, &current).unwrap();
        }));
    });
    let result = fixture.restore();
    MUTATION_STAGE.with(|hook| hook.borrow_mut().take());
    assert!(!result.success);
    assert_eq!(read_json_file(&fixture.settings).unwrap()["language"], "ja");
    assert_eq!(
        read_json_file(&fixture.relay).unwrap()["externalEdit"],
        true
    );
}

#[cfg(windows)]
#[test]
fn managed_config_settings_write_failure_rolls_back_the_saved_relay() {
    let fixture = Fixture::new();
    write_json_file(
        &fixture.settings,
        &json!({"env":{"ANTHROPIC_MODEL":"original"}}),
    )
    .unwrap();
    let original = fs::read(&fixture.settings).unwrap();
    let original_permissions = fs::metadata(&fixture.settings).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&fixture.settings, permissions.clone()).unwrap();
    let result = fixture.apply(true);
    fs::set_permissions(&fixture.settings, original_permissions).unwrap();
    assert!(!result.success);
    assert_eq!(fs::read(&fixture.settings).unwrap(), original);
    assert!(!fixture.relay.exists());
}

#[cfg(windows)]
#[test]
fn managed_config_relay_removal_failure_rolls_back_restored_settings() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    write_json_file(
        &fixture.settings,
        &json!({"env":{"ANTHROPIC_MODEL":"original"}}),
    )
    .unwrap();
    assert!(fixture.apply(true).success);
    let settings = fs::read(&fixture.settings).unwrap();
    let relay = fs::read(&fixture.relay).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&fixture.relay)
        .unwrap();
    assert!(fs::remove_file(&fixture.relay).is_err());
    let result = fixture.restore();
    drop(held);
    assert!(!result.success);
    assert_eq!(fs::read(&fixture.settings).unwrap(), settings);
    assert_eq!(fs::read(&fixture.relay).unwrap(), relay);
}

#[cfg(unix)]
#[test]
fn managed_config_relay_with_credentials_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    assert!(fixture.apply(false).success);
    assert_eq!(
        fs::metadata(&fixture.relay).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn managed_config_unknown_dangling_settings_link_is_preserved_without_creating_files() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let relative = std::path::Path::new("missing/settings.json");
    let target = fixture.dir.join(relative);
    symlink(relative, &fixture.settings).unwrap();
    assert!(!fixture.apply(false).success);
    assert!(!fixture.restore().success);
    assert_eq!(fs::read_link(&fixture.settings).unwrap(), relative);
    assert!(!target.exists());
    assert!(!target.parent().unwrap().exists());
    assert!(!fixture.relay.exists());
}

#[cfg(unix)]
#[test]
fn managed_config_preserves_relative_settings_links_and_directory_modes_through_rollback() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = Fixture::new();
    let profile_dir = fixture.dir.join("profile");
    let target_dir = fixture.dir.join("shared");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::set_permissions(&profile_dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o750)).unwrap();
    let settings = profile_dir.join("settings.json");
    let relative = std::path::Path::new("../shared/settings.json");
    let target = target_dir.join("settings.json");
    let relay = fixture.dir.join("state/relay.json");
    let original = json!({"env":{"ANTHROPIC_MODEL":"original","KEEP":"yes"}});
    write_json_file(&target, &original).unwrap();
    symlink(relative, &settings).unwrap();
    for mode in [false, true] {
        assert!(apply_claudecode_at(&model(mode), &settings, &relay).success);
        assert_eq!(fs::read_link(&settings).unwrap(), relative);
        assert!(read_json_file(&target).unwrap()["env"]["ANTHROPIC_BASE_URL"].is_string());
        assert_eq!(
            fs::metadata(&target_dir).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(&profile_dir).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let applied = fs::read(&target).unwrap();
    let original_relay = fs::read(&relay).unwrap();
    let held_relay = relay.with_extension("held");
    let callback_relay = relay.clone();
    let callback_held = held_relay.clone();
    MUTATION_STAGE.with(|callback| {
        *callback.borrow_mut() = Some(Box::new(move || {
            fs::rename(&callback_relay, &callback_held).unwrap();
            fs::create_dir(&callback_relay).unwrap();
            assert!(fs::remove_file(&callback_relay).is_err());
        }));
    });
    let failed = restore_claudecode_at(&settings, &relay);
    MUTATION_STAGE.with(|callback| {
        *callback.borrow_mut() = None;
    });
    fs::remove_dir(&relay).unwrap();
    fs::rename(&held_relay, &relay).unwrap();
    assert!(!failed.success);
    assert_eq!(fs::read_link(&settings).unwrap(), relative);
    assert_eq!(fs::read(&target).unwrap(), applied);
    assert_eq!(fs::read(&relay).unwrap(), original_relay);
    assert_eq!(
        fs::metadata(&target_dir).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::metadata(&profile_dir).unwrap().permissions().mode() & 0o777,
        0o755
    );

    assert!(restore_claudecode_at(&settings, &relay).success);
    assert_eq!(fs::read_link(&settings).unwrap(), relative);
    assert_eq!(read_json_file(&target).unwrap(), original);
    assert_eq!(
        fs::metadata(&target_dir).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::metadata(&profile_dir).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn managed_config_retains_each_profile_original_when_the_other_profile_is_restored() {
    for first_restore in ["first", "second"] {
        let fixture = Fixture::new();
        let first = fixture.dir.join("first/settings.json");
        let second = fixture.dir.join("second/settings.json");
        let original_first = json!({"env":{"ANTHROPIC_MODEL":"original-first","ANTHROPIC_API_KEY":"first-key","KEEP":"first"}});
        let original_second = json!({"env":{"ANTHROPIC_MODEL":"original-second","ANTHROPIC_API_KEY":"second-key","KEEP":"second"}});
        write_json_file(&first, &original_first).unwrap();
        write_json_file(&second, &original_second).unwrap();
        let mut first_model = model(false);
        first_model.model = Some("first-model".into());
        assert!(apply_claudecode_at(&first_model, &first, &fixture.relay).success);
        assert!(apply_claudecode_at(&model(true), &second, &fixture.relay).success);
        assert_eq!(
            read_claudecode_at(&first, &fixture.relay)
                .unwrap()
                .model
                .as_deref(),
            Some("first-model")
        );
        let order = if first_restore == "first" {
            [&first, &second]
        } else {
            [&second, &first]
        };
        let result = restore_claudecode_at(order[0], &fixture.relay);
        assert!(result.success, "{}", result.message);
        assert!(
            fixture.relay.exists(),
            "another profile still needs its relay and original values"
        );
        assert!(read_claudecode_at(order[0], &fixture.relay).is_none());
        assert_eq!(
            read_claudecode_at(order[1], &fixture.relay)
                .unwrap()
                .model
                .as_deref(),
            Some(if first_restore == "first" {
                "fixture-model"
            } else {
                "first-model"
            })
        );
        let result = restore_claudecode_at(order[1], &fixture.relay);
        assert!(result.success, "{}", result.message);
        assert_eq!(read_json_file(&first).unwrap(), original_first);
        assert_eq!(read_json_file(&second).unwrap(), original_second);
        assert!(!fixture.relay.exists());
    }
}

#[test]
fn managed_config_unknown_legacy_profile_never_snapshots_its_proxy_as_original() {
    let fixture = Fixture::new();
    let original = json!({"env":{"ANTHROPIC_MODEL":"original-first","KEEP":"first"}});
    write_json_file(&fixture.settings, &original).unwrap();
    assert!(fixture.apply(false).success);
    let legacy_path = fixture.dir.join("legacy/settings.json");
    write_json_file(
        &legacy_path,
        &json!({"env":{
            "ANTHROPIC_BASE_URL":"http://127.0.0.1:55534/claudecode",
            "ANTHROPIC_AUTH_TOKEN":"echobird-local-proxy","KEEP":"legacy"
        }}),
    )
    .unwrap();
    let first = fs::read(&fixture.settings).unwrap();
    let legacy = fs::read(&legacy_path).unwrap();
    let relay = fs::read(&fixture.relay).unwrap();
    let result = apply_claudecode_at(&model(true), &legacy_path, &fixture.relay);
    assert!(
        !result.success,
        "a legacy proxy cannot become its own original settings"
    );
    assert_eq!(fs::read(&fixture.settings).unwrap(), first);
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy);
    assert_eq!(fs::read(&fixture.relay).unwrap(), relay);
    assert!(!restore_claudecode_at(&legacy_path, &fixture.relay).success);
    assert_eq!(fs::read(&fixture.relay).unwrap(), relay);
    assert!(fixture.restore().success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
    assert!(!restore_claudecode_at(&legacy_path, &fixture.relay).success);
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy);
}

fn overlapping_mutations(restoring: bool) {
    use std::{sync::mpsc, thread, time::Duration};
    let fixture = Fixture::new();
    let original = json!({"env":{"ANTHROPIC_MODEL":"original","KEEP":"yes"}});
    write_json_file(&fixture.settings, &original).unwrap();
    if restoring {
        assert!(fixture.apply(true).success);
    }
    let (staged_tx, staged_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let settings = fixture.settings.clone();
    let relay = fixture.relay.clone();
    let first = thread::spawn(move || {
        MUTATION_STAGE.with(|callback| {
            *callback.borrow_mut() = Some(Box::new(move || {
                staged_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }));
        });
        if restoring {
            restore_claudecode_at(&settings, &relay)
        } else {
            apply_claudecode_at(&model(false), &settings, &relay)
        }
    });
    staged_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (finished_tx, finished_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let settings = fixture.settings.clone();
    let relay = fixture.relay.clone();
    let second = thread::spawn(move || {
        let mut next = model(true);
        next.model = Some("second-model".into());
        started_tx.send(()).unwrap();
        finished_tx
            .send(apply_claudecode_at(&next, &settings, &relay))
            .unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let early = finished_rx.recv_timeout(Duration::from_secs(2)).ok();
    resume_tx.send(()).unwrap();
    let first = first.join().unwrap();
    let second_result =
        early.unwrap_or_else(|| finished_rx.recv_timeout(Duration::from_secs(10)).unwrap());
    second.join().unwrap();
    assert!(
        first.success,
        "first mutation failed after another completed: {}",
        first.message
    );
    assert!(second_result.success, "{}", second_result.message);
    assert_eq!(
        read_json_file(&fixture.relay).unwrap()["actualModel"],
        "second-model"
    );
    assert_eq!(
        read_json_file(&fixture.settings).unwrap()["env"]["ANTHROPIC_MODEL"],
        "second-model"
    );
    assert!(fixture.restore().success);
    assert_eq!(read_json_file(&fixture.settings).unwrap(), original);
}

#[test]
fn managed_config_overlapping_applies_never_remove_another_successful_relay() {
    overlapping_mutations(false);
}

#[test]
fn managed_config_restore_and_apply_share_one_mutation_boundary() {
    overlapping_mutations(true);
}
