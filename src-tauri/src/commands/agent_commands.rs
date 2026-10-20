// Agent Commands — Tauri IPC commands for the MotherAgent frontend

use crate::commands::ssh_commands::SSHPool;
use crate::services::agent_loop::{self, AgentEvent, AgentRequest, SharedSessionMap};
use crate::services::agent_output_store::OutputStore;
use crate::services::parasite::{self, SharedParasiteSessions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

/// Send a message to the agent. The agent will process asynchronously
/// and emit `agent_event` events to the frontend.
#[tauri::command]
pub async fn agent_send_message(
    app: AppHandle,
    session_map: State<'_, SharedSessionMap>,
    ssh_pool: State<'_, SSHPool>,
    request: AgentRequest,
) -> Result<String, String> {
    let server_key = request.session_key();

    let map_clone = session_map.inner().clone();
    let pool_clone = ssh_pool.inner().clone();

    // Reserve the session before spawning so abort/reset also see pending startup.
    let session_id = {
        let mut map = session_map.lock().await;
        if !map.contains_key(&server_key) {
            map.insert(
                server_key.clone(),
                agent_loop::AgentSession::restore(&server_key)?,
            );
        }
        let sess = map.get_mut(&server_key).unwrap();
        sess.prepare_run()?;
        sess.id.clone()
    };

    // Spawn agent as background task
    let app_clone = app.clone();
    let conversation_id = request.conversation_id.clone();
    let request_id = request.request_id.clone();
    tokio::spawn(async move {
        if let Err(e) = agent_loop::run_agent(app, request, map_clone.clone(), pool_clone).await {
            log::error!("[AgentCommand] Agent error: {}", e);
            if let Some(sess) = map_clone.lock().await.get_mut(&server_key) {
                sess.running = false;
            }
            // Safety net: ensure frontend is never left stuck in isProcessing
            agent_loop::emit_event(
                &app_clone,
                conversation_id.as_deref(),
                request_id.as_deref(),
                AgentEvent::Error { message: e },
            );
            agent_loop::emit_event(
                &app_clone,
                conversation_id.as_deref(),
                request_id.as_deref(),
                AgentEvent::Done {},
            );
        }
    });

    Ok(session_id)
}

/// Abort the current agent execution
#[tauri::command]
pub async fn agent_abort(
    session_map: State<'_, SharedSessionMap>,
    server_key: String,
) -> Result<bool, String> {
    let mut map = session_map.lock().await;
    if let Some(sess) = map.get_mut(&server_key) {
        if sess.running {
            sess.cancel();
            log::info!("[AgentCommand] Agent aborted for server: {}", server_key);
            return Ok(true);
        }
    }
    Ok(false)
}

/// Reset the agent session for a specific server (clear conversation history)
#[tauri::command]
pub async fn agent_reset(
    session_map: State<'_, SharedSessionMap>,
    server_key: String,
) -> Result<String, String> {
    let mut map = session_map.lock().await;
    if map.get(&server_key).is_some_and(|sess| sess.running) {
        return Err("Agent is still processing a request".into());
    }
    // Keep the live history intact if removing its persisted copy fails.
    agent_loop::clear_session_from_disk(&server_key)?;
    let sess = map
        .entry(server_key.clone())
        .or_insert_with(agent_loop::AgentSession::new);
    sess.reset()?;
    log::info!(
        "[AgentCommand] Session reset for server {}: {}",
        server_key,
        sess.id
    );
    Ok(sess.id.clone())
}

#[tauri::command]
pub async fn agent_reset_conversation_context(
    session_map: State<'_, SharedSessionMap>,
    parasite_sessions: State<'_, SharedParasiteSessions>,
    server_key: String,
    include_parasite: bool,
    parasite_conversation_id: Option<String>,
) -> Result<String, String> {
    let mut agents = session_map.lock().await;
    let mut parasites = parasite_sessions.lock().await;
    let parasite_key = include_parasite
        .then(|| parasite::session_key("claudecode", parasite_conversation_id.as_deref()));
    let parasite_file = parasite_key.as_deref().map(parasite::session_file);
    reset_conversation_context(
        &mut agents,
        &mut parasites,
        &server_key,
        &agent_loop::session_file(&server_key),
        parasite_key.as_deref().zip(parasite_file.as_deref()),
        &OutputStore::standard(),
    )
}

struct StagedSessionFile {
    original: PathBuf,
    backup: PathBuf,
}

struct HistoryLock(std::fs::File);

impl Drop for HistoryLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

fn lock_history_file(path: &Path) -> Result<Option<HistoryLock>, String> {
    let path = path.with_extension("writer.lock");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("Saved context lock cannot use a linked file".into());
                }
            }
        }
        Ok(_) => return Err("Saved context lock is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot inspect saved context lock: {error}")),
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("Cannot open saved context lock: {error}"))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|error| format!("Conversation storage is busy: {error}"))?;
    Ok(Some(HistoryLock(file)))
}

