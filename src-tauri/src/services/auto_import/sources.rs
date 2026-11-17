use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) struct Source {
    pub name: &'static str,
    paths: Vec<PathBuf>,
    directories: Vec<PathBuf>,
}

fn environment(key: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(key)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}

pub(super) fn discover() -> Result<Vec<Source>, String> {
    let home = dirs::home_dir().ok_or("Home directory unavailable")?;
    let config = environment("XDG_CONFIG_HOME", home.join(".config"));
    let roaming = environment("APPDATA", home.join("AppData/Roaming"));
    let manager = environment(
        "ABV_DATA_DIR",
        pointer(&home.join(".antigravity_tools_location"))
            .unwrap_or_else(|| home.join(".antigravity_tools")),
    );
    let cockpit = environment(
        "COCKPIT_TOOLS_DATA_DIR",
        prefer(
            &home.join(".cockpit_tools"),
            &home.join(".antigravity_cockpit"),
        ),
    );
    let plus = environment("CODEX_PLUS_STATE_DIR", home.join(".codex-session-delete"));
    let router = environment(
        "DATA_DIR",
        if cfg!(windows) {
            roaming.join("9router")
        } else {
            home.join(".9router")
        },
    );
    let ccr = if cfg!(windows) {
        roaming.join("claude-code-router")
    } else {
        home.join(".claude-code-router")
    };
    let magpie = config.join("magpie");
    Ok(vec![
        Source {
            name: "9router",
            paths: vec![prefer(
                &router.join("db/data.sqlite"),
                &router.join("db.json"),
            )],
            directories: vec![],
        },
        Source {
            name: "Antigravity-Manager",
            paths: vec![manager.join("gui_config.json")],
            directories: vec![manager.join("accounts")],
        },
        Source {
            name: "cockpit-tools",
            paths: vec![],
            directories: vec![
                cockpit.join("accounts"),
                cockpit.join("codex_accounts"),
                cockpit.join("claude_accounts"),
                cockpit.join("github_copilot_accounts"),
                cockpit.join("cursor_accounts"),
                cockpit.join("kiro_accounts"),
                cockpit.join("qoder_accounts"),
                cockpit.join("trae_accounts"),
                cockpit.join("windsurf_accounts"),
                cockpit.join("codebuddy_accounts"),
                cockpit.join("codebuddy_cn_accounts"),
                cockpit.join("workbuddy_accounts"),
                cockpit.join("grok_accounts"),
                cockpit.join("zed_accounts"),
            ],
        },
        Source {
            name: "cc-switch",
            paths: vec![prefer(
                &home.join(".cc-switch/cc-switch.db"),
                &home.join(".cc-switch/config.json"),
            )],
            directories: vec![],
        },
        Source {
            name: "CodexPlusPlus",
            paths: vec![
                settings_file(
                    &home,
                    &plus,
                    std::env::var_os("CODEX_PLUS_STATE_DIR").is_some_and(|value| !value.is_empty()),
                ),
                plus.join("gateway.json"),
            ],
            directories: vec![],
        },
        Source {
            name: "magpie",
            paths: vec![magpie.join("providers.json"), magpie.join("logins.json")],
            directories: vec![],
        },
        Source {
            name: "CLIProxyAPI",
            paths: vec![home.join(".cli-proxy-api/config.yaml")],
            directories: vec![home.join(".cli-proxy-api")],
        },
        Source {
            name: "claude-code-router",
            paths: vec![prefer(&ccr.join("config.sqlite"), &ccr.join("config.json"))],
            directories: vec![],
        },
    ])
}

fn pointer(path: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(path).ok()?;
    let path = PathBuf::from(contents.trim());
    path.is_absolute().then_some(path)
}

fn settings_file(home: &Path, state: &Path, overridden: bool) -> PathBuf {
    if !overridden {
        if let Some(path) = pointer(&home.join("settings.path")) {
            return path;
        }
    }
    state.join("settings.json")
}

fn prefer(current: &Path, legacy: &Path) -> PathBuf {
    if !matches!(fs::symlink_metadata(current), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        current.into()
    } else {
        legacy.into()
    }
}

pub(super) struct ReadResult {
    pub documents: Vec<Value>,
    pub failures: usize,
}

pub(super) fn read(source: &Source) -> Result<ReadResult, String> {
    let mut result = ReadResult {
        documents: vec![],
        failures: 0,
    };
    let mut paths = source.paths.clone();
    for directory in &source.directories {
        match fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) if entry.path().extension().is_some_and(|e| e == "json") => {
                            paths.push(entry.path())
                        }
                        Err(_) => result.failures += 1,
                        _ => (),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => result.failures += 1,
        }
    }
    paths.sort();
    paths.dedup();
    for path in paths {
        match read_document(source.name, &path) {
            Ok(Some(documents)) => result.documents.extend(documents),
            Ok(None) => (),
            Err(_) => result.failures += 1,
        }
    }
    Ok(result)
}

fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read source".into()),
    };
    if !file
        .metadata()
        .map_err(|_| "Cannot inspect source")?
        .is_file()
    {
        return Err("Invalid source".into());
    }
    let mut data = vec![];
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "Cannot read source")?;
    if data.len() > 16 * 1024 * 1024 {
        return Err("Source too large".into());
    }
    Ok(Some(data))
}

fn read_document(source: &str, path: &Path) -> Result<Option<Vec<Value>>, String> {
    if matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("db" | "sqlite")
    ) {
        if !path.try_exists().map_err(|_| "Cannot inspect database")? {
            return Ok(None);
        }
        return sqlite(source, path).map(Some);
    }
    let Some(data) = bytes(path)? else {
        return Ok(None);
    };
    let value = if path.extension().is_some_and(|e| e == "yaml") {
        serde_yaml_ng::from_slice(&data).map_err(|_| "Invalid source")?
    } else {
        serde_json::from_slice(&data).map_err(|_| "Invalid source")?
    };
    let value = if source == "cockpit-tools" {
        decode_local_account(path, value)?
    } else {
        value
    };
    Ok(Some(vec![value]))
}

fn decode_local_account(path: &Path, mut value: Value) -> Result<Value, String> {
    let root = path
        .parent()
        .and_then(Path::parent)
        .ok_or("Invalid source path")?;
    if value.get("ciphertext").is_some() {
        if value["algorithm"] != "AES-256-GCM" {
            return Err("Unsupported encryption".into());
        }
        return decrypt_envelope(&root.join("secure-account-storage.key"), &value);
    }
    if let Some(envelope) = value.get("token_encrypted") {
        if value.get("token").is_none() {
            value["token"] = decrypt_envelope(&root.join("account-token.key"), envelope)?;
        }
    }
    Ok(value)
}

fn decrypt_envelope(key_path: &Path, envelope: &Value) -> Result<Value, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use base64::{engine::general_purpose::STANDARD, Engine};
    if envelope["version"] != 1 {
        return Err("Unsupported encryption version".into());
    }
    let key = bytes(key_path)?.ok_or("Source key unavailable")?;
    let key = std::str::from_utf8(&key).map_err(|_| "Invalid source key")?;
    let key = STANDARD
        .decode(key.trim())
        .map_err(|_| "Invalid source key")?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "Invalid source key")?;
    let nonce = STANDARD
        .decode(
            envelope["nonce"]
                .as_str()
                .ok_or("Invalid encrypted source")?,
        )
        .map_err(|_| "Invalid encrypted source")?;
    if nonce.len() != 12 {
        return Err("Invalid encrypted source".into());
    }
    let payload = STANDARD
        .decode(
            envelope["ciphertext"]
                .as_str()
                .ok_or("Invalid encrypted source")?,
        )
        .map_err(|_| "Invalid encrypted source")?;
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce), payload.as_slice())
        .map_err(|_| "Cannot decrypt source")?;
    serde_json::from_slice(&plain).map_err(|_| "Invalid encrypted source".into())
}

struct Snapshot(PathBuf);
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sqlite(source: &str, path: &Path) -> Result<Vec<Value>, String> {
    let originals = [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
    ];
    let before = originals
        .iter()
        .map(|path| fingerprint(path))
        .collect::<Result<Vec<_>, _>>()?;
    let directory = std::env::temp_dir().join(format!("echobird-import-{}", uuid::Uuid::new_v4()));
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder
        .create(&directory)
        .map_err(|_| "Cannot create snapshot")?;
    let _guard = Snapshot(directory.clone());
    let snapshot = directory.join("source.db");
    for suffix in ["", "-wal"] {
        let original = PathBuf::from(format!("{}{suffix}", path.display()));
        if let Some(data) = bytes(&original)? {
            let target = PathBuf::from(format!("{}{suffix}", snapshot.display()));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            use std::io::Write;
            options
                .open(target)
                .and_then(|mut file| file.write_all(&data))
                .map_err(|_| "Cannot write snapshot")?;
        }
    }
    let after = originals
        .iter()
        .map(|path| fingerprint(path))
        .collect::<Result<Vec<_>, _>>()?;
    if before != after {
        return Err("Source changed during snapshot".into());
    }
    let connection = rusqlite::Connection::open_with_flags(
        &snapshot,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|_| "Cannot open snapshot")?;
    let query = match source {
        "cc-switch" => "SELECT json_set(settings_config, '$.name', name) FROM providers ORDER BY id",
        "9router" => "SELECT json_set(data, '$.provider', provider, '$.authType', authType, '$.name', name, '$.email', email) FROM providerConnections ORDER BY id",
        "claude-code-router" => "SELECT value_json FROM app_config WHERE key = 'default'",
        _ => return Err("Unsupported database".into()),
    };
    let mut statement = connection
        .prepare(query)
        .map_err(|_| "Unsupported database schema")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| "Cannot read snapshot")?;
    rows.map(|row| {
        serde_json::from_str(&row.map_err(|_| "Cannot read row")?)
            .map_err(|_| "Invalid source row".into())
    })
    .collect()
}

