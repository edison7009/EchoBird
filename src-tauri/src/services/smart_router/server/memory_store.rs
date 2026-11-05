// Router responsibility; behavior is covered by the server regression suite.
use super::{
    AppState, ResponseMemory, RouteMemory, GROUP_MEMORY, ROUTE_MEMORY, ROUTE_MEMORY_WRITERS,
};
use crate::services::smart_router::profiles;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

pub(super) fn route_memory_version() -> u32 {
    1
}

pub(super) fn route_memory_path() -> PathBuf {
    profiles::root()
        .join("config")
        .join("smart-router-health.json")
}

pub(super) fn response_memory_path() -> PathBuf {
    profiles::root()
        .join("config")
        .join(format!("{}-state.json", profiles::current_id()))
}

pub(super) fn shared_route_memory() -> Arc<Mutex<RouteMemory>> {
    ROUTE_MEMORY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .entry(route_memory_path())
        .or_insert_with(|| {
            let path = route_memory_path();
            let memory = if path.exists() {
                load_route_memory(&path)
            } else {
                let mut merged = RouteMemory::default();
                for id in profiles::ids() {
                    let previous =
                        profiles::with(&id, || load_route_memory(&response_memory_path()));
                    merged.revision = merged.revision.max(previous.revision);
                    for (target, source) in [
                        (&mut merged.candidates, previous.candidates),
                        (&mut merged.accounts, previous.accounts),
                    ] {
                        for (key, health) in source {
                            if target.get(&key).is_none_or(|current| {
                                current.cooldown_until_ms < health.cooldown_until_ms
                            }) {
                                target.insert(key, health);
                            }
                        }
                    }
                }
                merged
            };
            let memory = Arc::new(Mutex::new(memory));
            // Save the migration before any group rewrites its session-only state.
            if !path.exists() {
                persist_memory_at(&path, memory.lock().unwrap());
            }
            memory
        })
        .clone()
}

pub(super) fn shared_response_memory() -> Arc<Mutex<ResponseMemory>> {
    GROUP_MEMORY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .entry(response_memory_path())
        .or_insert_with(|| {
            let memory = fs::read(response_memory_path())
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            Arc::new(Mutex::new(memory))
        })
        .clone()
}

pub(super) fn load_route_memory(path: &Path) -> RouteMemory {
    if !path.exists() {
        return RouteMemory::default();
    }
    fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|content| serde_json::from_str(&content).map_err(|e| e.to_string()))
        .unwrap_or_else(|error| {
            log::warn!(
                "[SmartRouter] Ignoring invalid route memory {}: {error}",
                path.display()
            );
            RouteMemory::default()
        })
}

// Allocate the sequence while holding route_memory, then release it before
// serialization or disk access. A later snapshot cannot be overwritten by an
// earlier caller that was descheduled before reaching the writer.
pub(super) fn persist_memory_at<T: Clone + Serialize>(
    path: &Path,
    memory: std::sync::MutexGuard<'_, T>,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let snapshot = memory.clone();
    drop(memory);
    write_memory_snapshot(path, sequence, &snapshot);
}

pub(super) fn write_memory_snapshot(path: &Path, sequence: u64, memory: &impl Serialize) {
    let Ok(mut written) = ROUTE_MEMORY_WRITERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    else {
        return;
    };
    if written.get(path).is_some_and(|latest| *latest >= sequence) {
        return;
    }
    let result = serde_json::to_vec_pretty(memory)
        .map_err(|error| error.to_string())
        .and_then(|content| {
            crate::services::native_account_files::write_private_file(path, &content)
        });
    match result {
        Ok(()) => {
            written.insert(path.to_owned(), sequence);
        }
        Err(error) => log::warn!(
            "[SmartRouter] Failed to save route memory {}: {error}",
            path.display()
        ),
    }
}

pub(super) fn persist_route_memory(
    state: &AppState,
    memory: std::sync::MutexGuard<'_, RouteMemory>,
) {
    if let Some(path) = &state.route_memory_path {
        persist_memory_at(path, memory);
    }
}

pub(super) fn persist_response_memory(
    state: &AppState,
    memory: std::sync::MutexGuard<'_, ResponseMemory>,
) {
    if let Some(path) = &state.response_memory_path {
        persist_memory_at(path, memory);
    }
}
