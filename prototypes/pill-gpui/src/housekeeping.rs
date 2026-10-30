//! Lo que Atic hace al arrancar y, con `native_pill`, le toca a la pill:
//! reparar grabaciones que quedaron a medio transcribir o resumir, y borrar
//! las grabaciones y capturas viejas según Ajustes. En un hilo aparte: toca
//! el disco y la base.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

use atic_core::{housekeeping, AppDirs, Config, Db, Recording, RecordingStatus};
use chrono::{DateTime, Local, Utc};

use crate::i18n::{t, tf};

pub fn spawn() {
    let started = std::thread::Builder::new()
        .name("orden-al-arrancar".into())
        .spawn(run);
    if let Err(error) = started {
        tracing::warn!(%error, "no arrancó el orden al iniciar");
    }
}

fn run() {
    let Ok(dirs) = AppDirs::new() else {
        return;
    };
    let config = Config::load(&dirs.config_path());
    // Sin la pill nativa, el dueño de las grabaciones es la app de Tauri.
    if !config.native_pill {
        return;
    }
    match Db::open(&dirs.db_path()) {
        Ok(db) => {
            recover_cut_recordings(&db, &dirs);
            housekeeping::recover_orphaned_statuses(&db, &dirs);
            housekeeping::run_auto_cleanup(&db, &dirs, &config);
        }
        Err(error) => tracing::warn!(%error, "no se abrió la base para el orden al iniciar"),
    }
    let result = atic_capture::retention::cleanup_captures(
        &dirs.captures_dir(),
        config.capture_retention_hours,
        std::time::SystemTime::now(),
    );
    if result.deleted > 0 || !result.errors.is_empty() {
        tracing::info!(
            deleted = result.deleted,
            errors = result.errors.len(),
            "limpieza automática de capturas"
        );
    }
}

/// Lo que se escribió hace menos que esto puede ser de un trabajo en curso
/// (una reunión del celular que se importa al arrancar), no de una caída.
const SETTLED: Duration = Duration::from_secs(120);

/// Las grabaciones que la app no alcanzó a cerrar: la fila en la base se
/// escribe al detener, así que si la pill se cae grabando queda la carpeta con
/// el audio y nada en Reuniones. Se les corrige el encabezado del WAV y entran
/// a la lista, para transcribirlas como cualquier otra.
fn recover_cut_recordings(db: &Db, dirs: &AppDirs) {
    let known: HashSet<String> = match db.list_recordings() {
        Ok(recs) => recs.into_iter().map(|r| r.id).collect(),
        Err(error) => {
            tracing::warn!(%error, "no se pudieron revisar las grabaciones cortadas");
            return;
        }
    };
    let Ok(entries) = std::fs::read_dir(dirs.recordings_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Some(id) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        if known.contains(&id) || !dir.is_dir() {
            continue;
        }
        let Some(rec) = cut_recording(&dir, id) else {
            continue;
        };
        match db.insert_recording(&rec) {
            Ok(()) => tracing::info!(id = %rec.id, secs = rec.duration_secs, "grabación cortada recuperada al iniciar"),
            Err(error) => tracing::warn!(%error, id = %rec.id, "no se pudo recuperar la grabación cortada"),
        }
    }
}

/// La fila de una carpeta sin fila, si tiene alguna pista con audio.
fn cut_recording(dir: &Path, id: String) -> Option<Recording> {
    let mut secs = 0.0f64;
    let mut created: Option<SystemTime> = None;
    let mut track = |name: &str| -> bool {
        let path = dir.join(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        let recent = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_none_or(|age| age < SETTLED);
        if recent {
            return false;
        }
        if let Err(error) = atic_audio::repair_wav_header(&path) {
            tracing::warn!(%error, path = %path.display(), "no se pudo reparar el WAV cortado");
            return false;
        }
        let Ok(reader) = hound::WavReader::open(&path) else {
            return false;
        };
        let spec = reader.spec();
        let frames = f64::from(reader.duration());
        if frames == 0.0 || spec.sample_rate == 0 {
            return false;
        }
        secs = secs.max(frames / f64::from(spec.sample_rate));
        // Se crea al empezar a grabar: es la hora de inicio.
        let born = meta.created().ok().or_else(|| meta.modified().ok().map(|m| m - Duration::from_secs_f64(frames / f64::from(spec.sample_rate))));
        created = match (created, born) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        true
    };
    let mic = track("mic.wav");
    let system = track("system.wav");
    if !mic && !system {
        return None;
    }
    let started_at: DateTime<Utc> = created.map(DateTime::from).unwrap_or_else(Utc::now);
    let when = started_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
    Some(Recording {
        id,
        title: format!("{} {}", tf("pill.meetings.recorder.defaultTitle", &[("when", &when)]), t("pill.meetings.recoveredTag")),
        started_at,
        duration_secs: secs.round() as i64,
        mic_path: mic.then(|| "mic.wav".into()),
        system_path: system.then(|| "system.wav".into()),
        status: RecordingStatus::Recorded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &Path, frames: usize) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..frames {
            writer.write_sample(0.1f32).unwrap();
        }
        writer.finalize().unwrap();
        // Como si la app se hubiera caído: el encabezado dice largo 0.
        let mut bytes = std::fs::read(path).unwrap();
        let data = bytes.windows(4).position(|w| w == b"data").unwrap();
        bytes[data + 4..data + 8].copy_from_slice(&0u32.to_le_bytes());
        std::fs::write(path, &bytes).unwrap();
    }

    #[test]
    fn recupera_la_grabacion_que_no_se_cerro() {
        let dir = std::env::temp_dir().join(format!("atic-cortada-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mic = dir.join("mic.wav");
        wav(&mic, 32_000);
        // Recién escrita puede ser un trabajo en curso: no se toca.
        assert!(cut_recording(&dir, "r1".into()).is_none());
        let old = SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options().write(true).open(&mic).unwrap().set_modified(old).unwrap();
        let rec = cut_recording(&dir, "r1".into()).expect("la recupera");
        assert_eq!(rec.duration_secs, 2);
        assert_eq!(rec.mic_path.as_deref(), Some("mic.wav"));
        assert_eq!(rec.system_path, None);
        assert_eq!(rec.status, RecordingStatus::Recorded);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
