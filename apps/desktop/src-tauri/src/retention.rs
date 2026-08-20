//! Política local de conservación de grabaciones.

use std::path::Path;

use chrono::{Duration, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::Recording;

use crate::state::AppState;
use atic_core::MutexExt;

#[derive(Clone, Serialize)]
pub struct RetentionItem {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub bytes: u64,
}

#[derive(Clone, Serialize)]
pub struct RetentionPreview {
    pub days: u32,
    pub count: usize,
    pub bytes: u64,
    pub items: Vec<RetentionItem>,
}

#[derive(Serialize)]
pub struct RetentionCleanupResult {
    pub deleted: usize,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn retention_preview(
    state: State<AppState>,
    days: Option<u32>,
) -> Result<RetentionPreview, String> {
    preview(&state, days)
}

#[tauri::command]
pub fn cleanup_retention(
    app: AppHandle,
    state: State<AppState>,
    confirm: bool,
    days: Option<u32>,
) -> Result<RetentionCleanupResult, String> {
    if !confirm {
        return Err(crate::ui_lang::msg(
            "La limpieza requiere confirmación explícita.",
            "Cleanup requires explicit confirmation.",
        ));
    }
    if state.active.lock_or_recover().is_some() || state.dictation.lock_or_recover().is_some() {
        return Err(crate::ui_lang::msg(
            "Termina la grabación o el dictado antes de limpiar datos.",
            "Finish recording or dictation before cleaning data.",
        ));
    }
    let result = cleanup(&state, days)?;
    let _ = app.emit("recordings-changed", ());
    Ok(result)
}

pub fn run_auto_cleanup(app: &AppHandle) {
    let state = app.state::<AppState>();
    let config = state.config.lock_or_recover().clone();
    if !config.retention_auto_cleanup || config.retention_days == 0 {
        return;
    }
    match cleanup(&state, Some(config.retention_days)) {
        Ok(result) => tracing::info!(
            deleted = result.deleted,
            bytes_freed = result.bytes_freed,
            errors = result.errors.len(),
            "limpieza automática de retención completada"
        ),
        Err(error) => tracing::warn!(%error, "falló la limpieza automática de retención"),
    }
}

fn preview(state: &AppState, override_days: Option<u32>) -> Result<RetentionPreview, String> {
    let days = override_days
        .unwrap_or_else(|| state.config.lock_or_recover().retention_days)
        .min(3_650);
    if days == 0 {
        return Ok(RetentionPreview {
            days,
            count: 0,
            bytes: 0,
            items: Vec::new(),
        });
    }
    let cutoff = Utc::now() - Duration::days(i64::from(days));
    let recordings = state
        .db
        .lock_or_recover()
        .list_recordings()
        .map_err(|error| error.to_string())?;
    let mut items = Vec::new();
    let mut bytes = 0u64;
    for recording in recordings {
        if recording.started_at >= cutoff {
            continue;
        }
        let item_bytes = directory_bytes(&state.dirs.recording_dir(&recording.id));
        bytes = bytes.saturating_add(item_bytes);
        items.push(retention_item(recording, item_bytes));
    }
    Ok(RetentionPreview {
        days,
        count: items.len(),
        bytes,
        items,
    })
}

fn cleanup(state: &AppState, override_days: Option<u32>) -> Result<RetentionCleanupResult, String> {
    let preview = preview(state, override_days)?;
    let root = state.dirs.recordings_dir();
    let canonical_root = std::fs::canonicalize(&root).map_err(|error| error.to_string())?;
    let mut result = RetentionCleanupResult {
        deleted: 0,
        bytes_freed: 0,
        errors: Vec::new(),
    };
    for item in preview.items {
        let path = root.join(&item.id);
        if path.exists() {
            match std::fs::canonicalize(&path) {
                Ok(resolved)
                    if resolved.starts_with(&canonical_root)
                        && resolved.parent() == Some(canonical_root.as_path()) =>
                {
                    if let Err(error) = std::fs::remove_dir_all(&resolved) {
                        result.errors.push(format!("{}: {error}", item.title));
                        continue;
                    }
                }
                Ok(_) => {
                    result.errors.push(format!(
                        "{}: ruta fuera del directorio permitido",
                        item.title
                    ));
                    continue;
                }
                Err(error) => {
                    result.errors.push(format!("{}: {error}", item.title));
                    continue;
                }
            }
        }
        match state.db.lock_or_recover().delete_recording(&item.id) {
            Ok(()) => {
                result.deleted += 1;
                result.bytes_freed = result.bytes_freed.saturating_add(item.bytes);
            }
            Err(error) => result.errors.push(format!("{}: {error}", item.title)),
        }
    }
    Ok(result)
}

fn retention_item(recording: Recording, bytes: u64) -> RetentionItem {
    RetentionItem {
        id: recording.id,
        title: recording.title,
        started_at: recording.started_at.to_rfc3339(),
        bytes,
    }
}

fn directory_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_file() => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            Ok(kind) if kind.is_dir() => directory_bytes(&entry.path()),
            _ => 0,
        })
        .fold(0u64, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_nested_files_without_following_unknown_entries() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("resume-retention-test-{nonce}"));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("one.bin"), [0u8; 3]).unwrap();
        std::fs::write(dir.join("nested/two.bin"), [0u8; 5]).unwrap();
        assert_eq!(directory_bytes(&dir), 8);
        std::fs::remove_dir_all(dir).ok();
    }
}
