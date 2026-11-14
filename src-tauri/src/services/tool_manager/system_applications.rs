use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex as SnapshotMutex};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemApplication {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: Option<u64>,
    #[serde(skip)]
    pub executable: Option<String>,
    #[serde(skip)]
    pub icon_target: String,
}

impl SystemApplication {
    fn new(name: String, path: String, executable: Option<String>, icon_target: String) -> Self {
        let identity = if cfg!(windows) {
            path.to_lowercase()
        } else {
            path.clone()
        };
        Self {
            id: format!("system:{:x}", Sha256::digest(identity.as_bytes())),
            name,
            path,
            created_at: None,
            executable,
            icon_target,
        }
    }
}

fn creation_time(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .created()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

static APPLICATIONS: SnapshotMutex<Option<Arc<Vec<SystemApplication>>>> = SnapshotMutex::new(None);
static SCAN: Mutex<()> = Mutex::const_new(());
static ICONS: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn executable(pc: &crate::models::tool::PathsConfig) -> Option<String> {
    let applications = APPLICATIONS.lock().ok()?.clone()?;
    let paths = super::get_platform_paths(&pc.paths);
    applications
        .iter()
        .filter(|app| {
            app.name.eq_ignore_ascii_case(&pc.name)
                || pc.install_hints.as_ref().is_some_and(|hints| {
                    hints
                        .windows_display_names
                        .iter()
                        .chain(hints.linux_desktop_names.iter())
                        .chain(hints.macos_app_name.iter())
                        .any(|name| app.name.eq_ignore_ascii_case(name))
                })
        })
        .filter_map(|app| app.executable.as_deref())
        .find_map(|executable| {
            let executable = std::path::Path::new(executable);
            let leaf = executable.file_name()?;
            let declared = paths.iter().any(|path| {
                std::path::Path::new(path)
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(leaf))
            }) || (!pc.command.is_empty()
                && leaf == std::ffi::OsStr::new(&pc.command));
            if !declared {
                return None;
            }
            let path = if executable.is_absolute() {
                executable.to_path_buf()
            } else {
                which::which(executable).ok()?
            };
            super::valid_executable(pc, &path).then(|| path.to_string_lossy().into_owned())
        })
}

pub async fn list(force_refresh: bool) -> Result<Arc<Vec<SystemApplication>>, String> {
    if !force_refresh {
        if let Some(applications) = APPLICATIONS.lock().unwrap().clone() {
            return Ok(applications);
        }
    }
    // Keep the previous snapshot readable while an explicit refresh scans the OS.
    let _scan = SCAN.lock().await;
    if !force_refresh {
        if let Some(applications) = APPLICATIONS.lock().unwrap().clone() {
            return Ok(applications);
        }
    }
    let started = std::time::Instant::now();
    let applications = tokio::task::spawn_blocking(enumerate)
        .await
        .map_err(|error| error.to_string())??;
    let applications = Arc::new(normalize(applications));
    log::info!(
        "[Applications] {} applications in {:?}",
        applications.len(),
        started.elapsed()
    );
    *APPLICATIONS.lock().unwrap() = Some(applications.clone());
    ICONS.lock().await.clear();
    Ok(applications)
}

fn normalize(mut applications: Vec<SystemApplication>) -> Vec<SystemApplication> {
    applications.retain(|app| !app.name.trim().is_empty() && !app.path.is_empty());
    applications.sort_by(|a, b| a.id.cmp(&b.id));
    applications.dedup_by(|a, b| a.id == b.id);
    applications.sort_by_cached_key(|app| app.name.to_lowercase());
    applications
}

