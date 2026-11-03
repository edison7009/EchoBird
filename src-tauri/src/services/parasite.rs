// Parasite — wrap an installed AI agent CLI (currently just Claude Code) as a
// drop-in alternative to EchoBird's own agent_loop. Each user turn spawns the
// agent's CLI as a one-shot subprocess, captures its output, and emits events
// to the Mother Agent UI in the same shape as agent_event.
//
// Why this exists: Claude Code already ships a mature memory system, a
// curated tool set, and provider integration. Rather than reimplement those
// in EchoBird, we "parasite" — borrow the installed binary + its config
// without modifying it. The user taps Connect in Mother Agent; we run
// `claude -p` in its natural environment and stream the result back into our
// chat UI.
//
// Scope deliberately narrowed to Claude Code only: earlier scaffolding for
// Hermes Agent and OpenClaw was dropped to concentrate polish on the one
// agent we actively support. The `AGENTS` array stays as a single-entry
// table so adding another agent later is just adding an entry — but every
// field is required (no Option soup) since multi-agent flexibility is no
// longer the goal.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

// ── Frontend Events ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ParasiteEvent {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "done")]
    Done {},
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "state")]
    StateChange { state: String },
}

// ── Agent Catalog ──

#[derive(Debug, Clone, Copy)]
pub struct ParasiteAgent {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    /// Args for a fresh conversation (excluding the session arg and the user message).
    pub args_template: &'static [&'static str],
    /// Args when resuming a stored session. `{sessionId}` is substituted at build time.
    pub resume_args_template: &'static [&'static str],
    /// CLI flag for the session id (e.g. `--session-id`).
    pub session_arg: &'static str,
    /// Args to verify the binary is installed (e.g. `--version`).
    pub detect_args: &'static [&'static str],
}

const AGENTS: &[ParasiteAgent] = &[ParasiteAgent {
    id: "claudecode",
    name: "Claude Code",
    command: "claude",
    args_template: &[
        "-p",
        "--dangerously-skip-permissions",
        "--output-format",
        "json",
    ],
    resume_args_template: &[
        "-p",
        "--dangerously-skip-permissions",
        "--output-format",
        "json",
        "--resume",
        "{sessionId}",
    ],
    session_arg: "--session-id",
    detect_args: &["--version"],
}];

pub fn lookup_agent(id: &str) -> Option<ParasiteAgent> {
    AGENTS.iter().find(|a| a.id == id).copied()
}

// ── Session State ──

pub struct ParasiteSession {
    pub session_id: Option<String>,
    pub running: bool,
    pub cancel: CancellationToken,
}

impl ParasiteSession {
    pub fn new() -> Self {
        Self {
            session_id: None,
            running: false,
            cancel: CancellationToken::new(),
        }
    }

    pub fn prepare(&mut self) -> Result<(), String> {
        if self.running {
            return Err("Parasite agent is already processing a request".into());
        }
        if self.cancel.is_cancelled() {
            self.cancel = CancellationToken::new();
        }
        self.running = true;
        Ok(())
    }

    pub fn abort(&mut self) {
        self.cancel.cancel();
    }

    pub fn reset(&mut self) -> Result<(), String> {
        if self.running {
            return Err("Parasite agent is still processing a request".into());
        }
        self.session_id = None;
        Ok(())
    }
}

impl Default for ParasiteSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Keyed by agent and conversation so each keeps its own --resume target.
pub type SharedParasiteSessions = Arc<Mutex<HashMap<String, ParasiteSession>>>;

pub fn create_parasite_sessions() -> SharedParasiteSessions {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn session_key(agent_id: &str, conversation_id: Option<&str>) -> String {
    match conversation_id {
        Some(id) => format!("{}:{}:{}", agent_id.len(), agent_id, id),
        None => agent_id.to_string(),
    }
}

fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".echobird")
        .join("config")
        .join("parasite_sessions")
}

