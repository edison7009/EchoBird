//! The official Copilot app's authenticated loopback control channel (app 1.1.28).
//! Its own frontend uses these commands; secrets are never sent to EchoBird UI.
use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Duration};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub(crate) const MODEL_KEY: &str = "copilot-selected-model";
static WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn same_executable(actual: &Path, expected: &Path) -> bool {
    let actual = actual.canonicalize().unwrap_or_else(|_| actual.into());
    let expected = expected.canonicalize().unwrap_or_else(|_| expected.into());
    #[cfg(windows)]
    {
        actual
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        actual == expected
    }
}

#[cfg(any(target_os = "linux", test))]
fn appimage_process_matches(actual: &Path, expected: &Path, environment: &[u8]) -> bool {
    if expected
        .extension()
        .is_none_or(|extension| extension != "AppImage")
        || actual.file_name().is_none_or(|name| name != "github")
        || !actual.to_string_lossy().contains("/.mount_")
    {
        return false;
    }
    let Some(value) = environment
        .split(|byte| *byte == 0)
        .find_map(|entry| entry.strip_prefix(b"APPIMAGE="))
        .and_then(|value| std::str::from_utf8(value).ok())
    else {
        return false;
    };
    let (Ok(actual), Ok(expected)) = (Path::new(value).canonicalize(), expected.canonicalize())
    else {
        return false;
    };
    actual == expected
}

/// AppImage's mounted executable is tied to this profile marker and outer file.
/// Only the APPIMAGE field is compared; process environment is never exposed.
#[cfg(target_os = "linux")]
pub(crate) fn verified_appimage_pid(executable: &Path) -> Option<u32> {
    if executable
        .extension()
        .is_none_or(|extension| extension != "AppImage")
    {
        return None;
    }
    let run = super::super::tool_config_manager::copilot_config_dir().join("run");
    let (_, pid) = marker(&fs::read_to_string(run.join("ws.release.port")).ok()?).ok()?;
    let process = std::path::PathBuf::from(format!("/proc/{pid}"));
    let actual = fs::read_link(process.join("exe")).ok()?;
    let environment = fs::read(process.join("environ")).ok()?;
    appimage_process_matches(&actual, executable, &environment).then_some(pid)
}

async fn verify_process(pid: u32, expected: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let actual = {
        use std::os::windows::ffi::OsStringExt;
        use windows::{
            core::PWSTR,
            Win32::{
                Foundation::{CloseHandle, WAIT_TIMEOUT},
                System::Threading::{
                    OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
                    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
                },
            },
        };
        unsafe {
            let process = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
            .map_err(|_| "accountError.initializeClient")?;
            let result = (|| -> Result<std::path::PathBuf, String> {
                if WaitForSingleObject(process, 0) != WAIT_TIMEOUT {
                    return Err("accountError.initializeClient".into());
                }
                let mut buffer = vec![0u16; 32_768];
                let mut length = buffer.len() as u32;
                QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    PWSTR(buffer.as_mut_ptr()),
                    &mut length,
                )
                .map_err(|_| "accountError.initializeClient")?;
                if WaitForSingleObject(process, 0) != WAIT_TIMEOUT {
                    return Err("accountError.initializeClient".into());
                }
                Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
                    &buffer[..length as usize],
                )))
            })();
            let _ = CloseHandle(process);
            result?
        }
    };
    #[cfg(target_os = "macos")]
    let actual = {
        let mut command = crate::utils::process::command("ps");
        command.args(["-p", &pid.to_string(), "-o", "comm="]);
        let output = tokio::process::Command::from(command)
            .output()
            .await
            .map_err(|_| "accountError.initializeClient")?;
        if !output.status.success() {
            return Err("accountError.initializeClient".into());
        }
        std::path::PathBuf::from(
            String::from_utf8(output.stdout)
                .map_err(|_| "accountError.format")?
                .trim(),
        )
    };
    #[cfg(target_os = "linux")]
    let actual =
        fs::read_link(format!("/proc/{pid}/exe")).map_err(|_| "accountError.initializeClient")?;
    #[cfg(target_os = "linux")]
    if expected
        .extension()
        .is_some_and(|extension| extension == "AppImage")
    {
        return (verified_appimage_pid(expected) == Some(pid))
            .then_some(())
            .ok_or_else(|| "accountError.initializeClient".into());
    }
    if !same_executable(&actual, expected) {
        return Err("accountError.initializeClient".into());
    }
    Ok(())
}

