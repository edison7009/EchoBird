use super::SystemApplication;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum Platform {
    Windows,
    Macos,
    Linux,
}

struct Roots {
    platform: Platform,
    home: PathBuf,
    config: Option<PathBuf>,
    data: Option<PathBuf>,
    program_data: Option<PathBuf>,
}

fn xdg(value: Option<std::ffi::OsString>, home: &Path, fallback: &str) -> PathBuf {
    value
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value
            .chars()
            .any(|ch| matches!(ch, '/' | '\\' | ':' | '\0') || ch.is_control())
}

fn names(app: &SystemApplication) -> Vec<String> {
    let mut result = vec![app.name.clone()];
    if let Some(stem) = app
        .executable
        .as_deref()
        .and_then(|path| Path::new(path).file_stem())
        .and_then(|name| name.to_str())
    {
        if ![
            "env",
            "sh",
            "bash",
            "node",
            "python",
            "python3",
            "electron",
            "flatpak",
            "snap",
            "open",
            "cmd",
            "powershell",
        ]
        .contains(&stem.to_ascii_lowercase().as_str())
        {
            result.push(stem.to_string());
        }
    }
    result.retain(|name| component(name));
    result
}

fn candidates(app: &SystemApplication, roots: &Roots, bundle_id: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let names = names(app);
    match roots.platform {
        Platform::Macos => {
            let library = roots.home.join("Library");
            for name in &names {
                paths.push(library.join("Application Support").join(name));
            }
            if let Some(id) = bundle_id.filter(|id| component(id)) {
                paths.extend([
                    library.join("Preferences").join(format!("{id}.plist")),
                    library.join("Application Support").join(id),
                    library.join("Containers").join(id),
                    library.join("Caches").join(id),
                    library
                        .join("Saved Application State")
                        .join(format!("{id}.savedState")),
                ]);
            }
        }
        Platform::Windows => {
            for root in [&roots.config, &roots.data, &roots.program_data]
                .into_iter()
                .flatten()
            {
                for name in &names {
                    paths.push(root.join(name));
                }
            }
            if let Some((family, _)) = app
                .path
                .strip_prefix("shell:AppsFolder\\")
                .and_then(|target| target.split_once('!'))
            {
                if component(family) {
                    if let Some(local) = &roots.data {
                        for state in ["LocalState", "RoamingState", "Settings"] {
                            paths.push(local.join("Packages").join(family).join(state));
                        }
                    }
                }
            }
        }
        Platform::Linux => {
            let mut ids = names;
            if let Some(id) = Path::new(&app.path)
                .file_stem()
                .and_then(|name| name.to_str())
                .filter(|id| component(id))
            {
                ids.push(id.into());
            }
            let original = ids.clone();
            ids.extend(original.into_iter().map(|name| name.to_lowercase()));
            for id in ids {
                for root in [&roots.config, &roots.data].into_iter().flatten() {
                    paths.push(root.join(&id));
                }
                if id.contains('.') {
                    paths.push(roots.home.join(".var/app").join(&id));
                }
            }
        }
    }
    paths
}

fn existing(paths: Vec<PathBuf>, roots: &Roots) -> Vec<String> {
    let mut seen = HashSet::new();
    let broad_roots: HashSet<PathBuf> = std::iter::once(roots.home.clone())
        .chain(
            [&roots.config, &roots.data, &roots.program_data]
                .into_iter()
                .flatten()
                .cloned(),
        )
        .chain(
            [
                "Library",
                "Library/Preferences",
                "Library/Containers",
                "Library/Caches",
                ".var/app",
            ]
            .into_iter()
            .map(|path| roots.home.join(path)),
        )
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .collect();
    paths
        .into_iter()
        .filter_map(|path| {
            if !path.is_absolute()
                || path.parent().is_none()
                || path == roots.home
                || [&roots.config, &roots.data, &roots.program_data]
                    .into_iter()
                    .flatten()
                    .any(|root| path == *root)
            {
                return None;
            }
            let info = std::fs::metadata(&path).ok()?;
            if !info.is_file() && !info.is_dir() {
                return None;
            }
            let resolved = std::fs::canonicalize(&path).ok()?;
            if broad_roots.contains(&resolved) || resolved.parent().is_none() {
                return None;
            }
            let mut identity = resolved.to_string_lossy().into_owned();
            if matches!(roots.platform, Platform::Windows) {
                identity.make_ascii_lowercase();
            }
            seen.insert(identity)
                .then(|| super::super::normalize_for_display(path.to_string_lossy().into_owned()))
        })
        .collect()
}