fn session_file_in(directory: &Path, key: &str) -> PathBuf {
    directory.join(format!("{:x}.json", Sha256::digest(key.as_bytes())))
}

pub(crate) fn session_file(key: &str) -> PathBuf {
    session_file_in(&sessions_dir(), key)
}

fn load_session_from_file(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

pub fn load_session_from_disk(key: &str) -> Option<String> {
    load_session_from_file(&session_file_in(&sessions_dir(), key))
}

fn save_session_to_file(path: &Path, session_id: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(session_id).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

pub fn clear_session_from_disk(key: &str) -> Result<(), String> {
    clear_session_file(&session_file_in(&sessions_dir(), key))
}

fn clear_session_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to clear parasite session file: {}", e)),
    }
}

// ── Detection ──

/// Probe each known agent by running `<command> --version` and return the IDs
/// of those that respond successfully. Hides Windows console popups via
/// CREATE_NO_WINDOW so the user never sees a flicker.
pub async fn detect_installed() -> Vec<String> {
    let mut installed = Vec::new();
    for agent in AGENTS {
        if probe(agent, agent_command(agent).await).await {
            installed.push(agent.id.to_string());
        }
    }
    installed
}

async fn agent_command(agent: &ParasiteAgent) -> tokio::process::Command {
    let executable = match crate::services::tool_manager::get_tool_exe_path(agent.id) {
        Some(path) => Some(path),
        None => crate::utils::platform::get_command_path(agent.command).await,
    };
    command_at(agent, executable.as_deref())
}

fn command_at(agent: &ParasiteAgent, executable: Option<&str>) -> tokio::process::Command {
    crate::utils::process::async_command(executable.unwrap_or(agent.command))
}

#[cfg(not(test))]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
// Test fixtures run through cmd.exe beside the rest of the suite.
#[cfg(test)]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

async fn probe(agent: &ParasiteAgent, mut cmd: tokio::process::Command) -> bool {
    cmd.args(agent.detect_args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };

    match tokio::time::timeout(PROBE_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status.success(),
        _ => {
            stop_child(&mut child).await;
            false
        }
    }
}

// ── Sending a Message ──

/// Build the arg list for a turn. Substitutes `{sessionId}` in
/// `resume_args_template` when continuing an existing session; otherwise
/// builds from `args_template` and generates a fresh UUID for `--session-id`
/// so the next turn can `--resume` it. The message travels over stdin so
/// Windows batch launchers never interpret its content as shell arguments.
///
/// The wrapped agent is invoked WITHOUT a `--model` flag — by design,
/// Mother Agent's Connect mode lets the wrapped agent use whatever model
/// is in its own config (set via App Desktop or the agent's own setup).
fn build_args(
    agent: &ParasiteAgent,
    existing_session_id: Option<&str>,
) -> (Vec<String>, Option<String>) {
    let mut args: Vec<String>;
    let effective_session: Option<String>;

    match existing_session_id {
        Some(sid) => {
            args = agent
                .resume_args_template
                .iter()
                .map(|a| a.replace("{sessionId}", sid))
                .collect();
            effective_session = Some(sid.to_string());
        }
        None => {
            args = agent.args_template.iter().map(|s| s.to_string()).collect();
            let new_id = uuid::Uuid::new_v4().to_string();
            args.push(agent.session_arg.to_string());
            args.push(new_id.clone());
            effective_session = Some(new_id);
        }
    }

    (args, effective_session)
}

