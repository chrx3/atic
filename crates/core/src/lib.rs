//! Núcleo de dominio de Atic: rutas de datos, configuración,
//! almacenamiento (SQLite) y tipos compartidos. No depende de Tauri ni de
//! la plataforma de UI, para poder reutilizarse en escritorio y móvil.

pub mod config;
pub mod locale;
pub mod db;
pub mod error;
pub mod fs_atomic;
pub mod models;
pub mod paths;
pub mod secrets;
pub mod summary;
pub mod sync;
pub mod transcript;

pub use config::{Config, SshHost};
pub use db::{AgentThreadRow, Db};
pub use error::{Error, Result};
pub use fs_atomic::{write_atomic, write_atomic_str};
pub use models::{Recording, RecordingStatus};
pub use paths::AppDirs;
pub use secrets::SecretKind;
pub use summary::Summary;
pub use sync::MutexExt;
pub use transcript::{Segment, Speaker, Transcript};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn config_roundtrip_defaults() {
        let dir = std::env::temp_dir().join(format!("atic-core-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");

        let cfg = Config::default();
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path);
        assert_eq!(loaded.whisper_model, "base");
        assert_eq!(loaded.dictation_whisper_model, "base");
        assert_eq!(loaded.dictation_backend, "groq");
        assert_eq!(loaded.summary_backend, "claude");
        assert_eq!(loaded.summary_model, "claude-opus-4-8");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn db_insert_and_list() {
        let dir = std::env::temp_dir().join(format!("atic-core-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("test.db3")).unwrap();

        let mut rec = Recording::new(Utc::now());
        rec.mic_path = Some("mic.wav".to_string());
        rec.system_path = Some("system.wav".to_string());
        db.insert_recording(&rec).unwrap();

        let all = db.list_recordings().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, rec.id);

        db.update_status(&rec.id, RecordingStatus::Transcribed)
            .unwrap();
        let fetched = db.get_recording(&rec.id).unwrap().unwrap();
        assert_eq!(fetched.status, RecordingStatus::Transcribed);

        db.delete_recording(&rec.id).unwrap();
        assert!(db.list_recordings().unwrap().is_empty());

        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_roundtrip() {
        for s in [
            RecordingStatus::Recorded,
            RecordingStatus::Transcribing,
            RecordingStatus::Transcribed,
            RecordingStatus::Summarizing,
            RecordingStatus::Summarized,
            RecordingStatus::Error,
        ] {
            assert_eq!(RecordingStatus::parse(s.as_str()).unwrap(), s);
        }
    }
}
