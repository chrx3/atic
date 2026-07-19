use std::path::PathBuf;

use directories::ProjectDirs;

use crate::error::{Error, Result};

/// Resuelve y crea las rutas de datos de Atic.
///
/// En Windows esto vive bajo `%APPDATA%\ciat\atic\data`.
#[derive(Debug, Clone)]
pub struct AppDirs {
    data_dir: PathBuf,
    db_path: PathBuf,
}

impl AppDirs {
    /// Inicializa el árbol de datos, creando los subdirectorios necesarios.
    pub fn new() -> Result<Self> {
        let dirs = ProjectDirs::from("com", "ciat", "atic").ok_or(Error::NoDataDir)?;
        let data_dir = migrate_legacy_data_dir(dirs.data_dir().to_path_buf())?;
        let db_path = migrate_legacy_db(&data_dir);
        let this = Self { data_dir, db_path };
        std::fs::create_dir_all(this.recordings_dir())?;
        std::fs::create_dir_all(this.models_dir())?;
        std::fs::create_dir_all(this.captures_dir())?;
        std::fs::create_dir_all(this.overlay_frames_dir())?;
        Ok(this)
    }

    /// Raíz de datos (útil para diagnósticos).
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    /// Carpeta que contiene una subcarpeta por grabación.
    pub fn recordings_dir(&self) -> PathBuf {
        self.data_dir.join("recordings")
    }

    /// Carpeta donde se descargan los modelos de Whisper bajo demanda.
    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    /// Carpeta de capturas de pantalla temporales (se limpian por antigüedad).
    pub fn captures_dir(&self) -> PathBuf {
        self.data_dir.join("captures")
    }

    /// Carpeta transitoria para los frames congelados del overlay de selección.
    /// Su contenido se sobreescribe/limpia en cada sesión de captura.
    pub fn overlay_frames_dir(&self) -> PathBuf {
        self.data_dir.join("overlay-frames")
    }

    /// Archivo SQLite principal.
    pub fn db_path(&self) -> PathBuf {
        self.db_path.clone()
    }

    /// Archivo de configuración (JSON).
    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    /// Carpeta propia de una grabación concreta.
    pub fn recording_dir(&self, id: &str) -> PathBuf {
        self.recordings_dir().join(id)
    }

    /// Ruta del JSON de transcripción de una grabación.
    pub fn transcript_path(&self, id: &str) -> PathBuf {
        self.recording_dir(id).join("transcript.json")
    }

    /// Ruta del JSON de resumen de una grabación.
    pub fn summary_path(&self, id: &str) -> PathBuf {
        self.recording_dir(id).join("summary.json")
    }
}

/// Mueve la carpeta de la aplicación anterior la primera vez que se abre Atic.
/// Si el sistema no permite moverla, se sigue usando la ubicación anterior para
/// que ninguna grabación o configuración quede inaccesible.
fn migrate_legacy_data_dir(current: PathBuf) -> Result<PathBuf> {
    let Some(legacy_dirs) = ProjectDirs::from("com", "tsg", "resume-bot") else {
        return Ok(current);
    };
    let legacy = legacy_dirs.data_dir();

    if current.exists() || !legacy.exists() {
        return Ok(current);
    }

    if let Some(parent) = current.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match std::fs::rename(legacy, &current) {
        Ok(()) => {
            tracing::info!(
                from = %legacy.display(),
                to = %current.display(),
                "datos migrados a Atic"
            );
            Ok(current)
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %legacy.display(),
                "no se pudo mover la carpeta anterior; Atic la seguirá usando"
            );
            Ok(legacy.to_path_buf())
        }
    }
}

/// Renombra la base de datos histórica sin impedir el arranque si el archivo
/// está bloqueado. En ese caso Atic continúa usando el nombre anterior.
fn migrate_legacy_db(data_dir: &std::path::Path) -> PathBuf {
    let current = data_dir.join("atic.db3");
    let legacy = data_dir.join("resumebot.db3");

    if current.exists() || !legacy.exists() {
        return current;
    }

    match std::fs::rename(&legacy, &current) {
        Ok(()) => current,
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %legacy.display(),
                "no se pudo renombrar la base de datos anterior; se seguirá usando"
            );
            legacy
        }
    }
}