/// Run a single turn against a parasited agent and stream the result to the
/// frontend via `parasite_event`. Cancellable via `session.cancel`.
pub async fn send_message(
    app: AppHandle,
    sessions: SharedParasiteSessions,
    agent_id: String,
    message: String,
    conversation_id: Option<String>,
    request_id: Option<String>,
) -> Result<(), String> {
    let agent =
        lookup_agent(&agent_id).ok_or_else(|| format!("Unknown parasite agent: {}", agent_id))?;
    let key = session_key(&agent_id, conversation_id.as_deref());

    // The command has already reserved the session; preserve any startup cancellation.
    let (existing_session_id, cancel_token) = {
        let mut map = sessions.lock().await;
        let sess = map.entry(key.clone()).or_insert_with(|| {
            let mut sess = ParasiteSession::new();
            sess.session_id = load_session_from_disk(&key);
            sess
        });
        (sess.session_id.clone(), sess.cancel.clone())
    };

    if cancel_token.is_cancelled() {
        finish_session(&sessions, &key, None).await;
        emit_event(
            &app,
            conversation_id.as_deref(),
            request_id.as_deref(),
            ParasiteEvent::Error {
                message: "Cancelled by user".into(),
            },
        );
        emit_event(
            &app,
            conversation_id.as_deref(),
            request_id.as_deref(),
            ParasiteEvent::Done {},
        );
        return Ok(());
    }

    emit_event(
        &app,
        conversation_id.as_deref(),
        request_id.as_deref(),
        ParasiteEvent::StateChange {
            state: "processing".into(),
        },
    );

    let (args, effective_session_id) = build_args(&agent, existing_session_id.as_deref());

    log::info!(
        "[Parasite] spawn {} {:?}",
        agent.command,
        args.iter()
            .map(|a| safe_truncate(a, 80))
            .collect::<Vec<_>>()
    );

    let mut cmd = agent_command(&agent).await;
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed to spawn {}: {}", agent.command, e);
            log::error!("[Parasite] {}", msg);
            finish_session(&sessions, &key, None).await;
            emit_event(
                &app,
                conversation_id.as_deref(),
                request_id.as_deref(),
                ParasiteEvent::Error { message: msg },
            );
            emit_event(
                &app,
                conversation_id.as_deref(),
                request_id.as_deref(),
                ParasiteEvent::Done {},
            );
            return Ok(());
        }
    };

    let outcome = run_child(child, &message, &cancel_token).await;

    match outcome {
        ParasiteOutcome::Cancelled => {
            finish_session(&sessions, &key, effective_session_id).await;
            emit_event(
                &app,
                conversation_id.as_deref(),
                request_id.as_deref(),
                ParasiteEvent::Error {
                    message: "error.userCancelled".into(),
                },
            );
        }
        ParasiteOutcome::SpawnError(e) => {
            finish_session(&sessions, &key, None).await;
            emit_event(
                &app,
                conversation_id.as_deref(),
                request_id.as_deref(),
                ParasiteEvent::Error { message: e },
            );
        }
        ParasiteOutcome::Completed {
            status,
            stdout,
            stderr,
        } => {
            if !status.success() {
                let msg = if !stderr.trim().is_empty() {
                    safe_truncate(stderr.trim(), 500).to_string()
                } else if !stdout.trim().is_empty() {
                    safe_truncate(stdout.trim(), 500).to_string()
                } else {
                    format!("{} exited with status {}", agent.command, status)
                };
                finish_session(&sessions, &key, effective_session_id).await;
                emit_event(
                    &app,
                    conversation_id.as_deref(),
                    request_id.as_deref(),
                    ParasiteEvent::Error { message: msg },
                );
            } else {
                let parsed = parse_claude_json(&stdout);
                let final_session_id = parsed.session_id.or(effective_session_id);
                // Always remember session_id even on error — the wrapped
                // agent's conversation state is still valid; the next turn
                // (after the user fixes auth, switches model, etc.) should
                // resume rather than start fresh.
                finish_session(&sessions, &key, final_session_id).await;
                if parsed.is_error {
                    // Wrapped agent returned a "successful" envelope that
                    // wraps an actual failure (Claude Code's 403 etc.) —
                    // surface it as a red error bubble instead of pretending
                    // it's a normal assistant reply.
                    let message = if parsed.text.is_empty() {
                        format!("{} returned an error with no message", agent.name)
                    } else {
                        parsed.text
                    };
                    emit_event(
                        &app,
                        conversation_id.as_deref(),
                        request_id.as_deref(),
                        ParasiteEvent::Error { message },
                    );
                } else if !parsed.text.is_empty() {
                    emit_event(
                        &app,
                        conversation_id.as_deref(),
                        request_id.as_deref(),
                        ParasiteEvent::TextDelta { text: parsed.text },
                    );
                }
            }
        }
    }

    emit_event(
        &app,
        conversation_id.as_deref(),
        request_id.as_deref(),
        ParasiteEvent::Done {},
    );
    emit_event(
        &app,
        conversation_id.as_deref(),
        request_id.as_deref(),
        ParasiteEvent::StateChange {
            state: "idle".into(),
        },
    );
    Ok(())
}

