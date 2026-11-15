use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use std::path::{Path, PathBuf};

const CODE_SESSIONS: &str = "claude-code-sessions";
const ECHOBIRD_PROFILE: &str = "7d8f4e2a-9c3b-4f1a-b0e5-1a2b3c4d5e6f";
const GATEWAY_ORG: &str = "00000000-0000-4000-8000-000000000001";

pub(super) struct Layout {
    pub(super) roots: Vec<PathBuf>,
    pub(super) targets: Vec<PathBuf>,
    pub(super) threep_root: Option<PathBuf>,
}

pub(super) fn resolve() -> Result<Layout, String> {
    let override_root = std::env::var_os("CLAUDE_USER_DATA_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let official = match &override_root {
        Some(root) => root.clone(),
        None => official_root()?,
    };
    #[cfg(windows)]
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let local = None;
    let (roots, threep) = user_data_roots(official, local, override_root.clone())?;
    #[cfg(windows)]
    let roots = {
        let mut roots = roots;
        if override_root.is_none() {
            if let Some(roaming) = dirs::config_dir() {
                for name in ["Claude", "Claude-3p"] {
                    let root = roaming.join(name);
                    if !roots.contains(&root) {
                        roots.push(root);
                    }
                }
            }
        }
        roots
    };
    resolve_at(roots, threep)
}

/// The same data folder account snapshots use.
fn official_root() -> Result<PathBuf, String> {
    crate::services::claude_desktop_accounts::data_dir()
        .map_err(|_| "Cannot locate the Claude Desktop data directory.".into())
}

fn user_data_roots(
    official: PathBuf,
    local: Option<PathBuf>,
    override_root: Option<PathBuf>,
) -> Result<(Vec<PathBuf>, PathBuf), String> {
    if let Some(root) = override_root {
        if !root.is_absolute() {
            return Err("CLAUDE_USER_DATA_DIR must be an absolute directory.".into());
        }
        return Ok((vec![root.clone()], root));
    }
    let legacy = official.with_file_name("Claude-3p");
    let threep = local
        .map(|root| root.join("Claude-3p"))
        .unwrap_or_else(|| legacy.clone());
    let mut roots = vec![official, threep.clone()];
    if !roots.contains(&legacy) {
        roots.push(legacy);
    }
    Ok((roots, threep))
}

fn resolve_at(roots: Vec<PathBuf>, threep: PathBuf) -> Result<Layout, String> {
    let mut targets = Vec::new();
    for root in &roots {
        let sessions = root.join(CODE_SESSIONS);
        for account in uuid_directories(&sessions)? {
            for organization in uuid_directories(&account)? {
                if !targets.contains(&organization) {
                    targets.push(organization);
                }
            }
        }
    }
    // Desktop's plain gateway identity is its installation UUID, not the
    // configLibrary profile ID. Other authentication modes can supply an org.
    let threep_root = if plain_gateway_profile(&threep)? {
        if let Some(account) = installation_id(&threep)? {
            let target = threep.join(CODE_SESSIONS).join(account).join(GATEWAY_ORG);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        Some(threep)
    } else {
        None
    };
    targets.sort();
    Ok(Layout {
        roots,
        targets,
        threep_root,
    })
}

fn uuid_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if is_link(&metadata) => return Ok(Vec::new()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Cannot inspect Claude Desktop Code scopes: {error}"
            ))
        }
    }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot read Claude Desktop Code scopes: {error}")),
    };
    let mut directories = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Cannot read Claude Desktop Code scope: {error}"))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_uuid(&name) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("Cannot inspect Claude Desktop Code scope: {error}"))?;
        if !metadata.is_dir() || is_link(&metadata) {
            continue;
        }
        directories.push(entry.path());
    }
    directories.sort();
    Ok(directories)
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::parse_str(value).is_ok()
}