fn marker(content: &str) -> Result<(u16, u32), String> {
    let mut lines = content.lines();
    let port = lines
        .next()
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or("accountError.format")?;
    let pid = lines
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|pid| *pid != 0)
        .ok_or("accountError.format")?;
    Ok((port, pid))
}

pub(crate) struct Desktop {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    pid: u32,
}

impl Desktop {
    pub(crate) async fn connect() -> Result<Self, String> {
        let executable = super::super::tool_manager::get_tool_exe_path("copilotdesktop")
            .ok_or("accountError.initializeClient")?;
        let run = super::super::tool_config_manager::copilot_config_dir().join("run");
        Self::connect_at(&run, Path::new(&executable)).await
    }

    async fn connect_at(run: &Path, executable: &Path) -> Result<Self, String> {
        // This integration targets the official stable channel, not a dev instance.
        let (port, pid) = marker(
            &fs::read_to_string(run.join("ws.release.port"))
                .map_err(|_| "accountError.initializeClient")?,
        )?;
        verify_process(pid, executable).await?;
        let token_file = fs::read_to_string(run.join("ws.release.token"))
            .map_err(|_| "accountError.initializeClient")?;
        let token = token_file
            .lines()
            .next()
            .filter(|v| !v.is_empty())
            .ok_or("accountError.format")?;
        let mut address = url::Url::parse(&format!("ws://127.0.0.1:{port}/"))
            .map_err(|_| "accountError.format")?;
        address.query_pairs_mut().append_pair("token", token);
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(address.as_str()),
        )
        .await
        .map_err(|_| "accountError.initializeClient")?
        .map_err(|_| "accountError.initializeClient")?;
        Ok(Self { socket, pid })
    }

    pub(crate) async fn request(
        &mut self,
        mut command: Value,
        expected: &str,
    ) -> Result<Value, String> {
        let id = uuid::Uuid::new_v4().to_string();
        command["request_id"] = json!(id);
        self.socket
            .send(Message::Text(command.to_string().into()))
            .await
            .map_err(|_| "accountError.network")?;
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(message) = self.socket.next().await {
                let message = message.map_err(|_| "accountError.network")?;
                let Message::Text(text) = message else {
                    continue;
                };
                let event: Value =
                    serde_json::from_str(text.as_str()).map_err(|_| "accountError.format")?;
                if event
                    .get("request_id")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value != id)
                {
                    continue;
                }
                let kind = event["type"].as_str().unwrap_or_default();
                if kind == expected {
                    return Ok(event);
                }
                if matches!(
                    kind,
                    "error" | "command_error" | "model_provider_error" | "provider_model_error"
                ) || (kind == "command_response" && !event["error"].is_null())
                {
                    // Native error text can include URLs, headers, or credentials.
                    return Err("accountError.write".into());
                }
                if expected == "github_auth_success"
                    && matches!(kind, "github_auth_error" | "github_account_auth_error")
                {
                    return Err("accountError.auth".into());
                }
            }
            Err("accountError.network".into())
        })
        .await
        .map_err(|_| "accountError.expired")?
    }
}

/// Explicit writes may briefly initialize the native vault/control channel.
/// Saved account lists and model reads never create this session.
pub(crate) struct WriteSession {
    pub(crate) bridge: Desktop,
    owned: Option<super::super::process_manager::OwnedDesktopConfigProcess>,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

impl WriteSession {
    pub(crate) async fn start() -> Result<Self, String> {
        let guard = WRITE_LOCK.lock().await;
        if let Ok(bridge) = Desktop::connect().await {
            return Ok(Self {
                bridge,
                owned: None,
                _guard: guard,
            });
        }
        let owned =
            if super::super::process_manager::desktop_tool_is_running("copilotdesktop").await? {
                None
            } else {
                Some(super::super::process_manager::start_desktop_for_config(
                    "copilotdesktop",
                )?)
            };
        let bridge = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Ok(bridge) = Desktop::connect().await {
                    if owned.as_ref().is_some_and(|child| child.id() != bridge.pid) {
                        return Err::<Desktop, String>("accountError.initializeClient".into());
                    }
                    return Ok(bridge);
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        })
        .await
        .map_err(|_| "accountError.initializeClient")??;
        Ok(Self {
            bridge,
            owned,
            _guard: guard,
        })
    }

