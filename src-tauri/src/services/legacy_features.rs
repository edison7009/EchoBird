//! One-time imports of built-in features previously stored by the plugin host.
use serde_json::Value;
use std::path::Path;

pub(crate) fn read_data(root: &Path, name: &str) -> Result<Value, String> {
    let path = root.join("plugins/data").join(format!("{name}.json"));
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid legacy feature data".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn selected(value: &Value, plugin: &str, id: &str) -> bool {
    value["pluginId"] == plugin && value["id"] == id
}

pub(crate) fn migrate_theme(path: &Path) -> Result<(), String> {
    let root = path.parent().ok_or("Invalid settings path")?;
    let host = read_data(root, "host")?;
    if host.is_null() {
        return Ok(());
    }
    crate::services::json_store::mutate(path, |settings: &mut Value| {
        if settings["builtinThemeMigrated"] == true {
            return Ok(((), false));
        }
        if settings.is_null() {
            *settings = serde_json::json!({"themeMode":"dark"});
        }
        let settings = settings.as_object_mut().ok_or("Invalid saved settings")?;
        if selected(&host["theme"], "echobird.ocean-theme", "ocean") {
            settings.insert("colorTheme".into(), Value::String("ocean".into()));
        }
        settings.insert("builtinThemeMigrated".into(), Value::Bool(true));
        Ok(((), true))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_ocean_is_imported_once_preserving_settings_and_old_data() {
        let root =
            std::env::temp_dir().join(format!("echobird-theme-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("plugins/data")).unwrap();
        let legacy = root.join("plugins/data/host.json");
        let original = r#"{"theme":{"pluginId":"echobird.ocean-theme","id":"ocean"}}"#;
        std::fs::write(&legacy, original).unwrap();
        let path = root.join("settings.json");
        std::fs::write(&path, r#"{"themeMode":"light","unknown":42}"#).unwrap();
        migrate_theme(&path).unwrap();
        let mut settings: Value = crate::services::cursor_auth::read(&path).unwrap();
        assert_eq!(settings["colorTheme"], "ocean");
        assert_eq!(settings["themeMode"], "light");
        assert_eq!(settings["unknown"], 42);
        settings["colorTheme"] = Value::String("aurora".into());
        crate::services::cursor_auth::write(&path, &settings).unwrap();
        migrate_theme(&path).unwrap();
        let settings: Value = crate::services::cursor_auth::read(&path).unwrap();
        assert_eq!(settings["colorTheme"], "aurora");
        assert_eq!(std::fs::read_to_string(legacy).unwrap(), original);
        std::fs::remove_dir_all(root).unwrap();
    }
}
