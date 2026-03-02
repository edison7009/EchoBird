// Application logger �?mirrors old appLogger.ts
// Stores log entries in memory and emits events to frontend

use crate::models::config::{AppLogEntry, LogCategory};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

const MAX_APP_LOGS: usize = 500;

static APP_LOGS: Mutex<Vec<AppLogEntry>> = Mutex::new(Vec::new());

/// Add a log entry and broadcast to frontend
pub fn add_log(app: &AppHandle, category: LogCategory, message: &str) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();
    let entry = AppLogEntry {
        timestamp,
        category,
        message: message.to_string(),
    };

    {
        let mut logs = APP_LOGS.lock().unwrap();
        logs.push(entry.clone());
        if logs.len() > MAX_APP_LOGS {
            logs.remove(0);
        }
    }

    // Emit to all frontend windows
    let _ = app.emit("app-log", &entry);
}

/// Get all stored logs
pub fn get_logs() -> Vec<AppLogEntry> {
    APP_LOGS.lock().unwrap().clone()
}

/// Clear all logs
pub fn clear_logs() {
    APP_LOGS.lock().unwrap().clear();
}