pub async fn find(id: &str) -> Result<SystemApplication, String> {
    list(false)
        .await?
        .iter()
        .find(|app| app.id == id)
        .cloned()
        .ok_or_else(|| "Application is no longer available. Refresh the application list.".into())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDetails {
    path: Option<String>,
    directory: Option<String>,
    version: Option<String>,
    file_size: Option<u64>,
    modified_at: Option<u64>,
    launch_target: String,
}

pub async fn details(id: &str) -> Result<ApplicationDetails, String> {
    let application = find(id).await?;
    tokio::task::spawn_blocking(move || inspect(&application))
        .await
        .map_err(|error| error.to_string())
}

fn inspect(application: &SystemApplication) -> ApplicationDetails {
    use std::path::PathBuf;
    #[cfg(not(target_os = "linux"))]
    let executable = application.executable.as_deref().and_then(|path| {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            Some(path)
        } else {
            which::which(path).ok()
        }
    });
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let mut location = Some(PathBuf::from(&application.path));
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let mut location = executable.clone();
    #[cfg(windows)]
    let version = {
        let mut version = None;
        if let Some(path) = executable.as_deref() {
            version = windows_file_version(path);
        } else if let Some((path, package_version)) =
            crate::services::msix::application_location(&application.path)
        {
            location = Some(path);
            version = Some(package_version);
        }
        version
    };
    #[cfg(target_os = "macos")]
    let version = {
        plist::Value::from_file(std::path::Path::new(&application.path).join("Contents/Info.plist"))
            .ok()
            .and_then(|info| {
                info.as_dictionary()?
                    .get("CFBundleShortVersionString")?
                    .as_string()
                    .map(str::to_owned)
            })
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let version = None;
    // A desktop entry remains useful when its executable cannot be resolved.
    if location.is_none() && std::path::Path::new(&application.path).is_absolute() {
        location = Some(PathBuf::from(&application.path));
    }
    // Linux entries can launch through shared programs such as env or flatpak.
    // Describe the registered entry so its path never identifies the shared launcher as this app.
    #[cfg(target_os = "linux")]
    let metadata_path = location.as_deref();
    #[cfg(not(target_os = "linux"))]
    let metadata_path = executable.as_deref().or(location.as_deref());
    let metadata = metadata_path.and_then(|path| std::fs::metadata(path).ok());
    let directory = location.as_deref().and_then(|path| {
        if path.is_dir() && path.extension().is_none_or(|ext| ext != "app") {
            Some(path)
        } else {
            path.parent().filter(|parent| parent.is_dir())
        }
    });
    ApplicationDetails {
        path: location
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        directory: directory.map(|path| path.to_string_lossy().into_owned()),
        version,
        file_size: metadata
            .as_ref()
            .filter(|info| info.is_file())
            .map(|info| info.len()),
        modified_at: metadata
            .and_then(|info| info.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64),
        launch_target: application.path.clone(),
    }
}

#[cfg(windows)]
fn windows_file_version(path: &std::path::Path) -> Option<String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    unsafe {
        let path = HSTRING::from(path.as_os_str());
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut buffer = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, buffer.as_mut_ptr().cast()).ok()?;
        let mut value = std::ptr::null_mut();
        let mut length = 0;
        if !VerQueryValueW(buffer.as_ptr().cast(), w!("\\"), &mut value, &mut length).as_bool()
            || length < std::mem::size_of::<VS_FIXEDFILEINFO>() as u32
            || value.is_null()
        {
            return None;
        }
        let info = std::ptr::read_unaligned(value.cast::<VS_FIXEDFILEINFO>());
        Some(format!(
            "{}.{}.{}.{}",
            info.dwFileVersionMS >> 16,
            info.dwFileVersionMS & 0xffff,
            info.dwFileVersionLS >> 16,
            info.dwFileVersionLS & 0xffff
        ))
    }
}

pub async fn icon(id: &str) -> Result<Option<String>, String> {
    let app = find(id).await?;
    // Serialize native image work and reuse both successful and missing icons.
    let mut cache = ICONS.lock().await;
    if let Some(icon) = cache.get(id) {
        return Ok(icon.clone());
    }
    let icon = tokio::task::spawn_blocking(move || {
        crate::services::voice_input::application_icon(&app.icon_target)
    })
    .await
    .map_err(|error| error.to_string())?;
    cache.insert(id.to_string(), icon.clone());
    Ok(icon)
}