enum ParasiteOutcome {
    Cancelled,
    SpawnError(String),
    Completed {
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
}

async fn stop_child(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let (Some(pid), Some(handle)) = (child.id(), child.raw_handle()) {
        let handle = unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(handle) };
        if let Ok((created, exited)) =
            crate::services::process_manager::owned_windows_process_times(&handle)
        {
            let _ = tokio::task::spawn_blocking(move || {
                crate::services::process_manager::stop_owned_windows_tree(pid, created, exited)
            })
            .await;
        }
    }
    let _ = child.kill().await;
}

async fn run_child(
    mut child: tokio::process::Child,
    message: &str,
    cancel: &CancellationToken,
) -> ParasiteOutcome {
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let outcome = tokio::select! {
        biased;
        _ = cancel.cancelled() => ParasiteOutcome::Cancelled,
        result = async {
            let write_input = async {
                if let Some(mut input) = stdin {
                    input.write_all(message.as_bytes()).await?;
                    input.shutdown().await?;
                }
                Ok::<_, std::io::Error>(())
            };
            let read_stdout = async {
                let mut output = String::new();
                if let Some(mut stdout) = stdout {
                    let _ = stdout.read_to_string(&mut output).await;
                }
                output
            };
            let read_stderr = async {
                let mut output = String::new();
                if let Some(mut stderr) = stderr {
                    let _ = stderr.read_to_string(&mut output).await;
                }
                output
            };
            let (input_result, stdout, stderr) = tokio::join!(write_input, read_stdout, read_stderr);
            let status = child.wait().await.map_err(|e| format!("wait error: {e}"))?;
            if status.success() && !parse_claude_json(&stdout).is_error {
                input_result.map_err(|error| format!("CLI input error: {error}"))?;
            }
            Ok::<_, String>((status, stdout, stderr))
        } => match result {
            Ok((status, stdout, stderr)) => ParasiteOutcome::Completed { status, stdout, stderr },
            Err(error) => ParasiteOutcome::SpawnError(error),
        }
    };
    if !matches!(outcome, ParasiteOutcome::Completed { .. }) {
        stop_child(&mut child).await;
    }
    outcome
}

async fn finish_session(sessions: &SharedParasiteSessions, key: &str, session_id: Option<String>) {
    let mut map = sessions.lock().await;
    if let Some(sess) = map.get_mut(key) {
        sess.running = false;
        if session_id.is_some() {
            sess.session_id = session_id;
        }
        if let Some(id) = &sess.session_id {
            if let Err(e) = save_session_to_file(&session_file_in(&sessions_dir(), key), id) {
                log::error!("[ParasiteSession] Failed to save session: {}", e);
            }
        }
    }
}

#[derive(Serialize)]
struct ScopedParasiteEvent<'a> {
    #[serde(flatten)]
    event: ParasiteEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<&'a str>,
}

pub fn emit_event(
    app: &AppHandle,
    conversation_id: Option<&str>,
    request_id: Option<&str>,
    event: ParasiteEvent,
) {
    let _ = app.emit(
        "parasite_event",
        &ScopedParasiteEvent {
            event,
            conversation_id,
            request_id,
        },
    );
}