    pub(crate) fn finish(mut self) -> Result<(), String> {
        if let Some(child) = &mut self.owned {
            child.stop()?;
        }
        Ok(())
    }
}

pub(crate) fn database() -> Result<Connection, String> {
    open_database(&super::super::tool_config_manager::copilot_config_dir().join("data.db"))
}

fn open_database(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| "accountError.initializeClient")?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|_| "accountError.read")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|_| "accountError.write")?;
    Ok(connection)
}

pub(crate) fn selected_model(connection: &Connection) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT value FROM app_state WHERE key=?1",
            [MODEL_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "accountError.read".into())
}

pub(crate) fn set_selected_model(
    connection: &mut Connection,
    value: Option<&str>,
) -> Result<(), String> {
    let transaction = connection.transaction().map_err(|_| "accountError.write")?;
    match value {
        Some(value) => {
            transaction.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')", params![MODEL_KEY, value]).map_err(|_| "accountError.write")?;
        }
        None => {
            transaction
                .execute("DELETE FROM app_state WHERE key=?1", [MODEL_KEY])
                .map_err(|_| "accountError.write")?;
        }
    }
    transaction
        .commit()
        .map_err(|_| "accountError.write".into())
}

pub(crate) struct AccountChange {
    pub(crate) id: String,
    previous: Option<(Option<String>, String, String)>,
    pub(crate) defaults: Vec<String>,
}

pub(crate) fn write_account(
    connection: &mut Connection,
    login: &str,
    host: &str,
    token: &str,
) -> Result<AccountChange, String> {
    let transaction = connection.transaction().map_err(|_| "accountError.write")?;
    let previous: Option<(String, Option<String>, String, String)> = transaction.query_row(
        "SELECT id,access_token,token_updated_at,updated_at FROM accounts WHERE lower(login)=lower(?1) AND lower(host)=lower(?2) AND kind='github'",
        params![login, host], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional().map_err(|_| "accountError.read")?;
    let defaults = {
        let mut statement = transaction
            .prepare("SELECT id FROM accounts WHERE is_default=1")
            .map_err(|_| "accountError.read")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| "accountError.read")?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "accountError.read")?;
        rows
    };
    let id = previous
        .as_ref()
        .map(|row| row.0.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if previous.is_some() {
        transaction.execute("UPDATE accounts SET access_token=?1,token_updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2", params![token, id]).map_err(|_| "accountError.write")?;
    } else {
        transaction.execute("INSERT INTO accounts(id,login,access_token,host,kind,settings_json) VALUES(?1,?2,?3,?4,'github','{}')", params![id, login, token, host]).map_err(|_| "accountError.write")?;
    }
    transaction.commit().map_err(|_| "accountError.write")?;
    Ok(AccountChange {
        id,
        previous: previous.map(|(_, token, token_time, updated)| (token, token_time, updated)),
        defaults,
    })
}

