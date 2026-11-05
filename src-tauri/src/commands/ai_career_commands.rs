//! Tauri command wrappers for the "我的AI生涯" (My AI Career) page.
//! Thin layer over [`crate::services::ai_career`]; the file I/O runs on the
//! blocking thread pool so it never stalls the IPC dispatcher.

use crate::services::ai_career::{self, DailyTokenResponse, Family, HeatmapEntry, SavedSession};

fn ensure_enabled() -> Result<(), String> {
    if super::settings_commands::get_settings().ai_career_enabled == Some(false) {
        return Err("AI career is disabled in Settings".into());
    }
    Ok(())
}

/// One page of a single family's session history, newest first. The frontend
/// requests one family at a time (the selected family card) and pages in more
/// on scroll via `offset`.
#[tauri::command]
pub async fn ai_career_family_history(
    family: String,
    offset: usize,
    limit: usize,
) -> Result<Vec<SavedSession>, String> {
    let fam = Family::from_id(&family).ok_or_else(|| format!("unknown family: {family}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        ensure_enabled()?;
        ai_career::family_history(fam, offset, limit)
    })
    .await
    .map_err(|e| format!("history task join failed: {e}"))?
}

/// Contribution-heatmap entries across all six families (drives the heatmap
/// grid and the five summary stats, which the frontend derives from these).
#[tauri::command]
pub async fn ai_career_heatmap(family: Option<String>) -> Result<Vec<HeatmapEntry>, String> {
    let family = parse_family(family)?;
    tauri::async_runtime::spawn_blocking(move || {
        ensure_enabled()?;
        ai_career::message_heatmap(family)
    })
    .await
    .map_err(|e| format!("heatmap task join failed: {e}"))?
}

/// Total on-disk byte size across all session files — the frontend divides
/// this by a bytes-per-token ratio for the approximate ("≈") cumulative
/// token count.
#[tauri::command]
pub async fn ai_career_token_bytes(family: Option<String>) -> Result<u64, String> {
    let family = parse_family(family)?;
    tauri::async_runtime::spawn_blocking(move || {
        ensure_enabled()?;
        ai_career::estimate_token_bytes(family)
    })
    .await
    .map_err(|e| format!("token estimate task join failed: {e}"))?
}

fn parse_family(family: Option<String>) -> Result<Option<Family>, String> {
    family
        .map(|id| Family::from_id(&id).ok_or_else(|| format!("unknown family: {id}")))
        .transpose()
}

#[tauri::command]
pub async fn ai_career_daily_tokens(family: Option<String>) -> Result<DailyTokenResponse, String> {
    let family = parse_family(family)?;
    tauri::async_runtime::spawn_blocking(move || {
        ensure_enabled()?;
        ai_career::daily_tokens(family)
    })
    .await
    .map_err(|e| format!("daily token task join failed: {e}"))?
}

#[tauri::command]
pub async fn reveal_ai_career_session_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        ai_career::reveal_session_file(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| format!("file manager task join failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_family_preserves_global_requests_and_rejects_unknown_ids() {
        assert_eq!(parse_family(None).unwrap(), None);
        assert_eq!(
            parse_family(Some("claude".to_string())).unwrap(),
            Some(Family::Claude)
        );
        assert!(parse_family(Some("unknown".to_string())).is_err());
    }
}