#[cfg(windows)]
fn enumerate() -> Result<Vec<SystemApplication>, String> {
    use windows::core::{w, Interface, PWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2,
        SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    unsafe fn string(value: windows::core::Result<PWSTR>) -> Option<String> {
        let value = value.ok()?;
        let text = value.to_string().ok();
        CoTaskMemFree(Some(value.0.cast()));
        text
    }
    let result = || -> windows::core::Result<Vec<SystemApplication>> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let _apartment = Apartment;
            let folder: IShellItem =
                SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
            let entries: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
            let mut target_key = PROPERTYKEY::default();
            PSGetPropertyKeyFromName(w!("System.Link.TargetParsingPath"), &mut target_key)?;
            let mut applications = Vec::new();
            loop {
                let mut item = [None];
                let mut fetched = 0;
                entries.Next(&mut item, Some(&mut fetched))?;
                let Some(item) = item[0].take().filter(|_| fetched != 0) else {
                    break;
                };
                let Some(name) = string(item.GetDisplayName(SIGDN_NORMALDISPLAY)) else {
                    continue;
                };
                let Some(identity) = string(item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING))
                else {
                    continue;
                };
                let path = format!("shell:AppsFolder\\{identity}");
                let target = item
                    .cast::<IShellItem2>()
                    .ok()
                    .and_then(|item| string(item.GetString(&target_key)));
                if target
                    .as_deref()
                    .is_some_and(|path| !windows_application_target(path))
                {
                    continue;
                }
                let executable = target
                    .as_deref()
                    .filter(|path| !path.is_empty())
                    .map(|path| super::expand_path(path).to_string_lossy().into_owned());
                let created_at = executable
                    .as_deref()
                    .and_then(|path| creation_time(std::path::Path::new(path)));
                if executable.as_deref().is_some_and(|path| {
                    super::is_uninstaller_stem(
                        &std::path::Path::new(path)
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase(),
                    )
                }) {
                    continue;
                }
                // Shell parsing names preserve shortcut arguments and Store activation identities.
                let mut application = SystemApplication::new(name, path.clone(), executable, path);
                application.created_at = created_at;
                applications.push(application);
            }
            let families = applications
                .iter()
                .filter(|app| app.created_at.is_none())
                .filter_map(|app| app.path.strip_prefix("shell:AppsFolder\\")?.split_once('!'))
                .map(|(family, _)| family.to_string())
                .collect();
            if let Some(locations) = crate::services::msix::application_locations(&families) {
                for app in applications
                    .iter_mut()
                    .filter(|app| app.created_at.is_none())
                {
                    let family = app
                        .path
                        .strip_prefix("shell:AppsFolder\\")
                        .and_then(|value| value.split_once('!'))
                        .map(|(family, _)| family);
                    app.created_at = family
                        .and_then(|family| locations.get(family))
                        .and_then(|path| creation_time(path));
                }
            }
            Ok(applications)
        }
    };
    result().map_err(|error| format!("Cannot enumerate Windows applications: {error}"))
}

#[cfg(any(windows, test))]
fn windows_application_target(path: &str) -> bool {
    path.is_empty()
        || std::path::Path::new(path).extension().is_some_and(|ext| {
            matches!(
                ext.to_string_lossy().to_ascii_lowercase().as_str(),
                "exe" | "com" | "msc" | "cpl" | "cmd" | "bat" | "appref-ms"
            )
        })
}

#[cfg(target_os = "macos")]
fn enumerate() -> Result<Vec<SystemApplication>, String> {
    use std::path::{Path, PathBuf};
    fn collect(root: &Path, depth: usize, paths: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "app") {
                paths.push(path);
            } else if depth > 0 && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                collect(&path, depth - 1, paths);
            }
        }
    }
    let mut paths = Vec::new();
    collect(Path::new("/Applications"), 3, &mut paths);
    collect(Path::new("/System/Applications"), 3, &mut paths);
    if let Some(home) = dirs::home_dir() {
        collect(&home.join("Applications"), 3, &mut paths);
    }
    if let Ok(output) = crate::utils::process::command("mdfind")
        .arg("kMDItemContentType == 'com.apple.application-bundle'")
        .output()
    {
        if output.status.success() {
            paths.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(PathBuf::from),
            );
        }
    }
    Ok(paths
        .into_iter()
        .filter(|path| path.is_dir())
        .filter_map(|path| macos_application(&path))
        .collect())
}

#[cfg(any(target_os = "macos", test))]
fn macos_application(path: &std::path::Path) -> Option<SystemApplication> {
    let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok()?;
    let info = info.as_dictionary()?;
    let binary = info.get("CFBundleExecutable")?.as_string()?;
    let executable = path.join("Contents/MacOS").join(binary);
    if !executable.is_file() {
        return None;
    }
    let name = info
        .get("CFBundleDisplayName")
        .or_else(|| info.get("CFBundleName"))
        .and_then(plist::Value::as_string)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let created_at = creation_time(path);
    let path = path.to_string_lossy().into_owned();
    let mut application = SystemApplication::new(
        name,
        path.clone(),
        Some(executable.to_string_lossy().into_owned()),
        path,
    );
    application.created_at = created_at;
    Some(application)
}