fn optional_object(path: &Path) -> Result<Option<Value>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot read Claude Desktop Code layout: {error}")),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Claude Desktop Code layout contains invalid JSON.".to_string())?;
    if !value.is_object() {
        return Err("Claude Desktop Code layout must contain a JSON object.".into());
    }
    Ok(Some(value))
}

fn plain_gateway_profile(root: &Path) -> Result<bool, String> {
    let Some(config) = optional_object(&root.join("claude_desktop_config.json"))? else {
        return Ok(false);
    };
    if config["deploymentMode"] != "3p"
        || !config.as_object().unwrap().keys().all(|key| {
            matches!(
                key.as_str(),
                "deploymentMode" | "mcpServers" | "theme" | "locale"
            )
        })
    {
        return Ok(false);
    }
    let library = root.join("configLibrary");
    let Some(meta) = optional_object(&library.join("_meta.json"))? else {
        return Ok(false);
    };
    if meta["appliedId"] != ECHOBIRD_PROFILE {
        return Ok(false);
    }
    let Some(profile) = optional_object(&library.join(format!("{ECHOBIRD_PROFILE}.json")))? else {
        return Ok(false);
    };
    Ok(profile["inferenceProvider"] == "gateway"
        && profile.as_object().unwrap().keys().all(|key| {
            matches!(
                key.as_str(),
                "disableDeploymentModeChooser"
                    | "inferenceGatewayApiKey"
                    | "inferenceGatewayAuthScheme"
                    | "inferenceGatewayBaseUrl"
                    | "inferenceModels"
                    | "inferenceProvider"
                    | "coworkEgressAllowedHosts"
                    | "disabledBuiltinTools"
            )
        }))
}