// ── Output Parsing ──

/// Result of parsing Claude Code's stdout: visible text, optional session id
/// for resume continuity, and whether the agent itself reported the response
/// is an error (distinct from a non-zero exit — process exits 0 but reports a
/// wrapped failure via the JSON envelope).
struct ParsedOutput {
    text: String,
    session_id: Option<String>,
    is_error: bool,
}

/// Claude Code with `--output-format json` emits a single JSON object on
/// completion. The envelope is `{"type":"result", "subtype":"success",
/// "is_error": bool, "result": "...", "session_id": "...", ...}`. Crucially,
/// `subtype` is always `"success"` even for HTTP errors — the truthful
/// failure indicator is `is_error: true` (often accompanied by
/// `api_error_status: 403/429/...`). See
/// memory/reference_claude_code_json_envelope_quirk.md.
fn parse_claude_json(stdout: &str) -> ParsedOutput {
    let trimmed = stdout.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let text = v
            .get("result")
            .and_then(|x| x.as_str())
            .unwrap_or(trimmed)
            .to_string();
        let session_id = v
            .get("session_id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let is_error = v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
        return ParsedOutput {
            text,
            session_id,
            is_error,
        };
    }
    ParsedOutput {
        text: trimmed.to_string(),
        session_id: None,
        is_error: false,
    }
}

// ── Helpers ──