fn stage_session_file(path: &Path) -> Result<Option<StagedSessionFile>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot read saved context: {}", e)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Saved context is not a file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Saved context cannot use a linked file".into());
        }
    }
    #[cfg(windows)]
    if metadata.permissions().readonly() {
        return Err("Saved context file is read-only".into());
    }
    let backup = path.with_extension(format!("{}.pending-clear", uuid::Uuid::new_v4()));
    std::fs::rename(path, &backup).map_err(|e| format!("Cannot stage saved context: {}", e))?;
    Ok(Some(StagedSessionFile {
        original: path.to_path_buf(),
        backup,
    }))
}

fn reset_conversation_context(
    agents: &mut HashMap<String, agent_loop::AgentSession>,
    parasites: &mut HashMap<String, parasite::ParasiteSession>,
    server_key: &str,
    agent_file: &Path,
    parasite_context: Option<(&str, &Path)>,
    outputs: &OutputStore,
) -> Result<String, String> {
    if agents
        .get(server_key)
        .is_some_and(|session| session.running)
    {
        return Err("Agent is still processing a request".into());
    }
    if parasite_context
        .is_some_and(|(key, _)| parasites.get(key).is_some_and(|session| session.running))
    {
        return Err("Parasite agent is still processing a request".into());
    }
    let history_lock = lock_history_file(agent_file)?;
    let history_backup = agent_file.with_extension("backup.json");
    let mut staged_files: Vec<StagedSessionFile> = Vec::new();
    for path in std::iter::once(agent_file)
        .chain(std::iter::once(history_backup.as_path()))
        .chain(parasite_context.map(|(_, path)| path))
    {
        match stage_session_file(path) {
            Ok(Some(staged)) => staged_files.push(staged),
            Ok(None) => {}
            Err(error) => {
                for staged in staged_files.into_iter().rev() {
                    std::fs::rename(&staged.backup, &staged.original).map_err(|restore| {
                        format!(
                            "{error}; context restoration failed: {restore} (retained at {})",
                            staged.backup.display()
                        )
                    })?;
                }
                return Err(error);
            }
        }
    }
    let session = agent_loop::AgentSession::new();
    let id = session.id.clone();
    agents.insert(server_key.to_string(), session);
    if let Some((key, _)) = parasite_context {
        if let Some(session) = parasites.get_mut(key) {
            session.session_id = None;
        }
    }
    // Both contexts are committed; a leftover backup must not restore the UI history.
    for staged in staged_files {
        if let Err(e) = std::fs::remove_file(&staged.backup) {
            log::warn!(
                "Failed to remove cleared context backup {}: {}",
                staged.backup.display(),
                e
            );
        }
    }
    if history_lock.is_some() {
        drop(history_lock);
        if let Err(error) = std::fs::remove_file(agent_file.with_extension("writer.lock")) {
            log::warn!("Failed to remove cleared conversation lock: {error}");
        }
    }
    if let Err(error) = outputs.clear_scope(server_key) {
        log::warn!("[AgentSession] Failed to clear saved output: {error}");
    }
    Ok(id)
}