fn catalog_paths(
    app: &SystemApplication,
    definitions: &[crate::models::tool::ToolDefinition],
) -> Vec<PathBuf> {
    let Some(executable) = app
        .executable
        .as_deref()
        .and_then(|value| std::fs::canonicalize(value).ok())
    else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for definition in definitions {
        let config = &definition.paths_config;
        let matches = super::super::get_platform_paths(&config.paths)
            .into_iter()
            .any(|path| {
                std::fs::canonicalize(super::super::expand_path(&path))
                    .is_ok_and(|path| path == executable)
            });
        if matches {
            paths.extend(
                [
                    &config.config_file,
                    &config.config_dir,
                    &definition.config_mapping.config_file,
                ]
                .into_iter()
                .filter(|value| !value.is_empty())
                .map(|value| super::super::expand_path(value)),
            );
        }
    }
    paths
}

pub(super) fn discover(app: &SystemApplication) -> Vec<String> {
    discover_with_paths(app, Vec::new())
}

pub(super) fn discover_managed(
    app: &SystemApplication,
    definition: &crate::models::tool::ToolDefinition,
) -> Vec<String> {
    let mut extra: Vec<PathBuf> = [
        &definition.paths_config.config_file,
        &definition.paths_config.config_dir,
        &definition.config_mapping.config_file,
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .map(|value| super::super::expand_path(value))
    .collect();
    if let Ok(path) = super::super::model_config_path(definition) {
        extra.push(path);
    }
    discover_with_paths(app, extra)
}

fn discover_with_paths(app: &SystemApplication, extra: Vec<PathBuf>) -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let platform = if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let roots = Roots {
        platform,
        config: if matches!(platform, Platform::Linux) {
            Some(xdg(std::env::var_os("XDG_CONFIG_HOME"), &home, ".config"))
        } else {
            dirs::config_dir()
        },
        data: if matches!(platform, Platform::Linux) {
            Some(xdg(
                std::env::var_os("XDG_DATA_HOME"),
                &home,
                ".local/share",
            ))
        } else {
            dirs::data_local_dir()
        },
        program_data: std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute()),
        home,
    };
    let bundle_id = if matches!(platform, Platform::Macos) {
        plist::Value::from_file(Path::new(&app.path).join("Contents/Info.plist"))
            .ok()
            .and_then(|value| {
                value
                    .as_dictionary()?
                    .get("CFBundleIdentifier")?
                    .as_string()
                    .map(str::to_owned)
            })
    } else {
        None
    };
    let mut paths = catalog_paths(app, &super::super::get_definitions());
    paths.extend(candidates(app, &roots, bundle_id.as_deref()));
    paths.extend(extra);
    existing(paths, &roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(platform: Platform) -> (PathBuf, Roots, SystemApplication) {
        let root = std::env::temp_dir().join(format!("app-config-paths-{}", uuid::Uuid::new_v4()));
        let roots = Roots {
            platform,
            home: root.join("home"),
            config: Some(root.join("config")),
            data: Some(root.join("data")),
            program_data: Some(root.join("program-data")),
        };
        std::fs::create_dir_all(&roots.home).unwrap();
        let app = SystemApplication::new(
            "Editor".into(),
            "org.example.Editor.desktop".into(),
            Some("/opt/editor/editor".into()),
            String::new(),
        );
        (root, roots, app)
    }
    #[test]
    fn macos_finds_existing_sandbox_and_preferences_without_making_up_support_paths() {
        let (root, roots, app) = fixture(Platform::Macos);
        let container = roots.home.join("Library/Containers/org.example.editor");
        let preference = roots
            .home
            .join("Library/Preferences/org.example.editor.plist");
        std::fs::create_dir_all(&container).unwrap();
        std::fs::create_dir_all(preference.parent().unwrap()).unwrap();
        std::fs::write(&preference, b"private contents not read").unwrap();
        let result = existing(candidates(&app, &roots, Some("org.example.editor")), &roots);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|path| Path::new(path) == container));
        assert!(result.iter().any(|path| Path::new(path) == preference));
        assert_eq!(
            std::fs::read(preference).unwrap(),
            b"private contents not read"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn windows_finds_appdata_and_only_the_selected_package_state() {
        let (root, roots, mut app) = fixture(Platform::Windows);
        app.path = "shell:AppsFolder\\Editor_family!App".into();
        let roaming = roots.config.as_ref().unwrap().join("Editor");
        let state = roots
            .data
            .as_ref()
            .unwrap()
            .join("Packages/Editor_family/LocalState");
        std::fs::create_dir_all(&roaming).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(
            roots
                .data
                .as_ref()
                .unwrap()
                .join("Packages/Other_family/LocalState"),
        )
        .unwrap();
        let result = existing(candidates(&app, &roots, None), &roots);
        assert_eq!(result.len(), 2);
        assert!(!result.iter().any(|path| path.contains("Other_family")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn linux_uses_xdg_and_exact_desktop_ids_not_shared_launchers() {
        let (root, roots, mut app) = fixture(Platform::Linux);
        app.executable = Some("/usr/bin/flatpak".into());
        let selected = roots.config.as_ref().unwrap().join("org.example.Editor");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(roots.config.as_ref().unwrap().join("flatpak")).unwrap();
        let result = existing(candidates(&app, &roots, None), &roots);
        assert_eq!(result, vec![selected.to_string_lossy()]);
        assert_eq!(
            xdg(Some("relative".into()), &roots.home, ".config"),
            roots.home.join(".config")
        );
        assert_eq!(
            xdg(Some("".into()), &roots.home, ".config"),
            roots.home.join(".config")
        );
        assert_eq!(
            xdg(
                Some(selected.clone().into_os_string()),
                &roots.home,
                ".config"
            ),
            selected
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn catalog_paths_require_the_selected_executable_and_only_publish_existing_locations() {
        use crate::models::tool::{PathsConfig, ToolDefinition};
        let (root, roots, mut app) = fixture(Platform::Linux);
        let executable = root.join("app");
        let config = root.join("private.json");
        std::fs::write(&executable, b"not executed").unwrap();
        std::fs::write(&config, b"not read").unwrap();
        app.executable = Some(executable.to_string_lossy().into_owned());
        let definition = ToolDefinition {
            id: "fixture".into(),
            tool_dir: String::new(),
            paths_config: serde_json::from_value::<PathsConfig>(serde_json::json!({
                "name":"Fixture", "category":"Desktop", "configFile":config,
                "paths":{"darwin":[executable],"win32":[executable],"linux":[executable]}
            }))
            .unwrap(),
            config_mapping: serde_json::from_str("{}").unwrap(),
        };
        let result = existing(
            catalog_paths(&app, std::slice::from_ref(&definition)),
            &roots,
        );
        assert_eq!(result, vec![config.to_string_lossy()]);
        app.executable = Some(root.join("not-the-same-app").to_string_lossy().into_owned());
        assert!(catalog_paths(&app, &[definition]).is_empty());
        assert_eq!(std::fs::read(config).unwrap(), b"not read");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_catalog_paths_include_existing_config_without_declared_executable_matching() {
        use crate::models::tool::{PathsConfig, ToolDefinition};
        let (root, _, mut app) = fixture(Platform::Linux);
        let private = root.join("private");
        let config = private.join("settings.json");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(&config, b"not modified").unwrap();
        app.name = "fixture-unique-managed-app".into();
        app.executable = None;
        let definition = ToolDefinition {
            id: "fixture-managed".into(),
            tool_dir: String::new(),
            paths_config: serde_json::from_value::<PathsConfig>(serde_json::json!({
                "name":"Fixture", "category":"Desktop", "configFile":config,
                "configDir":private, "paths":{}
            }))
            .unwrap(),
            config_mapping: serde_json::from_value(serde_json::json!({"configFile":config}))
                .unwrap(),
        };
        let result = discover_managed(&app, &definition);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|path| Path::new(path) == config));
        assert!(result.iter().any(|path| Path::new(path) == private));
        assert_eq!(std::fs::read(&config).unwrap(), b"not modified");
        std::fs::remove_dir_all(root).unwrap();
        assert!(discover_managed(&app, &definition).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn deduplicates_real_locations_and_excludes_links_to_shared_roots() {
        let (root, roots, _) = fixture(Platform::Macos);
        let config = roots.config.as_ref().unwrap();
        let private = config.join("App");
        let alias = roots.home.join("alias");
        let broad = roots.home.join("broad");
        std::fs::create_dir_all(&private).unwrap();
        std::os::unix::fs::symlink(&private, &alias).unwrap();
        std::os::unix::fs::symlink(config, &broad).unwrap();
        assert_eq!(existing(vec![private, alias, broad], &roots).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_traversal_missing_paths_and_broad_user_roots() {
        let (root, roots, mut app) = fixture(Platform::Macos);
        app.name = "../outside".into();
        app.executable = None;
        let mut paths = candidates(&app, &roots, Some("../../outside"));
        assert!(paths.is_empty());
        paths.extend([roots.home.clone(), root.join("missing"), roots.home.clone()]);
        assert!(existing(paths, &roots).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
