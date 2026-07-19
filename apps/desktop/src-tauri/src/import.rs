//! Importación de archivos de audio externos como grabaciones.

use std::path::Path;

use chrono::Utc;
use tauri::{AppHandle, Emitter, State};

use atic_core::{Recording, RecordingStatus};

use crate::state::AppState;

/// Importa uno o más archivos de audio (WAV/MP3/M4A) como grabaciones de una pista.
///
/// Cada archivo se convierte a `mic.wav` mono 16 kHz en una carpeta nueva y se
/// persiste con estado `recorded` para poder transcribir/resumir con el flujo batch.
#[tauri::command]
pub fn import_audio(
    app: AppHandle,
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Vec<Recording>, String> {
    if paths.is_empty() {
        return Err("No se seleccionó ningún archivo.".into());
    }

    let mut imported = Vec::with_capacity(paths.len());

    for path_str in paths {
        let src = Path::new(&path_str);
        if !src.is_file() {
            return Err(format!("Archivo no encontrado: {path_str}"));
        }

        let mut rec = Recording::new(Utc::now());
        if let Some(stem) = src.file_stem().and_then(|s| s.to_str()) {
            let title = stem.trim();
            if !title.is_empty() {
                rec.title = title.to_string();
            }
        }

        let dir = state.dirs.recording_dir(&rec.id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mic_wav = dir.join("mic.wav");

        match atic_transcribe::import_audio_to_wav(src, &mic_wav) {
            Ok(duration_secs) => {
                rec.duration_secs = duration_secs.max(0);
                rec.mic_path = Some("mic.wav".into());
                rec.system_path = None;
                rec.status = RecordingStatus::Recorded;

                if let Err(err) = state.db.lock().unwrap().insert_recording(&rec) {
                    let _ = std::fs::remove_dir_all(&dir);
                    return Err(err.to_string());
                }
                imported.push(rec);
            }
            Err(err) => {
                let _ = std::fs::remove_dir_all(&dir);
                if !imported.is_empty() {
                    let _ = app.emit("recordings-changed", ());
                }
                let name = src
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path_str);
                return Err(format!("No se pudo importar «{name}»: {err}"));
            }
        }
    }

    let _ = app.emit("recordings-changed", ());
    Ok(imported)
}
