pub(crate) mod native;
mod parse;
pub(crate) mod saved;
mod sources;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::ipc::Channel;

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub completed: usize,
    pub total: usize,
    pub source: String,
    pub models: usize,
    pub accounts: usize,
    pub duplicates: usize,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub source: String,
    pub reason: String,
    pub count: usize,
}

pub(crate) fn active() -> bool {
    RUNNING.load(Ordering::Acquire)
}

struct RunningGuard;
impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

pub async fn run(channel: Channel<Progress>) -> Result<Progress, String> {
    RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Import already running")?;
    let _guard = RunningGuard;
    if super::config_backup::active() {
        return Err("Import already running".into());
    }
    let sources = sources::discover()?;
    let mut progress = Progress {
        total: sources.len() + native::SOURCES.len(),
        ..Default::default()
    };
    for source in sources {
        progress.source = source.name.to_owned();
        let _ = channel.send(progress.clone());
        let name = source.name;
        // No credentials, paths, or parser error excerpts cross the IPC boundary.
        match tauri::async_runtime::spawn_blocking(move || sources::read(&source)).await {
            Ok(Ok(read)) => {
                for _ in 0..read.failures {
                    issue(&mut progress, name, "readFailed");
                }
                for document in read.documents {
                    let parsed = parse::parse(name, &document);
                    for model in parsed.models {
                        match super::model_manager::import_model(model) {
                            Ok(true) => progress.models += 1,
                            Ok(false) => progress.duplicates += 1,
                            Err(_) => issue(&mut progress, name, "saveFailed"),
                        }
                    }
                    for account in parsed.accounts {
                        let result = match account {
                            parse::Account::Codex(auth) => {
                                super::codex_accounts::import_snapshot(&auth)
                            }
                            parse::Account::Claude(credentials, config) => {
                                super::claude_code_accounts::import_snapshot(&credentials, &config)
                                    .await
                            }
                            parse::Account::Antigravity(raw) => {
                                super::antigravity_accounts::import_snapshot(&raw).await
                            }
                        };
                        match result {
                            Ok(true) => progress.accounts += 1,
                            Ok(false) => progress.duplicates += 1,
                            Err(error) => {
                                let reason = if [
                                    "accountError.write",
                                    "accountError.read",
                                    "accountError.keychain",
                                ]
                                .iter()
                                .any(|code| error.starts_with(code))
                                {
                                    "saveFailed"
                                } else {
                                    "loginRequired"
                                };
                                issue(&mut progress, name, reason);
                            }
                        }
                    }
                    for reason in parsed.issues {
                        issue(&mut progress, name, reason);
                    }
                }
            }
            _ => issue(&mut progress, name, "readFailed"),
        }
        progress.completed += 1;
        let _ = channel.send(progress.clone());
    }
    let discovery_started = std::time::Instant::now();
    super::tool_manager::with_account_import_discovery(async {
        log::info!(
            "[auto-import] stage=discovery elapsed_ms={}",
            discovery_started.elapsed().as_millis()
        );
        for &(id, tools) in native::SOURCES {
            progress.source = id.into();
            let _ = channel.send(progress.clone());
            // An absent client is not a failed source, and its configuration is not read.
            let started = std::time::Instant::now();
            let installed = super::tool_manager::any_tool_installed(tools);
            log::info!(
                "[auto-import] source={id} stage=detect installed={installed} elapsed_ms={}",
                started.elapsed().as_millis()
            );
            let started = std::time::Instant::now();
            let counts = native::scan_installed(installed, || native::scan(id)).await;
            log::info!(
                "[auto-import] source={id} stage=accounts elapsed_ms={}",
                started.elapsed().as_millis()
            );
            progress.accounts += counts.added;
            progress.duplicates += counts.duplicates;
            for _ in 0..counts.skipped {
                issue(&mut progress, id, "readFailed");
            }
            progress.completed += 1;
            let _ = channel.send(progress.clone());
        }
    })
    .await?;
    Ok(progress)
}

fn issue(progress: &mut Progress, source: &str, reason: &str) {
    if let Some(issue) = progress
        .issues
        .iter_mut()
        .find(|issue| issue.source == source && issue.reason == reason)
    {
        issue.count += 1;
        return;
    }
    progress.issues.push(Issue {
        source: source.to_owned(),
        reason: reason.to_owned(),
        count: 1,
    });
}