#[cfg(target_os = "linux")]
fn enumerate() -> Result<Vec<SystemApplication>, String> {
    use gio::prelude::*;
    Ok(gio::AppInfo::all()
        .into_iter()
        .filter(|app| app.should_show())
        .filter_map(|app| {
            let desktop = app.downcast::<gio::DesktopAppInfo>().ok()?;
            let path = desktop.filename()?.to_string_lossy().into_owned();
            let icon = desktop.id()?.to_string();
            let created_at = creation_time(std::path::Path::new(&path));
            let mut application = SystemApplication::new(
                desktop.display_name().to_string(),
                path,
                Some(desktop.executable().to_string_lossy().into_owned()),
                icon,
            );
            application.created_at = created_at;
            Some(application)
        })
        .collect())
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn enumerate() -> Result<Vec<SystemApplication>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_uses_launch_target_and_keeps_distinct_installations() {
        let first = SystemApplication::new("App".into(), "one/app".into(), None, "".into());
        let renamed = SystemApplication::new("Localized".into(), "one/app".into(), None, "".into());
        let second = SystemApplication::new("App".into(), "two/app".into(), None, "".into());
        assert_eq!(first.id, renamed.id);
        assert_ne!(first.id, second.id);
        assert_eq!(normalize(vec![first.clone(), renamed, second]).len(), 2);
    }

    #[test]
    fn windows_keeps_application_launchers_and_excludes_document_links() {
        for path in [
            "App.EXE",
            "services.msc",
            "settings.cpl",
            "start.cmd",
            "app.appref-ms",
            "",
        ] {
            assert!(windows_application_target(path), "{path}");
        }
        for path in ["Manual.html", "Website.url", "Readme.txt"] {
            assert!(!windows_application_target(path), "{path}");
        }
    }

    #[test]
    fn macos_bundle_uses_its_declared_executable_and_display_name() {
        let root = std::env::temp_dir().join(format!("echobird-apps-{}", uuid::Uuid::new_v4()));
        let bundle = root.join("Custom.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let mut info = plist::Dictionary::new();
        info.insert("CFBundleExecutable".into(), "Actual Binary".into());
        info.insert("CFBundleDisplayName".into(), "本机应用".into());
        plist::Value::Dictionary(info)
            .to_file_binary(bundle.join("Contents/Info.plist"))
            .unwrap();
        assert!(macos_application(&bundle).is_none());
        let executable = bundle.join("Contents/MacOS/Actual Binary");
        std::fs::write(&executable, "fixture").unwrap();
        let app = macos_application(&bundle).unwrap();
        assert_eq!(app.name, "本机应用");
        assert_eq!(app.created_at, creation_time(&bundle));
        assert_eq!(app.path, bundle.to_string_lossy());
        assert_eq!(
            app.executable.as_deref().map(std::path::Path::new),
            Some(executable.as_path())
        );
        std::fs::write(bundle.join("Contents/Info.plist"), "invalid").unwrap();
        assert!(macos_application(&bundle).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn details_read_file_metadata_without_launching_and_handle_missing_files() {
        let root = std::env::temp_dir().join(format!("echobird-apps-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("fixture");
        std::fs::write(&file, "data").unwrap();
        let expected_created = std::fs::metadata(&file)
            .unwrap()
            .created()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);
        assert_eq!(creation_time(&file), expected_created);
        let target = file.to_string_lossy().into_owned();
        let app = SystemApplication::new(
            "Fixture".into(),
            target.clone(),
            Some(target.clone()),
            "".into(),
        );
        let details = inspect(&app);
        assert_eq!(details.file_size, Some(4));
        assert!(details.modified_at.is_some());
        assert_eq!(
            details.directory.as_deref().map(std::path::Path::new),
            Some(root.as_path())
        );
        assert_eq!(details.launch_target, target);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(creation_time(&file), None);
        let missing = inspect(&app);
        assert_eq!(missing.file_size, None);
        assert_eq!(missing.modified_at, None);
        std::fs::remove_dir(&root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_details_identify_the_desktop_entry_instead_of_a_shared_launcher() {
        let root = std::env::temp_dir().join(format!("echobird-apps-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("fixture.desktop");
        let entry =
            "[Desktop Entry]\nType=Application\nName=Local app\nExec=env FLAG=1 /opt/app/start\n";
        std::fs::write(&file, entry).unwrap();
        let target = file.to_string_lossy().into_owned();
        let app = SystemApplication::new(
            "Fixture".into(),
            target.clone(),
            Some("env".into()),
            "".into(),
        );
        let details = inspect(&app);
        assert_eq!(details.path.as_deref(), Some(target.as_str()));
        assert_eq!(details.launch_target, target);
        assert_eq!(details.file_size, Some(entry.len() as u64));
        assert_eq!(
            details.directory.as_deref().map(std::path::Path::new),
            Some(root.as_path())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_uses_gio_for_quoted_desktop_entries_and_visibility() {
        use gio::prelude::*;
        let root = std::env::temp_dir().join(format!("echobird-apps-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("fixture.desktop");
        // GIO rejects entries whose Exec program is missing or non-executable.
        use std::os::unix::fs::PermissionsExt;
        let executable = root.join("Custom App/main");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let entry = format!("[Desktop Entry]\nType=Application\nName=Local app\nExec=\"{}\" --open %U\nIcon=fixture\n", executable.display());
        std::fs::write(&file, &entry).unwrap();
        let app = gio::DesktopAppInfo::from_filename(&file).unwrap();
        assert_eq!(app.executable(), executable);
        assert!(app.should_show());
        std::fs::write(&file, format!("{entry}NoDisplay=true\n")).unwrap();
        assert!(!gio::DesktopAppInfo::from_filename(&file)
            .unwrap()
            .should_show());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "reads this machine's registered applications"]
    fn live_windows_inventory() {
        let apps = enumerate().unwrap();
        assert!(!apps.is_empty());
        assert!(apps
            .iter()
            .all(|app| app.path.starts_with("shell:AppsFolder\\")));
        eprintln!("Found {} registered applications", apps.len());
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    #[ignore = "reads local application registration and native icons"]
    async fn live_cached_inventory_and_icons() {
        let started = std::time::Instant::now();
        let apps = list(true).await.unwrap();
        let elapsed = started.elapsed();
        let cached_at = std::time::Instant::now();
        let cached = list(false).await.unwrap();
        let cache_elapsed = cached_at.elapsed();
        assert!(Arc::ptr_eq(&apps, &cached));
        let name = std::env::var("ECHOBIRD_TEST_APPLICATION")
            .expect("Set ECHOBIRD_TEST_APPLICATION to an installed application's display name");
        let selected = apps.iter().find(|app| app.name == name).unwrap();
        // Linux filesystems may omit birth time, which is represented as unknown.
        #[cfg(not(target_os = "linux"))]
        assert!(selected.created_at.is_some());
        let icon = icon(&selected.id).await.unwrap().unwrap();
        assert!(
            icon.starts_with("data:image/png;base64,")
                || icon.starts_with("data:image/svg+xml;base64,")
        );
        let properties = details(&selected.id).await.unwrap();
        assert!(properties
            .directory
            .as_deref()
            .is_some_and(|path| std::path::Path::new(path).is_dir()));
        assert!(properties.file_size.is_some_and(|size| size > 0));
        if let Some(store_app) = apps.iter().find(|app| app.name == "NanoFile") {
            assert!(store_app.created_at.is_some());
            let properties = details(&store_app.id).await.unwrap();
            assert!(properties
                .path
                .as_deref()
                .is_some_and(|path| std::path::Path::new(path).is_dir()));
            assert!(properties.version.is_some());
            assert_eq!(properties.file_size, None);
            eprintln!("Store application properties {properties:?}");
        }
        assert!(find("system:nonexistent").await.is_err());
        eprintln!(
            "{} applications ({} with creation dates), scan {:?}, cached {:?}, selected target {:?}",
            apps.len(),
            apps.iter().filter(|app| app.created_at.is_some()).count(),
            elapsed,
            cache_elapsed,
            selected.path
        );
    }
}