fn installation_id(root: &Path) -> Result<Option<String>, String> {
    let encoded = match std::fs::read_to_string(root.join("ant-did")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Cannot read Claude Desktop installation identity: {error}"
            ))
        }
    };
    let value = STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| is_uuid(value))
        .ok_or_else(|| "Claude Desktop installation identity is invalid.".to_string())?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ACCOUNT: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";
    const ORG: &str = "33333333-3333-4333-8333-333333333333";

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("claude-layout-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn profile(&self) -> PathBuf {
            let root = self.0.join("Claude-3p");
            let library = root.join("configLibrary");
            std::fs::create_dir_all(&library).unwrap();
            std::fs::write(
                root.join("claude_desktop_config.json"),
                r#"{"deploymentMode":"3p"}"#,
            )
            .unwrap();
            std::fs::write(
                library.join("_meta.json"),
                format!(r#"{{"appliedId":"{ECHOBIRD_PROFILE}"}}"#),
            )
            .unwrap();
            std::fs::write(
                library.join(format!("{ECHOBIRD_PROFILE}.json")),
                r#"{"inferenceProvider":"gateway","inferenceModels":[]}"#,
            )
            .unwrap();
            root
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn platform_roots_include_windows_legacy_and_override_is_exclusive() {
        let fixture = Fixture::new();
        let official = fixture.0.join("Roaming/Claude");
        let local = fixture.0.join("Local");
        let (roots, threep) = user_data_roots(official.clone(), Some(local.clone()), None).unwrap();
        assert_eq!(threep, local.join("Claude-3p"));
        assert_eq!(
            roots,
            [
                official.clone(),
                threep.clone(),
                fixture.0.join("Roaming/Claude-3p")
            ]
        );
        let (roots, _) = user_data_roots(official.clone(), None, None).unwrap();
        assert_eq!(roots.len(), 2);
        let custom = fixture.0.join("custom");
        let (roots, threep) = user_data_roots(official, Some(local), Some(custom.clone())).unwrap();
        assert_eq!(roots, std::slice::from_ref(&custom));
        assert_eq!(threep, custom);
        assert!(user_data_roots(fixture.0.clone(), None, Some(PathBuf::from("relative"))).is_err());
    }

    #[test]
    fn discovers_all_existing_uuid_scopes_without_creating_anything() {
        let fixture = Fixture::new();
        let root = fixture.0.join("Claude");
        let one = root.join(CODE_SESSIONS).join(ACCOUNT).join(ORG);
        let two = root.join(CODE_SESSIONS).join(OTHER).join(ORG);
        for scope in [
            &one,
            &two,
            &root.join(CODE_SESSIONS).join("not-an-account").join(ORG),
        ] {
            std::fs::create_dir_all(scope).unwrap();
        }
        std::fs::write(
            root.join(CODE_SESSIONS).join(ACCOUNT).join(OTHER),
            b"not a directory",
        )
        .unwrap();
        let missing = fixture.0.join("missing");
        let layout = resolve_at(vec![root, missing.clone()], missing.clone()).unwrap();
        assert_eq!(layout.targets, [one, two]);
        assert_eq!(layout.roots.len(), 2);
        assert!(!missing.exists());
        assert!(layout.threep_root.is_none());
    }

    #[test]
    fn derives_gateway_scope_from_installation_identity_before_scope_exists() {
        let fixture = Fixture::new();
        let root = fixture.profile();
        std::fs::write(root.join("ant-did"), STANDARD.encode(ACCOUNT)).unwrap();
        let expected = root.join(CODE_SESSIONS).join(ACCOUNT).join(GATEWAY_ORG);
        let layout = resolve_at(vec![root.clone()], root.clone()).unwrap();
        assert_eq!(layout.targets, std::slice::from_ref(&expected));
        assert_eq!(layout.threep_root, Some(root));
        assert!(!expected.exists());
    }

    #[test]
    fn missing_installation_identity_is_reported_for_explicit_preparation() {
        let fixture = Fixture::new();
        let root = fixture.profile();
        let layout = resolve_at(vec![root.clone()], root.clone()).unwrap();
        assert!(layout.targets.is_empty());
        assert_eq!(layout.threep_root, Some(root.clone()));
        assert!(!root.join("ant-did").exists());
        std::fs::write(root.join("ant-did"), STANDARD.encode("not-a-uuid")).unwrap();
        assert!(resolve_at(vec![root.clone()], root).is_err());
    }

    #[test]
    fn unknown_authentication_profiles_keep_existing_scopes_without_guessing_org() {
        let fixture = Fixture::new();
        let root = fixture.profile();
        let existing = root.join(CODE_SESSIONS).join(ACCOUNT).join(ORG);
        std::fs::create_dir_all(&existing).unwrap();
        let profile_path = root
            .join("configLibrary")
            .join(format!("{ECHOBIRD_PROFILE}.json"));
        for key in ["bootstrap", "authentication", "telemetry", "unknown"] {
            let mut profile = serde_json::json!({"inferenceProvider":"gateway"});
            profile[key] = serde_json::json!({});
            std::fs::write(&profile_path, profile.to_string()).unwrap();
            let layout = resolve_at(vec![root.clone()], root.clone()).unwrap();
            assert_eq!(layout.targets, std::slice::from_ref(&existing));
            assert!(layout.threep_root.is_none());
        }
        std::fs::write(&profile_path, r#"{"inferenceProvider":"gateway"}"#).unwrap();
        std::fs::write(
            root.join("claude_desktop_config.json"),
            r#"{"deploymentMode":"3p","bootstrap":{}}"#,
        )
        .unwrap();
        assert!(resolve_at(vec![root.clone()], root)
            .unwrap()
            .threep_root
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_account_or_organization_symlinks() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let root = fixture.0.join("Claude");
        let sessions = root.join(CODE_SESSIONS);
        let elsewhere = fixture.0.join("elsewhere");
        std::fs::create_dir_all(elsewhere.join(ORG)).unwrap();
        std::fs::create_dir_all(sessions.join(OTHER)).unwrap();
        symlink(&elsewhere, sessions.join(ACCOUNT)).unwrap();
        symlink(elsewhere.join(ORG), sessions.join(OTHER).join(ORG)).unwrap();
        let layout = resolve_at(vec![root.clone()], root).unwrap();
        assert!(layout.targets.is_empty());
    }
}
