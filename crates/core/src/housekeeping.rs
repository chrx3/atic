//! El orden de la casa al arrancar: estados huérfanos y retención de
//! grabaciones. Lo corre el proceso dueño de las grabaciones (la app de Tauri
//! o, con `native_pill`, la pill GPUI), nunca los dos: si uno arranca mientras
//! el otro transcribe, le «repararía» un trabajo que sigue en curso.

use std::path::Path;

use chrono::{Duration, Utc};
use serde::Serialize;

use crate::{AppDirs, Db, Recording, RecordingStatus, Summary, Transcript};

/// Repara estados transitorios huérfanos tras un cierre abrupto.
///
/// Si la app se cerró mientras transcribía o resumía, la fila quedó en
/// `transcribing`/`summarizing` sin ningún hilo que la avance. Al arrancar no
/// hay trabajo en curso, así que degradamos cada fila a un estado consistente
/// con lo que exista en disco para que el usuario pueda reintentar.
pub fn recover_orphaned_statuses(db: &Db, dirs: &AppDirs) {
    let recs = match db.list_recordings() {
        Ok(recs) => recs,
        Err(err) => {
            tracing::warn!(%err, "no se pudieron revisar estados huérfanos al iniciar");
            return;
        }
    };
    for rec in recs {
        let next = match rec.status {
            RecordingStatus::Transcribing => match Transcript::load(&dirs.transcript_path(&rec.id)) {
                Ok(Some(t)) if !t.segments.is_empty() => RecordingStatus::Transcribed,
                _ => RecordingStatus::Recorded,
            },
            RecordingStatus::Summarizing => match Summary::load(&dirs.summary_path(&rec.id)) {
                Ok(Some(_)) => RecordingStatus::Summarized,
                _ => RecordingStatus::Transcribed,
            },
            _ => continue,
        };
        match db.update_status(&rec.id, next) {
            Ok(()) => tracing::info!(
                id = %rec.id, from = ?rec.status, to = ?next,
                "estado huérfano recuperado al iniciar"
            ),
            Err(err) => {
                tracing::warn!(%err, id = %rec.id, "no se pudo recuperar el estado huérfano")
            }
        }
    }
}

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

/// Las grabaciones de más de `days` días (con tope de diez años).
pub fn retention_preview(db: &Db, dirs: &AppDirs, days: u32) -> Result<RetentionPreview, String> {
    let days = days.min(3_650);
    if days == 0 {
        return Ok(RetentionPreview {
            days,
            count: 0,
            bytes: 0,
            items: Vec::new(),
        });
    }
    let cutoff = Utc::now() - Duration::days(i64::from(days));
    let recordings = db.list_recordings().map_err(|error| error.to_string())?;
    let mut items = Vec::new();
    let mut bytes = 0u64;
    for recording in recordings {
        if recording.started_at >= cutoff {
            continue;
        }
        let item_bytes = directory_bytes(&dirs.recording_dir(&recording.id));
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

/// Borra las grabaciones de más de `days` días: su carpeta y su fila.
pub fn retention_cleanup(db: &Db, dirs: &AppDirs, days: u32) -> Result<RetentionCleanupResult, String> {
    let preview = retention_preview(db, dirs, days)?;
    let root = dirs.recordings_dir();
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
        match db.delete_recording(&item.id) {
            Ok(()) => {
                result.deleted += 1;
                result.bytes_freed = result.bytes_freed.saturating_add(item.bytes);
            }
            Err(error) => result.errors.push(format!("{}: {error}", item.title)),
        }
    }
    Ok(result)
}

/// La limpieza automática de `config.json` (`retention_auto_cleanup`).
pub fn run_auto_cleanup(db: &Db, dirs: &AppDirs, config: &crate::Config) {
    if !config.retention_auto_cleanup || config.retention_days == 0 {
        return;
    }
    match retention_cleanup(db, dirs, config.retention_days) {
        Ok(result) => tracing::info!(
            deleted = result.deleted,
            bytes_freed = result.bytes_freed,
            errors = result.errors.len(),
            "limpieza automática de retención completada"
        ),
        Err(error) => tracing::warn!(%error, "falló la limpieza automática de retención"),
    }
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
