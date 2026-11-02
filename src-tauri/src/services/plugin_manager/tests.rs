use super::*;
use serde_json::json;
use std::io::{Cursor, Write};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    manager: PluginManager,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("echobird-plugin-test-{}", uuid::Uuid::new_v4()));
        let source = root.join("source");
        fs::create_dir_all(source.join("ui")).unwrap();
        fs::write(source.join("ui/index.html"), "<h1>Plugin fixture</h1>").unwrap();
        fs::write(
            source.join("package.json"),
            serde_json::to_vec(&manifest_value()).unwrap(),
        )
        .unwrap();
        let manager = PluginManager::new(root.join("storage"));
        Self {
            root,
            source,
            manager,
        }
    }

    async fn preview(&self) -> Candidate {
        self.manager
            .preview(
                "directory",
                self.source.to_str().unwrap(),
                "international",
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}

fn manifest_value() -> serde_json::Value {
    json!({"name":"game", "publisher":"fixture", "version":"1.0.0", "displayName":"Game", "description":"A local fixture", "license":"MIT",
        "repository":"https://example.com/fixture/game", "scripts":{"postinstall":"must-not-run"},
        "echobird":{"manifestVersion":1, "apiVersion":"0.1-draft", "platforms":["windows","macos","linux"], "permissions":["model.request"],
        "contributes":{"pages":[{"id":"game","title":"Game","entry":"ui/index.html"}]}}})
}

#[test]
fn provider_contributions_reference_a_page_and_unique_application_targets() {
    let mut value = manifest_value();
    value["echobird"]["contributes"]["providers"] = json!([{
        "id":"claude-code",
        "title":"Claude Code accounts",
        "applications":["claudecode"],
        "pageId":"game",
        "searchTerms":["Claude Code"]
    }]);
    let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
    assert!(manifest.validate().is_ok());
    value["echobird"]["contributes"]["providers"][0]["pageId"] = json!("missing");
    let manifest: Manifest = serde_json::from_value(value).unwrap();
    assert_eq!(
        manifest.validate().unwrap_err(),
        "Invalid provider contribution"
    );
}

#[tokio::test]
async fn installing_a_new_version_preserves_disabled_state_on_disk() {
    let fixture = Fixture::new();
    let first = fixture.preview().await;
    assert!(fixture.manager.install(&first.token).unwrap()[0].enabled);
    fixture.manager.set_enabled("fixture.game", false).unwrap();
    let mut manifest = manifest_value();
    manifest["version"] = json!("1.1.0");
    fs::write(
        fixture.source.join("package.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let next = fixture.preview().await;
    let installed = fixture.manager.install(&next.token).unwrap();
    assert_eq!(installed[0].manifest.version, "1.1.0");
    assert!(!installed[0].enabled);
    let reopened = PluginManager::new(fixture.root.join("storage"));
    assert!(!reopened.list().unwrap()[0].enabled);
}

#[test]
fn account_worker_requires_permission_and_declared_operations() {
    let mut value = manifest_value();
    value["echobird"]["worker"] = json!({"entry":"account.mjs"});
    value["echobird"]["contributes"]["providers"] = json!([{
        "id":"fixture-account", "title":"Fixture account",
        "applications":["fixture"], "pageId":"game", "operations":["accounts"]
    }]);
    let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
    assert!(manifest.validate().is_err());
    value["echobird"]["permissions"] = json!(["account.code"]);
    value["echobird"]["contributes"]["pages"] = json!([]);
    value["echobird"]["contributes"]["providers"][0]
        .as_object_mut()
        .unwrap()
        .remove("pageId");
    let manifest: Manifest = serde_json::from_value(value.clone()).unwrap();
    manifest.validate().unwrap();
    value["echobird"]["contributes"]["providers"][0]["operations"] = json!(["unknown"]);
    let manifest: Manifest = serde_json::from_value(value).unwrap();
    assert!(manifest.validate().is_err());
}

#[tokio::test]
async fn pausing_discards_staged_installs_without_disabling_installed_plugins() {
    let fixture = Fixture::new();
    let first = fixture.preview().await;
    let installed = fixture.manager.install(&first.token).unwrap();
    let pending = fixture.preview().await;
    fixture.manager.pause();
    assert!(!fixture.manager.staging(&pending.token).exists());
    assert!(fixture.manager.install(&pending.token).is_err());
    let kept = fixture.manager.list().unwrap();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].enabled);
    assert!(fixture
        .manager
        .package_root(&installed[0])
        .unwrap()
        .exists());
    assert!(fixture.source.join("package.json").exists());
}

#[tokio::test]
async fn cancelled_preview_never_creates_staging_or_changes_installed_state() {
    let fixture = Fixture::new();
    let cancelled = tokio_util::sync::CancellationToken::new();
    cancelled.cancel();
    assert!(fixture
        .manager
        .preview(
            "directory",
            fixture.source.to_str().unwrap(),
            "international",
            cancelled
        )
        .await
        .is_err());
    assert!(!fixture.manager.root.exists());
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[tokio::test]
async fn plugin_install_snapshot_survives_source_changes_and_persists_enablement() {
    let fixture = Fixture::new();
    assert!(fixture.manager.list().unwrap().is_empty());
    assert!(
        !fixture.manager.root.exists(),
        "Passive loading must not write"
    );
    let candidate = fixture.preview().await;
    assert!(
        fixture.manager.list().unwrap().is_empty(),
        "Preview must not install"
    );
    fs::write(
        fixture.source.join("ui/index.html"),
        "changed after preview",
    )
    .unwrap();
    let installed = fixture.manager.install(&candidate.token).unwrap();
    assert_eq!(installed.len(), 1);
    let package = fixture.manager.package_root(&installed[0]).unwrap();
    assert_eq!(
        fs::read_to_string(package.join("ui/index.html")).unwrap(),
        "<h1>Plugin fixture</h1>"
    );
    let reopened = PluginManager::new(fixture.manager.root.clone());
    assert!(reopened.list().unwrap()[0].enabled);
    reopened.set_enabled("fixture.game", false).unwrap();
    assert!(!fixture.manager.list().unwrap()[0].enabled);
    assert!(fixture
        .manager
        .uninstall("fixture.game")
        .unwrap()
        .is_empty());
    assert!(!package.exists());
    assert!(fixture.source.join("ui/index.html").exists());
}

#[tokio::test]
async fn plugin_cancel_and_expired_preview_cannot_install() {
    let fixture = Fixture::new();
    let candidate = fixture.preview().await;
    fixture.manager.cancel(&candidate.token).unwrap();
    assert!(!fixture.manager.staging(&candidate.token).exists());
    assert!(fixture.manager.install(&candidate.token).is_err());
    assert!(fixture.manager.list().unwrap().is_empty());
}

#[tokio::test]
async fn plugin_failed_update_preserves_existing_installation() {
    let fixture = Fixture::new();
    let first = fixture.preview().await;
    let old = fixture.manager.install(&first.token).unwrap();
    let candidate = fixture.preview().await;
    let path = fixture
        .manager
        .staging(&candidate.token)
        .join("package/package.json");
    let mut changed = manifest_value();
    changed["version"] = json!("2.0.0");
    fs::write(path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(fixture.manager.install(&candidate.token).is_err());
    assert_eq!(
        fixture.manager.list().unwrap()[0].directory,
        old[0].directory
    );
    assert!(fixture.manager.package_root(&old[0]).unwrap().is_dir());
    fixture.manager.cancel(&candidate.token).unwrap();
}

#[test]
fn plugin_manifest_rejects_unsupported_capabilities_and_unsafe_entries() {
    for (pointer, value) in [
        ("/echobird/permissions", json!(["process.exec"])),
        (
            "/echobird/permissions",
            json!(["model.request", "model.request"]),
        ),
        ("/echobird/apiVersion", json!("9")),
        (
            "/echobird/contributes/pages/0/entry",
            json!("../outside.html"),
        ),
        (
            "/echobird/contributes/pages/0/entry",
            json!("C:\\outside.html"),
        ),
        (
            "/echobird/contributes/pages/0/entry",
            json!("ui/%2e%2e/index.html"),
        ),
        (
            "/repository",
            json!("https://user:password@example.com/source"),
        ),
        ("/version", json!("latest")),
    ] {
        let mut value_base = manifest_value();
        *value_base.pointer_mut(pointer).unwrap() = value;
        assert!(
            serde_json::from_value::<Manifest>(value_base)
                .unwrap()
                .validate()
                .is_err(),
            "{pointer}"
        );
    }
    for path in [
        "/absolute.html",
        "a/../outside",
        "a\\outside",
        "a//index.html",
        "NUL.txt",
        "COM1.html",
        "ui/file.",
    ] {
        assert!(!manifest::relative_path(path), "{path}");
    }
    assert!(serde_json::from_value::<Manifest>(manifest_value())
        .unwrap()
        .validate()
        .is_ok());
}

#[test]
fn plugin_zip_accepts_a_regular_package_and_rejects_traversal_and_case_collisions() {
    let fixture = Fixture::new();
    let target = fixture.root.join("archive");
    packages::extract(&archive(&[("ui/index.html", b"fixture")]), &target).unwrap();
    assert_eq!(fs::read(target.join("ui/index.html")).unwrap(), b"fixture");
    for entries in [
        vec![("../outside", b"bad".as_slice())],
        vec![("C:/outside", b"bad".as_slice())],
        vec![
            ("ui/index.html", b"one".as_slice()),
            ("ui/INDEX.html", b"two".as_slice()),
        ],
    ] {
        assert!(packages::extract(&archive(&entries), &target).is_err());
    }
    assert!(!fixture.root.join("outside").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_directory_import_rejects_symlinks() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(
        fixture.source.join("package.json"),
        fixture.source.join("linked.json"),
    )
    .unwrap();
    assert!(fixture
        .manager
        .preview(
            "directory",
            fixture.source.to_str().unwrap(),
            "international",
            tokio_util::sync::CancellationToken::new()
        )
        .await
        .is_err());
    assert!(fixture.manager.list().unwrap().is_empty());
}

#[test]
fn plugin_bundled_registry_matches_public_copy() {
    let bundled = include_bytes!("../../../../src/data/pluginIndex.json");
    assert_eq!(
        bundled.as_slice(),
        include_bytes!("../../../../docs/api/plugins/index.json")
    );
    Registry::parse(bundled).unwrap();
}

#[test]
fn plugin_source_descriptions_round_trip_without_changing_releases() {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../src/data/pluginIndex.json")).unwrap();
    let index = value["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .position(|plugin| plugin["id"] == "echobird.reversi")
        .unwrap();
    let id = value["plugins"][index]["id"].as_str().unwrap().to_owned();
    let baseline = Registry::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    let releases = serde_json::to_value(
        &baseline
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .unwrap()
            .releases,
    )
    .unwrap();
    value["plugins"][index]["descriptions"] =
        json!({"international":"English source description","china":"中文来源说明"});
    let parsed = Registry::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    let entry = serde_json::to_value(
        parsed
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        entry["descriptions"],
        value["plugins"][index]["descriptions"]
    );
    assert_eq!(entry["releases"], releases);
    for invalid in [
        json!({"international":"English only"}),
        json!({"international":"English","china":"中文","other":"extra"}),
        json!({"international":"","china":"中文"}),
        json!({"international":"English","china":"x".repeat(501)}),
        json!({"international":42,"china":"中文"}),
    ] {
        value["plugins"][index]["descriptions"] = invalid;
        assert!(Registry::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    value["plugins"][index]
        .as_object_mut()
        .unwrap()
        .remove("descriptions");
    assert!(Registry::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
}

#[test]
fn plugin_registry_icons_are_optional_and_require_public_urls_in_both_regions() {
    let bundled = include_bytes!("../../../../src/data/pluginIndex.json");
    let mut value: serde_json::Value = serde_json::from_slice(bundled).unwrap();
    assert!(Registry::parse(bundled)
        .unwrap()
        .plugins
        .iter()
        .any(|p| p.icons.is_some()));
    value["plugins"][0]["icons"]["china"] = json!("file:///private.svg");
    assert!(Registry::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    value["plugins"][0].as_object_mut().unwrap().remove("icons");
    assert!(Registry::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["plugins"][0]["icons"] = json!({"international":"https://example.com/icon.svg"});
    assert!(Registry::parse(&serde_json::to_vec(&value).unwrap()).is_err());
}

#[test]
fn plugin_client_icons_are_optional_safe_filenames_independent_of_remote_images() {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../src/data/pluginIndex.json")).unwrap();
    let index = value["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .position(|plugin| plugin["id"] == "echobird.reversi")
        .unwrap();
    value["plugins"][index]
        .as_object_mut()
        .unwrap()
        .remove("icons");
    for file in ["workbuddy.png", "claudecode.svg"] {
        value["plugins"][index]["clientIcon"] = json!(file);
        let registry = Registry::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            registry
                .plugins
                .iter()
                .find(|plugin| plugin.id == "echobird.reversi")
                .unwrap()
                .client_icon
                .as_deref(),
            Some(file)
        );
    }
    for file in [
        "../private.png",
        "/icons/tools/workbuddy.png",
        "https://example.com/icon.png",
        "a/b.svg",
        "a.exe",
        "a.png?token=1",
        "",
    ] {
        value["plugins"][index]["clientIcon"] = json!(file);
        assert!(
            Registry::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{file}"
        );
    }
    value["plugins"][index]
        .as_object_mut()
        .unwrap()
        .remove("clientIcon");
    assert!(Registry::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
}

#[test]
fn plugin_package_checksum_is_checked_before_extraction() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let bytes = archive(&[("ui/index.html", b"approved")]);
    let target = fixture.root.join("verified");
    assert!(packages::verified_archive(&bytes, &"0".repeat(64), &target).is_err());
    assert!(!target.exists());
    packages::verified_archive(&bytes, &hex::encode(Sha256::digest(&bytes)), &target).unwrap();
    assert_eq!(fs::read(target.join("ui/index.html")).unwrap(), b"approved");
}

#[tokio::test]
#[ignore = "Requires the published community repositories; installs only into temporary storage"]
async fn plugin_live_community_account_packages_install_without_executing_code() {
    let fixture = Fixture::new();
    for application in [
        "claude-code-accounts",
        "claude-desktop-accounts",
        "doubao-work-accounts",
        "dola-accounts",
        "workbuddy-accounts",
        "workbuddy-ai-accounts",
        "trae-accounts",
        "trae-cn-accounts",
    ] {
        let id = format!("echobird-community.{application}");
        let candidate = fixture
            .manager
            .preview(
                "catalog",
                &id,
                "china",
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(candidate.manifest.echobird.contributes.pages.is_empty());
        let plugins = fixture.manager.install(&candidate.token).unwrap();
        let plugin = plugins.iter().find(|plugin| plugin.id == id).unwrap();
        let root = fixture.manager.package_root(plugin).unwrap();
        let worker = plugin.manifest.echobird.worker.as_ref().unwrap();
        assert_eq!(worker.runtime.as_deref(), Some("native"));
        assert_eq!(&fs::read(root.join(&worker.entry)).unwrap()[..2], b"MZ");
    }
    assert_eq!(fixture.manager.list().unwrap().len(), 8);
}

#[test]
fn zip_expansion_counts_implicit_directories_and_rejects_deep_paths() {
    let f = Fixture::new();
    let deep = format!("{}index.html", "dir/".repeat(33));
    assert!(packages::extract(&archive(&[(&deep, b"")]), &f.root.join("deep")).is_err());
    assert!(!f.root.join("deep").exists());
    let names: Vec<_> = (0..1001)
        .map(|index| format!("folder-{index}/file.txt"))
        .collect();
    let entries: Vec<_> = names.iter().map(|name| (name.as_str(), &b""[..])).collect();
    assert!(
        packages::extract(&archive(&entries), &f.root.join("implicit"))
            .unwrap_err()
            .contains("too many")
    );
    packages::extract(
        &archive(&[("shared/a.txt", b"a"), ("shared/b.txt", b"b")]),
        &f.root.join("valid"),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(f.root.join("valid/shared/b.txt")).unwrap(),
        b"b"
    );
}

#[test]
fn application_tools_require_a_service_for_the_same_application() {
    let mut value = serde_json::json!({
        "name":"tool","publisher":"fixture","version":"1.0.0","displayName":"Tool",
        "description":"Tool","license":"MIT","repository":"https://example.com/tool",
        "tools":[{"id":"fixture-tool","directory":"tools/fixture"}],
        "echobird":{"manifestVersion":1,"apiVersion":"0.1-draft",
            "platforms":["windows","macos","linux"],"permissions":["account.code"],
            "worker":{"runtime":"node","entry":"service.mjs"},
            "contributes":{"pages":[],"providers":[{"id":"fixture","title":"Fixture",
                "applications":["fixture-tool"],"operations":["accounts"]}]}}
    });
    let parse =
        |value: &serde_json::Value| serde_json::from_value::<Manifest>(value.clone()).unwrap();
    assert!(parse(&value).validate().is_ok());
    assert_eq!(parse(&value).tools()[0].id, "fixture-tool");

    value["echobird"]["contributes"]["providers"][0]["applications"][0] = "other".into();
    assert!(parse(&value).validate().is_err());
    value["echobird"]["contributes"]["providers"][0]["applications"][0] = "fixture-tool".into();
    value["tools"][0]["directory"] = "../outside".into();
    assert!(parse(&value).validate().is_err());
    value["tools"][0]["directory"] = "tools/fixture".into();
    value["echobird"]["worker"]["runtime"] = "native".into();
    assert!(parse(&value).validate().is_err());
}

async fn fetch_from(
    sources: &[(&str, Result<Vec<u8>, String>)],
    calls: &Mutex<Vec<String>>,
    url: String,
) -> Result<Vec<u8>, String> {
    calls.lock().unwrap().push(url.clone());
    sources
        .iter()
        .find(|(source, _)| *source == url)
        .map(|(_, result)| result.clone())
        .unwrap()
}

fn empty_index() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schemaVersion": 1,
        "name": "EchoBird Plugin Hub",
        "plugins": []
    }))
    .unwrap()
}

#[tokio::test]
async fn failed_mirror_fallback_reports_both_sources() {
    let calls = Mutex::new(Vec::new());
    let sources = [
        ("mirror", Err("mirror down".to_string())),
        ("origin", Err("origin down".to_string())),
    ];
    let error = packages::try_sources(
        "mirror",
        Some("origin"),
        |url| fetch_from(&sources, &calls, url),
        |bytes| Ok(bytes.to_vec()),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "GitCode: mirror down\nGitHub: origin down");
}

#[tokio::test]
async fn source_without_fallback_reports_its_own_failure() {
    let calls = Mutex::new(Vec::new());
    let sources = [("origin", Err("origin down".to_string()))];
    let error = packages::try_sources(
        "origin",
        None,
        |url| fetch_from(&sources, &calls, url),
        |bytes| Ok(bytes.to_vec()),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "origin down");
    assert_eq!(*calls.lock().unwrap(), ["origin"]);
}

#[tokio::test]
async fn accepted_mirror_response_does_not_touch_the_fallback() {
    let calls = Mutex::new(Vec::new());
    let sources = [("mirror", Ok(empty_index()))];
    packages::try_sources(
        "mirror",
        Some("origin"),
        |url| fetch_from(&sources, &calls, url),
        Registry::parse,
    )
    .await
    .unwrap();
    assert_eq!(*calls.lock().unwrap(), ["mirror"]);
}

#[tokio::test]
async fn unusable_mirror_index_falls_back_to_the_origin() {
    let calls = Mutex::new(Vec::new());
    let sources = [
        ("mirror", Ok(b"<html>gateway</html>".to_vec())),
        ("origin", Ok(empty_index())),
    ];
    let registry = packages::try_sources(
        "mirror",
        Some("origin"),
        |url| fetch_from(&sources, &calls, url),
        Registry::parse,
    )
    .await
    .unwrap();
    assert!(registry.plugins.is_empty());
    assert_eq!(*calls.lock().unwrap(), ["mirror", "origin"]);
}

#[tokio::test]
async fn stale_mirror_package_falls_back_and_installs_the_verified_bytes() {
    use sha2::{Digest, Sha256};
    let fixture = Fixture::new();
    let target = fixture.root.join("fallback");
    let good = archive(&[("ui/index.html", b"approved")]);
    let digest = hex::encode(Sha256::digest(&good));
    let calls = Mutex::new(Vec::new());
    let sources = [
        ("mirror", Ok(archive(&[("ui/index.html", b"stale")]))),
        ("origin", Ok(good)),
    ];
    packages::try_sources(
        "mirror",
        Some("origin"),
        |url| fetch_from(&sources, &calls, url),
        |bytes| packages::verified_archive(bytes, &digest, &target),
    )
    .await
    .unwrap();
    assert_eq!(*calls.lock().unwrap(), ["mirror", "origin"]);
    assert_eq!(fs::read(target.join("ui/index.html")).unwrap(), b"approved");
}

#[tokio::test]
async fn rejected_content_from_both_sources_reports_both() {
    let fixture = Fixture::new();
    let target = fixture.root.join("rejected");
    let calls = Mutex::new(Vec::new());
    let sources = [
        ("mirror", Ok(b"stale".to_vec())),
        ("origin", Ok(b"tampered".to_vec())),
    ];
    let error = packages::try_sources(
        "mirror",
        Some("origin"),
        |url| fetch_from(&sources, &calls, url),
        |bytes| packages::verified_archive(bytes, &"0".repeat(64), &target),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        "GitCode: Plugin package SHA-256 mismatch\nGitHub: Plugin package SHA-256 mismatch"
    );
    assert!(!target.exists());
}

#[tokio::test]
async fn network_errors_keep_their_underlying_cause() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let error = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap_err();
    let details = packages::error_details(&error);

    assert!(details.starts_with(&error.to_string()), "{details}");
    assert!(details.len() > error.to_string().len(), "{details}");
}
