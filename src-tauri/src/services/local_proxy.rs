//! Reserve loopback listeners before publishing their addresses to the UI.

use std::fs;
use std::io;
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

use crate::utils::platform::echobird_dir;

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedPort {
    port: u16,
    #[serde(default)]
    pending_migrations: Vec<u16>,
}

pub(crate) fn saved_port(name: &str, default_port: u16) -> Result<u16, String> {
    let path = echobird_dir()
        .join("config")
        .join(format!("{name}-port.json"));
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<SavedPort>(&content)
            .map(|saved| saved.port)
            .map_err(|e| e.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(default_port),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn bind(name: &str, default_port: u16) -> Result<TcpListener, String> {
    let path = echobird_dir()
        .join("config")
        .join(format!("{name}-port.json"));
    let mut saved = match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<SavedPort>(&content).map_err(|e| e.to_string())?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => SavedPort {
            port: default_port,
            pending_migrations: Vec::new(),
        },
        Err(error) => return Err(error.to_string()),
    };
    let preferred = saved.port;
    let listener = bind_with_fallback(preferred)
        .map_err(|e| format!("{name}: cannot bind a loopback listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    if port != preferred {
        log::warn!("[{name}] port {preferred} unavailable; using 127.0.0.1:{port}");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Save both the new port and old addresses before editing client files.
    // Interrupted/failed migrations are retried on the next launch.
    if port != preferred {
        saved.pending_migrations.push(preferred);
    }
    saved.port = port;
    let save = |saved: &SavedPort| -> Result<(), String> {
        let content = serde_json::to_string(saved).map_err(|e| e.to_string())?;
        fs::write(&path, content).map_err(|e| e.to_string())
    };
    save(&saved)?;
    // Keep the listener open throughout migration; there is no bind/rebind race.
    saved.pending_migrations.retain(|old| {
        if *old == port {
            return false;
        }
        if let Err(error) = super::tool_config_manager::migrate_local_proxy_port(*old, port) {
            log::error!("[{name}] client URL migration incomplete: {error}");
            return true;
        }
        false
    });
    save(&saved)?;
    Ok(listener)
}

pub(super) fn bind_with_fallback(port: u16) -> io::Result<TcpListener> {
    match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => Ok(listener),
        Err(error) if should_fallback(&error) => TcpListener::bind((Ipv4Addr::LOCALHOST, 0)),
        Err(error) => Err(error),
    }
}

fn should_fallback(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
    )
}

/// Replace only complete HTTP loopback authorities, preserving paths, comments,
/// whitespace and credentials in the JSON/JSONC/TOML/YAML files tools own.
fn replace_port(content: &str, old: u16, new: u16) -> String {
    let mut result = content.to_string();
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        let from = format!("http://{host}:{old}");
        let to = format!("http://{host}:{new}");
        let mut output = String::with_capacity(result.len());
        let mut start = 0;
        for (index, _) in result.match_indices(&from) {
            let end = index + from.len();
            let boundary = result[end..].chars().next();
            if boundary
                .is_none_or(|c| c.is_whitespace() || matches!(c, '/' | '"' | '\'' | '?' | '#'))
            {
                output.push_str(&result[start..index]);
                output.push_str(&to);
                start = end;
            }
        }
        output.push_str(&result[start..]);
        result = output;
    }
    result
}

pub(crate) fn migrate_file(path: &Path, old: u16, new: u16) -> Result<(), String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let updated = replace_port(&content, old, new);
    if updated != content {
        // Retain the original file beside it before the first migration.
        let backup = path.with_extension(format!(
            "{}.before-proxy-port-{old}",
            path.extension().unwrap_or_default().to_string_lossy()
        ));
        if !backup.exists() {
            fs::copy(path, backup).map_err(|e| e.to_string())?;
        }
        fs::write(path, updated).map_err(|e| format!("write {}: {e}", path.display()))?;
        log::info!(
            "[LocalProxy] updated port {old} -> {new} in {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_port_is_reused() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let reused = bind_with_fallback(port).unwrap();
        assert_eq!(reused.local_addr().unwrap().port(), port);
    }

    #[test]
    fn migration_backs_up_and_preserves_client_settings() {
        let dir =
            std::env::temp_dir().join(format!("echobird-proxy-port-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let original = "{\"env\":{\"ANTHROPIC_BASE_URL\":\"http://127.0.0.1:53682/claudecode\"},\"permissions\":{\"allow\":[\"Read\"]}}";
        fs::write(&path, original).unwrap();
        migrate_file(&path, 53682, 41234).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original.replace(":53682/", ":41234/")
        );
        assert_eq!(
            fs::read_to_string(dir.join("settings.json.before-proxy-port-53682")).unwrap(),
            original
        );
        migrate_file(&path, 53682, 41234).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn occupied_port_gets_a_different_loopback_listener() {
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = occupied.local_addr().unwrap();
        let fallback = bind_with_fallback(address.port()).unwrap();
        assert_ne!(fallback.local_addr().unwrap().port(), address.port());
        assert_eq!(fallback.local_addr().unwrap().ip(), Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn permission_denied_and_address_in_use_allow_fallback() {
        assert!(should_fallback(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(should_fallback(&io::Error::from(io::ErrorKind::AddrInUse)));
        assert!(!should_fallback(&io::Error::from(
            io::ErrorKind::OutOfMemory
        )));
    }

    #[test]
    fn migration_preserves_paths_format_and_unrelated_urls() {
        let original = "# keep\r\nurl = \"http://127.0.0.1:53683/v1\"\r\nother = 'http://localhost:53683'\nremote = \"https://example.com:53683/v1\"\nlonger = \"http://127.0.0.1:536830/v1\"\nhost = \"http://127.0.0.1:53683.example.com\"";
        let updated = replace_port(original, 53683, 41234);
        assert_eq!(
            updated,
            original
                .replace("127.0.0.1:53683/v1", "127.0.0.1:41234/v1")
                .replace("localhost:53683'", "localhost:41234'")
        );
        assert_eq!(replace_port(&updated, 53683, 41234), updated);
    }
}
