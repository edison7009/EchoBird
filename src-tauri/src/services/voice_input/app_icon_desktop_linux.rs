use base64::Engine;
use gio::prelude::*;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

pub(super) fn read(window: u64) -> Option<String> {
    read_names(&names(window)?)
}

pub(super) fn cache_key(window: u64) -> Option<String> {
    let names = names(window)?;
    (!names.is_empty()).then(|| format!("class:{}", names.join("\0")))
}

fn names(window: u64) -> Option<Vec<String>> {
    if super::super::native::wayland() {
        return None;
    }
    let (connection, _) = x11rb::connect(None).ok()?;
    let mut names = Vec::new();
    for key in [b"_GTK_APPLICATION_ID".as_slice(), b"WM_CLASS"] {
        let atom = connection.intern_atom(false, key).ok()?.reply().ok()?.atom;
        let value = connection
            .get_property(
                false,
                u32::try_from(window).ok()?,
                atom,
                AtomEnum::ANY,
                0,
                1024,
            )
            .ok()?
            .reply()
            .ok()?
            .value;
        names.extend(value.split(|byte| *byte == 0).filter_map(|value| {
            let value = std::str::from_utf8(value).ok()?;
            (!value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)))
            .then(|| value.to_owned())
        }));
    }
    Some(names)
}

pub(super) fn read_app_id(app_id: &str) -> Option<String> {
    let app = super::super::wayland_state::DesktopApp::new(app_id, "icon")?;
    read_names(&[app.app_id])
}

pub(super) fn read_names(names: &[String]) -> Option<String> {
    let app = names
        .iter()
        .find_map(|name| gio::DesktopAppInfo::new(&format!("{name}.desktop")))
        .or_else(|| {
            gio::AppInfo::all().into_iter().find_map(|app| {
                let app = app.downcast::<gio::DesktopAppInfo>().ok()?;
                let class = app.startup_wm_class()?;
                names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&class))
                    .then_some(app)
            })
        })?;
    let icon = app.icon()?;
    let path = if let Some(file) = icon.downcast_ref::<gio::FileIcon>() {
        file.file().path()?
    } else {
        let icon = icon.downcast_ref::<gio::ThemedIcon>()?;
        // Read the desktop setting directly; never spawn gsettings or a shell.
        let theme = gio::SettingsSchemaSource::default()
            .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
            .map(|schema| {
                gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
                    .string("icon-theme")
                    .to_string()
            })
            .unwrap_or_else(|| "hicolor".into());
        icon.names().iter().find_map(|name| {
            freedesktop_icons::lookup(name)
                .with_size(super::ICON_SIZE as u16)
                .with_theme(&theme)
                .with_cache()
                .find()
        })?
    };
    let bytes = std::fs::read(&path).ok()?;
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        Some(format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    } else {
        super::encode_image(image::load_from_memory(&bytes).ok()?)
    }
}
