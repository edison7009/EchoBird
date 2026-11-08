//! Keep AppKit window controls consistent with the application's saved theme.
use crate::commands::settings_commands;
use tauri::{Listener, Manager, Theme};

fn theme_for_mode(mode: &str) -> Option<Option<Theme>> {
    match mode {
        "dark" => Some(Some(Theme::Dark)),
        "light" => Some(Some(Theme::Light)),
        "system" => Some(None),
        _ => None,
    }
}

fn theme_from_event(payload: &str) -> Option<Option<Theme>> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    theme_for_mode(value.get("mode")?.as_str()?)
}

fn apply(app: &tauri::AppHandle, theme: Option<Theme>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.set_theme(theme) {
            log::warn!("[macOS] Failed to synchronize native appearance: {error}");
        }
    }
}

pub(crate) fn install(app: &tauri::App) {
    let settings = settings_commands::get_settings();
    let initial = settings
        .theme_mode
        .as_deref()
        .and_then(theme_for_mode)
        .flatten();
    apply(app.handle(), initial);

    let handle = app.handle().clone();
    app.listen("echobird-theme-changed", move |event| {
        if let Some(theme) = theme_from_event(event.payload()) {
            apply(&handle, theme);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_appearance_follows_explicit_modes_and_releases_system_override() {
        assert_eq!(theme_for_mode("dark"), Some(Some(Theme::Dark)));
        assert_eq!(theme_for_mode("light"), Some(Some(Theme::Light)));
        assert_eq!(theme_for_mode("system"), Some(None));
    }

    #[test]
    fn palette_and_plugin_payloads_preserve_the_native_theme_source() {
        assert_eq!(
            theme_from_event(r#"{"mode":"dark","colorTheme":"aurora","pluginTheme":null}"#),
            Some(Some(Theme::Dark))
        );
        assert_eq!(theme_from_event(r#"{"mode":"system"}"#), Some(None));
        for payload in [
            "invalid",
            "null",
            "{}",
            r#"{"mode":false}"#,
            r#"{"mode":"blue"}"#,
        ] {
            assert_eq!(theme_from_event(payload), None);
        }
    }
}
