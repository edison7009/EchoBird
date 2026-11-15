//! Acquiring bundled apps adds them to the desktop without installing software.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Deserialize)]
struct Catalog {
    apps: Vec<App>,
}
#[derive(Deserialize)]
struct App {
    id: String,
    bundled: bool,
}
#[derive(Default, Deserialize, Serialize)]
struct Acquired {
    apps: BTreeSet<String>,
}

pub fn is_bundled(id: &str) -> bool {
    let catalog: Catalog = serde_json::from_str(include_str!("../../../src/data/bundledApps.json"))
        .expect("bundled app catalog is valid");
    catalog.apps.iter().any(|app| app.id == id && app.bundled)
}

fn path() -> std::path::PathBuf {
    crate::utils::platform::echobird_dir().join("builtin-apps.json")
}

pub fn is_acquired(id: &str) -> bool {
    is_bundled(id)
        && super::json_store::read::<Acquired>(&path()).is_ok_and(|state| state.apps.contains(id))
}

fn acquire_at(path: &Path, id: &str) -> Result<(), String> {
    if !is_bundled(id) {
        return Err("Unknown bundled app".into());
    }
    super::json_store::mutate::<Acquired, _>(path, |state| {
        let changed = state.apps.insert(id.into());
        Ok(((), changed))
    })
}

#[tauri::command]
pub fn acquire_builtin_app(id: String) -> Result<(), String> {
    acquire_at(&path(), &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acquiring_is_local_idempotent_and_validated() {
        let root = std::env::temp_dir().join(format!("builtin-apps-{}", uuid::Uuid::new_v4()));
        let path = root.join("builtin-apps.json");
        assert!(super::super::json_store::read::<Acquired>(&path)
            .unwrap()
            .apps
            .is_empty());
        assert!(acquire_at(&path, "claudedesktop").is_err());
        assert!(!path.exists());
        acquire_at(&path, "translator").unwrap();
        let first = std::fs::read(&path).unwrap();
        acquire_at(&path, "translator").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first);
        let state = super::super::json_store::read::<Acquired>(&path).unwrap();
        assert_eq!(state.apps, BTreeSet::from(["translator".into()]));
        std::fs::remove_dir_all(root).unwrap();
    }
}