fn fingerprint(path: &Path) -> Result<Option<(u64, std::time::SystemTime)>, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some((
            metadata.len(),
            metadata.modified().map_err(|_| "Cannot inspect source")?,
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Cannot inspect source".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_state_directory_ignores_the_global_settings_pointer() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join("isolated");
        let previous = home.path().join("previous/settings.json");
        fs::write(
            home.path().join("settings.path"),
            previous.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(settings_file(home.path(), &state, false), previous);
        assert_eq!(
            settings_file(home.path(), &state, true),
            state.join("settings.json")
        );
    }

    #[test]
    fn reads_provider_database_schemas_and_retains_names_and_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cc-switch.db");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE providers (id TEXT, name TEXT, settings_config TEXT); INSERT INTO providers VALUES ('fixture', 'Configured provider', '{\"env\":{\"ANTHROPIC_AUTH_TOKEN\":\"synthetic\",\"ANTHROPIC_BASE_URL\":\"https://api.example\"}}');").unwrap();
        drop(connection);
        let documents = sqlite("cc-switch", &path).unwrap();
        let parsed = super::super::parse::parse("cc-switch", &documents[0]);
        assert_eq!(
            parsed.models[0].name.as_deref(),
            Some("Configured provider")
        );
        assert_eq!(parsed.models[0].api_key.as_deref(), Some("synthetic"));
        let path = directory.path().join("9router.sqlite");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE providerConnections (id TEXT, provider TEXT, authType TEXT, name TEXT, email TEXT, data TEXT); INSERT INTO providerConnections VALUES ('fixture', 'custom', 'apikey', 'Configured node', NULL, '{\"apiKey\":\"synthetic\",\"providerSpecificData\":{\"baseUrl\":\"https://api.example/v1\"}}');").unwrap();
        drop(connection);
        let documents = sqlite("9router", &path).unwrap();
        let parsed = super::super::parse::parse("9router", &documents[0]);
        assert_eq!(parsed.models[0].name.as_deref(), Some("Configured node"));
        assert_eq!(parsed.models[0].api_key.as_deref(), Some("synthetic"));
    }

    #[test]
    fn reads_encrypted_local_accounts_without_changing_the_source_key_or_file() {
        use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
        use base64::{engine::general_purpose::STANDARD, Engine};
        let directory = tempfile::tempdir().unwrap();
        let accounts = directory.path().join("codex_accounts");
        fs::create_dir(&accounts).unwrap();
        let path = accounts.join("fixture.json");
        let key_path = directory.path().join("secure-account-storage.key");
        fs::write(&key_path, STANDARD.encode([42u8; 32])).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&[42u8; 32]).unwrap();
        let nonce = [7u8; 12];
        let plain = br#"{"tokens":{"access_token":"synthetic"}}"#;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
            .unwrap();
        let envelope = serde_json::json!({"version":1,"algorithm":"AES-256-GCM","nonce":STANDARD.encode(nonce),"ciphertext":STANDARD.encode(ciphertext)});
        fs::write(&path, envelope.to_string()).unwrap();
        let before = fs::read(&path).unwrap();
        let before_key = fs::read(&key_path).unwrap();
        let rows = read_document("cockpit-tools", &path).unwrap().unwrap();
        assert_eq!(rows[0]["tokens"]["access_token"], "synthetic");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(&key_path).unwrap(), before_key);
        fs::write(&key_path, STANDARD.encode([43u8; 32])).unwrap();
        assert!(read_document("cockpit-tools", &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn sqlite_snapshot_reads_live_wal_without_changing_the_source() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.sqlite");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE app_config (key TEXT PRIMARY KEY, value_json TEXT); INSERT INTO app_config VALUES ('default', '{\"Providers\":[]}');").unwrap();
        let original = fs::read(&path).unwrap();
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        let original_wal = fs::read(&wal).unwrap();
        let rows = sqlite("claude-code-router", &path).unwrap();
        assert_eq!(rows[0]["Providers"], serde_json::json!([]));
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&wal).unwrap(), original_wal);
    }

    #[test]
    fn missing_files_are_normal_and_corrupt_files_do_not_hide_valid_sources() {
        let directory = tempfile::tempdir().unwrap();
        let valid = directory.path().join("providers.json");
        let invalid = directory.path().join("logins.json");
        fs::write(&valid, b"{\"providers\":[]}").unwrap();
        fs::write(&invalid, b"invalid {").unwrap();
        let source = Source {
            name: "magpie",
            paths: vec![valid, invalid, directory.path().join("missing")],
            directories: vec![],
        };
        let read = read(&source).unwrap();
        assert_eq!(read.documents.len(), 1);
        assert_eq!(read.failures, 1);
    }

    #[test]
    fn current_database_is_preferred_to_stale_json() {
        let directory = tempfile::tempdir().unwrap();
        let current = directory.path().join("data.sqlite");
        let legacy = directory.path().join("db.json");
        fs::write(&legacy, b"{}").unwrap();
        assert_eq!(prefer(&current, &legacy), legacy);
        fs::write(&current, b"not a database").unwrap();
        assert_eq!(prefer(&current, &legacy), current);
        assert!(read_document("9router", &current).is_err());
    }
}