fn safe_truncate(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut end = 0;
    for (i, _) in s.char_indices().take(max_chars) {
        end = i;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    struct CliFixture {
        directory: PathBuf,
        executable: String,
        agent: ParasiteAgent,
    }

    #[cfg(windows)]
    impl CliFixture {
        fn new() -> Self {
            let directory = std::env::temp_dir()
                .join(format!("echobird-parasite-cli-{}", uuid::Uuid::new_v4()));
            let scripts = directory.join("代理 fixture & O'Brien");
            std::fs::create_dir_all(&scripts).unwrap();
            std::fs::write(
                scripts.join("probe.ps1"),
                r#"[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
$mode = $env:ECHOBIRD_PARASITE_FIXTURE_MODE
if ($args[0] -eq '--version') {
    if ($mode -eq 'fail') { exit 7 }
    [Console]::Out.Write('fixture 1.0.0'); exit 0
}
if ($mode -eq 'blocked') {
    [IO.File]::WriteAllText($env:ECHOBIRD_PARASITE_FIXTURE_READY, "$PID")
    Start-Sleep -Seconds 60; exit 0
}
if ($mode -eq 'early-error') { [Console]::Error.Write('fixture configuration failed'); exit 9 }
if ($mode -eq 'json-error') {
    [Console]::Out.Write('{"type":"result","is_error":true,"result":"fixture authentication failed"}'); exit 0
}
if ($mode -eq 'duplex') {
    [Console]::Out.Write(('o' * 131072)); [Console]::Error.Write(('e' * 131072))
}
$inputText = [Console]::In.ReadToEnd()
@{ arguments=@($args); input=$inputText } | ConvertTo-Json -Compress
"#,
            )
            .unwrap();
            let executable = scripts.join("fixture.cmd");
            std::fs::write(
                &executable,
                "@echo off\r\n\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0probe.ps1\" %*\r\n",
            )
            .unwrap();
            Self {
                directory,
                executable: executable.to_str().unwrap().to_string(),
                agent: ParasiteAgent {
                    command: "echobird-missing-agent-7bd96c59",
                    ..AGENTS[0]
                },
            }
        }

        fn turn(&self, args: &[String], mode: &str) -> tokio::process::Child {
            command_at(&self.agent, Some(&self.executable))
                .args(args)
                .env("ECHOBIRD_PARASITE_FIXTURE_MODE", mode)
                .env(
                    "ECHOBIRD_PARASITE_FIXTURE_READY",
                    self.directory.join("ready"),
                )
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap()
        }
    }

    #[cfg(windows)]
    impl Drop for CliFixture {
        fn drop(&mut self) {
            if let (Ok(path), Ok(parent)) = (
                std::fs::canonicalize(&self.directory),
                std::fs::canonicalize(std::env::temp_dir()),
            ) {
                if path.parent() == Some(parent.as_path()) {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn resolved_cli_path_drives_detection_and_preserves_multiline_turns() {
        let fixture = CliFixture::new();
        let error = command_at(&fixture.agent, None)
            .arg("--version")
            .output()
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(
            probe(
                &fixture.agent,
                command_at(&fixture.agent, Some(&fixture.executable))
            )
            .await
        );
        let mut failing = command_at(&fixture.agent, Some(&fixture.executable));
        failing.env("ECHOBIRD_PARASITE_FIXTURE_MODE", "fail");
        assert!(!probe(&fixture.agent, failing).await);
        let missing = fixture.directory.join("missing.cmd");
        assert!(!probe(&fixture.agent, command_at(&fixture.agent, missing.to_str())).await);

        let message =
            "第一行 & | ^ < > %SystemRoot% ! \"quoted\" 'single'\r\n第二行 $(literal) `ticks`";
        for session in [None, Some("fixture-resume-session")] {
            let (args, effective) = build_args(&fixture.agent, session);
            assert!(!args.iter().any(|arg| arg == message || arg == "--model"));
            let outcome = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                run_child(fixture.turn(&args, ""), message, &CancellationToken::new()),
            )
            .await
            .unwrap();
            let ParasiteOutcome::Completed {
                status,
                stdout,
                stderr,
            } = outcome
            else {
                panic!("fixture turn did not complete");
            };
            assert!(status.success(), "{stderr}");
            let output: serde_json::Value = serde_json::from_str(&stdout).unwrap();
            assert_eq!(output["input"], message);
            assert_eq!(output["arguments"], serde_json::json!(args));
            if let Some(session) = session {
                assert_eq!(effective.as_deref(), Some(session));
                assert!(args.contains(&"--resume".to_string()));
            }
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cli_stdin_handles_large_duplex_output_early_errors_and_cancellation() {
        use std::time::Duration;

        let fixture = CliFixture::new();
        let (args, _) = build_args(&fixture.agent, None);
        let message = "multiline & %SystemRoot% \"quoted\"\n".repeat(32768);
        for mode in ["duplex", "early-error", "json-error"] {
            let outcome = tokio::time::timeout(
                Duration::from_secs(60),
                run_child(
                    fixture.turn(&args, mode),
                    &message,
                    &CancellationToken::new(),
                ),
            )
            .await
            .unwrap();
            let ParasiteOutcome::Completed {
                status,
                stdout,
                stderr,
            } = outcome
            else {
                panic!("fixture output was replaced by an input error");
            };
            match mode {
                "duplex" => {
                    assert!(status.success());
                    assert_eq!(stderr, "e".repeat(131072));
                    let output: serde_json::Value =
                        serde_json::from_str(&stdout[131072..]).unwrap();
                    assert_eq!(output["input"], message);
                }
                "early-error" => {
                    assert_eq!(status.code(), Some(9));
                    assert_eq!(stderr, "fixture configuration failed");
                }
                _ => assert!(parse_claude_json(&stdout).is_error),
            }
        }

        let cancel = CancellationToken::new();
        let work = run_child(fixture.turn(&args, "blocked"), &message, &cancel);
        let interrupt = async {
            let ready = fixture.directory.join("ready");
            tokio::time::timeout(Duration::from_secs(10), async {
                while !ready.exists() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            cancel.cancel();
        };
        let (outcome, ()) = tokio::time::timeout(Duration::from_secs(20), async {
            tokio::join!(work, interrupt)
        })
        .await
        .unwrap();
        assert!(matches!(outcome, ParasiteOutcome::Cancelled));
        let child_pid = std::fs::read_to_string(fixture.directory.join("ready")).unwrap();
        let stopped = crate::utils::process::async_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "if(Get-Process -Id $env:ECHOBIRD_PARASITE_CHILD -ErrorAction SilentlyContinue){exit 1}"])
            .env("ECHOBIRD_PARASITE_CHILD", child_pid)
            .status()
            .await
            .unwrap();
        assert!(
            stopped.success(),
            "cancel left the owned batch child running"
        );
    }

    #[test]
    fn clearing_resume_target_handles_missing_files_and_reports_deletion_failure() {
        let directory =
            std::env::temp_dir().join(format!("echobird-parasite-reset-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("session.json");

        clear_session_file(&path).unwrap();
        save_session_to_file(&path, "native-session-a").unwrap();
        clear_session_file(&path).unwrap();
        assert!(load_session_from_file(&path).is_none());

        std::fs::create_dir(&path).unwrap();
        assert!(clear_session_file(&path).is_err());
        assert!(
            path.is_dir(),
            "failed deletion must preserve the saved path"
        );
        std::fs::remove_dir(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn abort_keeps_session_busy_and_reset_preserves_resume_target_until_finalized() {
        let mut session = ParasiteSession::new();
        session.session_id = Some("native-session-a".into());
        session.prepare().unwrap();
        assert!(
            session.prepare().is_err(),
            "accepted run reserves the session immediately"
        );
        session.abort();
        assert!(session.cancel.is_cancelled());
        assert!(
            session.running,
            "cancelled run must remain reserved until it exits"
        );
        assert!(session.reset().is_err());
        assert!(
            session.prepare().is_err(),
            "cancelled run must reject overlapping sends"
        );
        assert!(
            session.cancel.is_cancelled(),
            "rejected send must preserve pending cancellation"
        );
        assert_eq!(session.session_id.as_deref(), Some("native-session-a"));

        session.running = false;
        session.reset().unwrap();
        assert!(session.session_id.is_none());
        session.prepare().unwrap();
        assert!(!session.cancel.is_cancelled());
    }

    #[test]
    fn session_keys_isolate_conversations_and_preserve_legacy_agent_key() {
        assert_eq!(session_key("claudecode", None), "claudecode");
        assert_ne!(
            session_key("claudecode", Some("first")),
            session_key("claudecode", Some("second"))
        );
        assert_ne!(
            session_key("first", Some("same")),
            session_key("second", Some("same"))
        );
        assert_ne!(session_key("a", Some("b:c")), session_key("a:b", Some("c")));
    }

    #[test]
    fn persistence_paths_stay_inside_directory_and_do_not_merge_sanitized_ids() {
        let directory = PathBuf::from("sessions");
        let first = session_file_in(&directory, &session_key("claudecode", Some("../a/b")));
        let second = session_file_in(&directory, &session_key("claudecode", Some("..\\a\\b")));
        assert_eq!(first.parent(), Some(directory.as_path()));
        assert_eq!(second.parent(), Some(directory.as_path()));
        assert_ne!(first, second);
    }

    #[test]
    fn persisted_resume_target_survives_memory_reset_without_crossing_conversations() {
        let directory =
            std::env::temp_dir().join(format!("echobird-parasite-{}", uuid::Uuid::new_v4()));
        let first = session_file_in(&directory, &session_key("claudecode", Some("first")));
        let second = session_file_in(&directory, &session_key("claudecode", Some("second")));
        assert!(load_session_from_file(&first).is_none());
        save_session_to_file(&first, "native-session-a").unwrap();
        save_session_to_file(&second, "native-session-b").unwrap();

        let restored = load_session_from_file(&first).unwrap();
        let agent = lookup_agent("claudecode").unwrap();
        let (args, session_id) = build_args(&agent, Some(&restored));
        assert_eq!(session_id.as_deref(), Some("native-session-a"));
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"native-session-a".to_string()));
        assert_eq!(
            load_session_from_file(&second).as_deref(),
            Some("native-session-b")
        );

        std::fs::remove_file(&first).unwrap();
        assert!(load_session_from_file(&first).is_none());
        assert_eq!(
            load_session_from_file(&second).as_deref(),
            Some("native-session-b")
        );
        std::fs::remove_file(second).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn events_include_scope_without_changing_legacy_shape() {
        let scoped = serde_json::to_value(ScopedParasiteEvent {
            event: ParasiteEvent::TextDelta {
                text: "reply".into(),
            },
            conversation_id: Some("conversation-a"),
            request_id: Some("request-a"),
        })
        .unwrap();
        assert_eq!(scoped["conversation_id"], "conversation-a");
        assert_eq!(scoped["request_id"], "request-a");
        assert_eq!(scoped["text"], "reply");
        let legacy = serde_json::to_value(ScopedParasiteEvent {
            event: ParasiteEvent::Done {},
            conversation_id: None,
            request_id: None,
        })
        .unwrap();
        assert_eq!(legacy, serde_json::json!({"type": "done"}));
    }

    #[test]
    fn claude_args_new_session_generates_session_id() {
        let agent = lookup_agent("claudecode").unwrap();
        let (args, sid) = build_args(&agent, None);
        assert!(sid.is_some(), "fresh session must get a generated id");
        let sid = sid.unwrap();
        assert!(args.contains(&"--session-id".to_string()));
        assert!(args.contains(&sid));
        assert_eq!(args.last(), Some(&sid));
    }

    #[test]
    fn claude_args_resume_uses_existing_session_id() {
        let agent = lookup_agent("claudecode").unwrap();
        let (args, sid) = build_args(&agent, Some("sess-abc"));
        assert_eq!(sid.as_deref(), Some("sess-abc"));
        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"sess-abc".to_string()));
        assert!(!args.iter().any(|a| a.contains("{sessionId}")));
        assert_eq!(args.last().map(|s| s.as_str()), Some("sess-abc"));
    }

    #[test]
    fn claude_args_never_passes_model_flag() {
        // Connect mode intentionally lets Claude Code use its own configured
        // model — no per-turn override — so --model should never appear.
        let agent = lookup_agent("claudecode").unwrap();
        let (args, _) = build_args(&agent, None);
        assert!(!args.contains(&"--model".to_string()));
    }

    #[test]
    fn parse_claude_json_extracts_result_and_session() {
        let raw = r#"{"type":"result","subtype":"success","result":"hello","session_id":"sess-x"}"#;
        let parsed = parse_claude_json(raw);
        assert_eq!(parsed.text, "hello");
        assert_eq!(parsed.session_id.as_deref(), Some("sess-x"));
        assert!(!parsed.is_error);
    }

    #[test]
    fn parse_claude_json_falls_back_on_non_json() {
        let parsed = parse_claude_json("not json at all");
        assert_eq!(parsed.text, "not json at all");
        assert!(parsed.session_id.is_none());
        assert!(!parsed.is_error);
    }

    #[test]
    fn parse_claude_json_flags_is_error_even_when_subtype_is_success() {
        // Claude Code's quirky envelope: subtype="success" means "successfully
        // wrapped a result" but is_error=true means the wrapped result is an
        // upstream failure (e.g. 403 from Anthropic). Both flags can coexist.
        let raw = r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":403,"result":"Your organization has disabled Claude subscription access","session_id":"sess-y"}"#;
        let parsed = parse_claude_json(raw);
        assert!(parsed.is_error);
        assert_eq!(
            parsed.text,
            "Your organization has disabled Claude subscription access"
        );
        assert_eq!(parsed.session_id.as_deref(), Some("sess-y"));
    }

    #[test]
    fn lookup_agent_unknown_returns_none() {
        assert!(lookup_agent("hermes").is_none());
        assert!(lookup_agent("openclaw").is_none());
        assert!(lookup_agent("").is_none());
    }
}
