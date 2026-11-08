use std::future::Future;
use std::sync::{Mutex, OnceLock};
use tauri::Manager;
use tokio_util::sync::CancellationToken;

const PAUSED: &str = "Developer preview is paused";
static GATE: OnceLock<Gate> = OnceLock::new();

#[derive(Default)]
struct Gate(Mutex<Option<CancellationToken>>);

impl Gate {
    fn configure(&self, enabled: bool) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if enabled {
            state.get_or_insert_with(CancellationToken::new);
        } else if let Some(token) = state.take() {
            token.cancel();
        }
    }

    fn token(&self) -> Result<CancellationToken, String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or_else(|| PAUSED.into())
    }

    fn with_enabled<T>(&self, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.is_none() {
            return Err(PAUSED.into());
        }
        work()
    }

    async fn run<T>(&self, work: impl Future<Output = Result<T, String>>) -> Result<T, String> {
        let token = self.token()?;
        tokio::select! {
            biased;
            _ = token.cancelled() => Err(PAUSED.into()),
            result = work => {
                if token.is_cancelled() { Err(PAUSED.into()) } else { result }
            }
        }
    }
}

fn gate() -> &'static Gate {
    GATE.get_or_init(Gate::default)
}

pub fn token() -> Result<CancellationToken, String> {
    gate().token()
}

pub fn enter() -> Result<std::sync::MutexGuard<'static, Option<CancellationToken>>, String> {
    let state = gate().0.lock().unwrap_or_else(|error| error.into_inner());
    if state.is_none() {
        Err(PAUSED.into())
    } else {
        Ok(state)
    }
}

pub fn with_enabled<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    gate().with_enabled(work)
}

pub async fn run<T>(work: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    gate().run(work).await
}

pub fn command_allowed(command: &str) -> bool {
    let developer_command = (command.starts_with("plugin_")
        && !matches!(command, "plugin_cancel" | "plugin_window_control"))
        || matches!(
            command,
            "get_smart_router_statistics"
                | "get_smart_router_statistics_catalog"
                | "get_smart_router_token_credits"
        );
    !developer_command || token().is_ok()
}

pub fn configure(app: &tauri::AppHandle, enabled: bool) {
    gate().configure(enabled);
    if !enabled {
        app.state::<super::plugin_runtime::PluginRuntime>()
            .pause(app);
        app.state::<super::plugin_manager::PluginManager>().pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn paused_ipc_blocks_developer_operations_but_keeps_global_features_and_cleanup_available() {
        for command in [
            "get_smart_router_statistics",
            "get_smart_router_statistics_catalog",
            "plugin_launch",
            "plugin_preview",
            "plugin_install",
            "plugin_registry",
            "plugin_model_request",
        ] {
            assert!(!command_allowed(command), "{command}");
        }
        for command in [
            "get_settings",
            "save_settings",
            "get_models",
            "get_smart_router_config",
            "plugin_cancel",
            "plugin_window_control",
        ] {
            assert!(command_allowed(command), "{command}");
        }
    }

    #[tokio::test]
    async fn default_off_stops_work_and_reenabling_does_not_revive_old_operations() {
        let gate = Gate::default();
        let hits = AtomicUsize::new(0);
        assert!(gate
            .run(async {
                hits.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await
            .is_err());
        assert_eq!(hits.load(Ordering::Relaxed), 0);
        gate.configure(true);
        let first = gate.token().unwrap();
        gate.configure(true);
        assert!(
            !first.is_cancelled(),
            "saving another setting must not cancel work"
        );
        gate.run(async {
            hits.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(hits.load(Ordering::Relaxed), 1, "positive control");
        let work = gate
            .run(async {
                gate.configure(false);
                gate.configure(true);
                Ok(123)
            })
            .await;
        assert!(
            work.is_err(),
            "a late result from the old generation is discarded"
        );
        assert!(first.is_cancelled());
        assert!(!gate.token().unwrap().is_cancelled());
    }

    #[tokio::test]
    async fn disabling_cancels_pending_work_without_polling_the_remaining_operation() {
        let gate = Gate::default();
        gate.configure(true);
        let pending = gate.run(async {
            gate.configure(false);
            std::future::pending::<Result<(), String>>().await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap_err();
        let mut writes = 0;
        assert!(gate
            .with_enabled(|| {
                writes += 1;
                Ok(())
            })
            .is_err());
        assert_eq!(writes, 0);
    }
}