#[cfg(test)]
mod conversation_context_tests {
    use super::*;
    use crate::services::agent_history::{HistorySnapshot, HistoryStore, HistorySummary};
    use crate::services::agent_output_store::OutputRecord;
    use crate::services::llm_client::{Message, MessageContent};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("echobird-context-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    fn output_record() -> OutputRecord {
        OutputRecord {
            command: String::new(),
            target: "local".into(),
            exit_code: Some(0),
            success: true,
            stdout: "saved original evidence".into(),
            stderr: String::new(),
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn contexts() -> (
        HashMap<String, agent_loop::AgentSession>,
        HashMap<String, parasite::ParasiteSession>,
    ) {
        let mut agent = agent_loop::AgentSession::new();
        agent.messages.push(Message {
            role: "user".into(),
            content: MessageContent::Text("remember this context".into()),
        });
        let mut parasite = parasite::ParasiteSession::new();
        parasite.session_id = Some("existing-cli-session".into());
        (
            HashMap::from([("conversation".into(), agent)]),
            HashMap::from([("cli".into(), parasite)]),
        )
    }

    #[test]
    fn second_file_failure_restores_disk_and_keeps_both_contexts_usable() {
        let fixture = Fixture::new();
        let agent_file = fixture.0.join("agent.json");
        let parasite_file = fixture.0.join("parasite.json");
        let (mut agents, mut parasites) = contexts();
        let original_id = agents["conversation"].id.clone();
        let bytes = serde_json::to_vec(&agents["conversation"].messages).unwrap();
        std::fs::write(&agent_file, &bytes).unwrap();
        std::fs::create_dir(&parasite_file).unwrap();
        let outputs = OutputStore::new(fixture.0.join("outputs"));
        let evidence = outputs.store("conversation", &output_record()).unwrap();

        assert!(reset_conversation_context(
            &mut agents,
            &mut parasites,
            "conversation",
            &agent_file,
            Some(("cli", &parasite_file)),
            &outputs,
        )
        .is_err());
        assert_eq!(std::fs::read(&agent_file).unwrap(), bytes);
        assert!(parasite_file.is_dir());
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 3);
        assert!(outputs.read("conversation", &evidence, 0, 6000).is_ok());
        assert_eq!(agents["conversation"].id, original_id);
        assert_eq!(agents["conversation"].messages.len(), 1);
        assert_eq!(
            parasites["cli"].session_id.as_deref(),
            Some("existing-cli-session")
        );
        agents
            .get_mut("conversation")
            .unwrap()
            .prepare_run()
            .unwrap();
        parasites.get_mut("cli").unwrap().prepare().unwrap();
        assert_eq!(agents["conversation"].messages.len(), 1);
        assert_eq!(
            parasites["cli"].session_id.as_deref(),
            Some("existing-cli-session")
        );
        let persisted: Vec<Message> =
            serde_json::from_slice(&std::fs::read(agent_file).unwrap()).unwrap();
        assert_eq!(persisted.len(), 1);
    }

    #[test]
    fn success_clears_both_files_and_both_memories() {
        let fixture = Fixture::new();
        let agent_file = fixture.0.join("agent.json");
        let parasite_file = fixture.0.join("parasite.json");
        std::fs::write(&agent_file, b"history").unwrap();
        std::fs::write(&parasite_file, b"resume-id").unwrap();
        let (mut agents, mut parasites) = contexts();
        let old_id = agents["conversation"].id.clone();
        let outputs = OutputStore::new(fixture.0.join("outputs"));
        let evidence = outputs.store("conversation", &output_record()).unwrap();
        let other = outputs.store("other", &output_record()).unwrap();
        let id = reset_conversation_context(
            &mut agents,
            &mut parasites,
            "conversation",
            &agent_file,
            Some(("cli", &parasite_file)),
            &outputs,
        )
        .unwrap();
        assert_ne!(id, old_id);
        assert!(agents["conversation"].messages.is_empty());
        assert!(parasites["cli"].session_id.is_none());
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
        assert!(outputs.read("conversation", &evidence, 0, 6000).is_err());
        assert!(outputs.read("other", &other, 0, 6000).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn readonly_second_file_preserves_both_persisted_contexts() {
        let fixture = Fixture::new();
        let agent_file = fixture.0.join("agent.json");
        let parasite_file = fixture.0.join("parasite.json");
        std::fs::write(&agent_file, b"agent history").unwrap();
        std::fs::write(&parasite_file, b"cli resume").unwrap();
        let mut permissions = std::fs::metadata(&parasite_file).unwrap().permissions();
        let original_permissions = permissions.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(&parasite_file, permissions.clone()).unwrap();
        let (mut agents, mut parasites) = contexts();
        let result = reset_conversation_context(
            &mut agents,
            &mut parasites,
            "conversation",
            &agent_file,
            Some(("cli", &parasite_file)),
            &OutputStore::new(fixture.0.join("outputs")),
        );
        std::fs::set_permissions(&parasite_file, original_permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(agent_file).unwrap(), b"agent history");
        assert_eq!(std::fs::read(parasite_file).unwrap(), b"cli resume");
        assert_eq!(agents["conversation"].messages.len(), 1);
        assert_eq!(
            parasites["cli"].session_id.as_deref(),
            Some("existing-cli-session")
        );
    }

    #[test]
    fn busy_second_engine_keeps_first_file_and_history_intact() {
        let fixture = Fixture::new();
        let agent_file = fixture.0.join("agent.json");
        std::fs::write(&agent_file, b"history").unwrap();
        let (mut agents, mut parasites) = contexts();
        parasites.get_mut("cli").unwrap().prepare().unwrap();
        assert!(reset_conversation_context(
            &mut agents,
            &mut parasites,
            "conversation",
            &agent_file,
            Some(("cli", &fixture.0.join("parasite.json"))),
            &OutputStore::new(fixture.0.join("outputs")),
        )
        .is_err());
        assert_eq!(std::fs::read(agent_file).unwrap(), b"history");
        assert_eq!(agents["conversation"].messages.len(), 1);
        assert_eq!(
            parasites["cli"].session_id.as_deref(),
            Some("existing-cli-session")
        );
    }

    #[test]
    fn agent_only_reset_leaves_shared_legacy_cli_context_intact() {
        let fixture = Fixture::new();
        let (mut agents, mut parasites) = contexts();
        reset_conversation_context(
            &mut agents,
            &mut parasites,
            "conversation",
            &fixture.0.join("missing.json"),
            None,
            &OutputStore::new(fixture.0.join("outputs")),
        )
        .unwrap();
        assert!(agents["conversation"].messages.is_empty());
        assert_eq!(
            parasites["cli"].session_id.as_deref(),
            Some("existing-cli-session")
        );
    }

    #[test]
    fn output_cleanup_failure_does_not_restore_committed_contexts() {
        let fixture = Fixture::new();
        let agent_file = fixture.0.join("agent.json");
        let parasite_file = fixture.0.join("parasite.json");
        std::fs::write(&agent_file, b"history").unwrap();
        std::fs::write(&parasite_file, b"resume-id").unwrap();
        let blocked = fixture.0.join("outputs");
        std::fs::write(&blocked, b"existing file").unwrap();
        let (mut agents, mut parasites) = contexts();
        reset_conversation_context(
            &mut agents,
            &mut parasites,
            "conversation",
            &agent_file,
            Some(("cli", &parasite_file)),
            &OutputStore::new(blocked),
        )
        .unwrap();
        assert!(!agent_file.exists());
        assert!(!parasite_file.exists());
        assert!(agents["conversation"].messages.is_empty());
        assert!(parasites["cli"].session_id.is_none());
    }

    #[test]
    fn history_primary_and_backup_commit_or_restore_with_the_parasite_context() {
        for fail in [true, false] {
            let fixture = Fixture::new();
            let histories = HistoryStore::new(fixture.0.join("histories"));
            let (mut agents, mut parasites) = contexts();
            let snapshot = HistorySnapshot {
                search_session_id: None,
                messages: agents["conversation"].messages.clone(),
                summary: Some(HistorySummary {
                    through: 0,
                    text: "verified previous task".into(),
                }),
            };
            histories.save("conversation", &snapshot).unwrap();
            let agent_file = fixture.0.join("histories/conversation.json");
            let backup_file = agent_file.with_extension("backup.json");
            let original = std::fs::read(&agent_file).unwrap();
            let backup = std::fs::read(&backup_file).unwrap();
            assert_eq!(histories.load("conversation").unwrap().messages.len(), 1);
            let parasite_file = fixture.0.join("parasite.json");
            if fail {
                std::fs::create_dir(&parasite_file).unwrap();
            } else {
                std::fs::write(&parasite_file, b"existing resume").unwrap();
            }
            let outputs = OutputStore::new(fixture.0.join("outputs"));
            let evidence = outputs.store("conversation", &output_record()).unwrap();
            let result = reset_conversation_context(
                &mut agents,
                &mut parasites,
                "conversation",
                &agent_file,
                Some(("cli", &parasite_file)),
                &outputs,
            );
            if fail {
                assert!(result.is_err());
                assert_eq!(std::fs::read(&agent_file).unwrap(), original);
                assert_eq!(std::fs::read(&backup_file).unwrap(), backup);
                assert_eq!(
                    std::fs::read_dir(fixture.0.join("histories"))
                        .unwrap()
                        .count(),
                    3
                );
                assert_eq!(histories.load("conversation").unwrap().messages.len(), 1);
                assert!(outputs.read("conversation", &evidence, 0, 6000).is_ok());
                assert_eq!(agents["conversation"].messages.len(), 1);
                assert_eq!(
                    parasites["cli"].session_id.as_deref(),
                    Some("existing-cli-session")
                );
            } else {
                result.unwrap();
                assert!(!agent_file.exists());
                assert!(!backup_file.exists());
                assert!(!agent_file.with_extension("writer.lock").exists());
                let restored = histories.load("conversation").unwrap();
                assert!(restored.messages.is_empty());
                assert!(restored.summary.is_none());
                assert!(outputs.read("conversation", &evidence, 0, 6000).is_err());
                assert!(agents["conversation"].messages.is_empty());
                assert!(parasites["cli"].session_id.is_none());
            }
        }
    }
}