pub(crate) fn rollback_account(
    connection: &mut Connection,
    change: &AccountChange,
) -> Result<(), String> {
    let transaction = connection.transaction().map_err(|_| "accountError.write")?;
    if let Some((token, token_time, updated)) = &change.previous {
        transaction
            .execute(
                "UPDATE accounts SET access_token=?1,token_updated_at=?2,updated_at=?3 WHERE id=?4",
                params![token, token_time, updated, change.id],
            )
            .map_err(|_| "accountError.write")?;
    } else {
        transaction
            .execute("DELETE FROM accounts WHERE id=?1", [&change.id])
            .map_err(|_| "accountError.write")?;
    }
    transaction
        .execute("UPDATE accounts SET is_default=0 WHERE is_default=1", [])
        .map_err(|_| "accountError.write")?;
    for id in &change.defaults {
        transaction
            .execute("UPDATE accounts SET is_default=1 WHERE id=?1", [id])
            .map_err(|_| "accountError.write")?;
    }
    transaction
        .commit()
        .map_err(|_| "accountError.write".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_process_identity_requires_a_live_pid_and_matching_executable() {
        let expected = std::env::current_exe().unwrap();
        let pid = std::process::id();
        verify_process(pid, &expected).await.unwrap();
        for (pid, executable) in [(0, expected), (pid, "wrong-client.exe".into())] {
            assert_eq!(
                verify_process(pid, &executable).await.unwrap_err(),
                "accountError.initializeClient"
            );
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_process_identity_rejects_retained_exited_pids_including_exit_code_259() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{Foundation::HANDLE, System::Threading::GetExitCodeProcess};

        let executable = std::path::PathBuf::from(std::env::var_os("ComSpec").unwrap());
        for exit_code in [0, 259] {
            let mut child = crate::utils::process::command(&executable)
                .args(["/d", "/c", &format!("exit {exit_code}")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            assert_eq!(child.wait().unwrap().code(), Some(exit_code));
            let mut native_exit_code = 0;
            unsafe {
                GetExitCodeProcess(HANDLE(child.as_raw_handle()), &mut native_exit_code).unwrap();
            }
            assert_eq!(native_exit_code, exit_code as u32);
            assert_eq!(
                verify_process(child.id(), &executable).await.unwrap_err(),
                "accountError.initializeClient"
            );
            drop(child);
        }
    }

    #[tokio::test]
    async fn authenticated_channel_has_live_pid_positive_control_and_redacts_native_errors() {
        let directory = std::env::temp_dir().join(format!("copilot-ws-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        fs::write(
            directory.join("ws.release.port"),
            format!("{port}\n{}\n", std::process::id()),
        )
        .unwrap();
        fs::write(directory.join("ws.release.token"), "fixture-secret\n").unwrap();
        assert!(
            Desktop::connect_at(&directory, Path::new("wrong-client.exe"))
                .await
                .is_err()
        );
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // tungstenite's callback fixes the error type to its HTTP response.
            #[allow(clippy::result_large_err)]
            let mut socket=tokio_tungstenite::accept_hdr_async(stream,|request:&tokio_tungstenite::tungstenite::handshake::server::Request,response:tokio_tungstenite::tungstenite::handshake::server::Response|{
                assert_eq!(request.uri().query(),Some("token=fixture-secret"));
                Ok(response)
            }).await.unwrap();
            socket
                .send(Message::Text(
                    json!({"type":"server_hello"}).to_string().into(),
                ))
                .await
                .unwrap();
            let command: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .into_text()
                    .unwrap()
                    .as_str(),
            )
            .unwrap();
            assert_eq!(command["type"], "list_model_providers");
            socket.send(Message::Text(json!({"type":"model_providers_list","request_id":"older-request","providers":[{"id":"wrong"}]}).to_string().into())).await.unwrap();
            socket.send(Message::Text(json!({"type":"model_providers_list","request_id":command["request_id"],"providers":[{"id":"positive-control","name":"EchoBird"}]}).to_string().into())).await.unwrap();
            let command: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .into_text()
                    .unwrap()
                    .as_str(),
            )
            .unwrap();
            socket.send(Message::Text(json!({"type":"command_response","request_id":command["request_id"],"error":"native error echoes fixture-secret"}).to_string().into())).await.unwrap();
        });
        let mut desktop = Desktop::connect_at(&directory, &std::env::current_exe().unwrap())
            .await
            .unwrap();
        let response = desktop
            .request(
                json!({"type":"list_model_providers"}),
                "model_providers_list",
            )
            .await
            .unwrap();
        assert_eq!(response["providers"][0]["id"], "positive-control");
        let error = desktop
            .request(
                json!({"type":"upsert_model_provider","secret":{"value":"fixture-secret"}}),
                "model_provider_upserted",
            )
            .await
            .unwrap_err();
        assert_eq!(error, "accountError.write");
        assert!(!error.contains("fixture-secret"));
        server.await.unwrap();
        fs::remove_file(directory.join("ws.release.port")).unwrap();
        fs::remove_file(directory.join("ws.release.token")).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn marker_requires_live_identity_and_executable_match_has_positive_control() {
        assert_eq!(marker("59320\n28432\n").unwrap(), (59320, 28432));
        assert!(marker("0\n28432").is_err());
        assert!(marker("59320\n0").is_err());
        assert!(marker("59320\n").is_err());
        let path = std::env::current_exe().unwrap();
        assert!(same_executable(&path, &path));
        assert!(!same_executable(&path, Path::new("wrong-client.exe")));
    }

    #[test]
    fn native_transactions_preserve_unrelated_accounts_and_preferences_and_rollback() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE accounts(id TEXT PRIMARY KEY,login TEXT,access_token TEXT,host TEXT,kind TEXT,is_default INTEGER DEFAULT 0,settings_json TEXT,token_updated_at TEXT DEFAULT 'before',updated_at TEXT DEFAULT 'before'); CREATE TABLE app_state(key TEXT PRIMARY KEY,value TEXT,updated_at TEXT DEFAULT 'before'); INSERT INTO accounts VALUES('mine','mine','old-secret','github.com','github',0,'{\"theme\":true}','before','before'); INSERT INTO accounts VALUES('other','other','untouched','github.com','github',1,'{\"custom\":true}','before','before'); INSERT INTO app_state(key,value) VALUES('theme','dark');").unwrap();
        let other: (String, String, String) = connection
            .query_row(
                "SELECT access_token,settings_json,updated_at FROM accounts WHERE id='other'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let change = write_account(&mut connection, "mine", "github.com", "fixture-new").unwrap();
        assert_eq!(change.id, "mine");
        let custom: String = connection
            .query_row(
                "SELECT settings_json FROM accounts WHERE id='mine'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(custom, "{\"theme\":true}");
        connection
            .execute("UPDATE accounts SET is_default=(id='mine')", [])
            .unwrap();
        rollback_account(&mut connection, &change).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT access_token FROM accounts WHERE id='mine'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "old-secret"
        );
        let after: (String, String, String) = connection
            .query_row(
                "SELECT access_token,settings_json,updated_at FROM accounts WHERE id='other'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(after, other);
        assert_eq!(
            connection
                .query_row(
                    "SELECT is_default FROM accounts WHERE id='other'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        let new = write_account(&mut connection, "new", "github.com", "fixture-new").unwrap();
        rollback_account(&mut connection, &new).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM accounts", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
        set_selected_model(&mut connection, Some("echo/selected")).unwrap();
        assert_eq!(
            selected_model(&connection).unwrap().as_deref(),
            Some("echo/selected")
        );
        assert_eq!(
            connection
                .query_row("SELECT value FROM app_state WHERE key='theme'", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "dark"
        );
        set_selected_model(&mut connection, None).unwrap();
        assert!(selected_model(&connection).unwrap().is_none());
    }
}
#[test]
fn appimage_identity_requires_the_native_mount_and_matching_outer_file() {
    let root = std::env::temp_dir().join(format!("copilot-appimage-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let expected = root.join("GitHub-Copilot-linux-arm64.AppImage");
    let other = root.join("Other-Copilot.AppImage");
    fs::write(&expected, b"fixture").unwrap();
    fs::write(&other, b"fixture").unwrap();
    let environment = |path: &Path| {
        format!("UNRELATED=private-fixture\0APPIMAGE={}\0", path.display()).into_bytes()
    };
    let actual = Path::new("/tmp/.mount_GitHub123/usr/bin/github");
    assert!(appimage_process_matches(
        actual,
        &expected,
        &environment(&expected)
    ));
    assert!(!appimage_process_matches(
        actual,
        &expected,
        &environment(&other)
    ));
    assert!(!appimage_process_matches(
        actual,
        &expected,
        b"UNRELATED=private-fixture\0"
    ));
    assert!(!appimage_process_matches(
        Path::new("/tmp/.mount_GitHub123/usr/bin/other"),
        &expected,
        &environment(&expected)
    ));
    assert!(!appimage_process_matches(
        Path::new("/usr/bin/github"),
        &expected,
        &environment(&expected)
    ));
    fs::remove_file(&expected).unwrap();
    assert!(!appimage_process_matches(
        actual,
        &expected,
        &environment(&expected)
    ));
    fs::remove_file(other).unwrap();
    fs::remove_dir(root).unwrap();
}
