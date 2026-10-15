//! Grabar una zona de la pantalla a MP4. Pendiente en macOS (irá con
//! ScreenCaptureKit y AVAssetWriter): por ahora `start` falla con un aviso y
//! la grabación nunca existe.

use std::path::PathBuf;
use std::time::Duration;

use atic_capture::{Frame, Rect as PhysRect};

use crate::screen_audio::Tracks;

/// Lo que queda al terminar (ver `win32/screen_record.rs`).
pub struct Saved {
    pub path: PathBuf,
    pub thumb: Option<Frame>,
}

/// Una grabación en curso. En macOS todavía no se puede crear una.
pub struct Recording {
    never: std::convert::Infallible,
}

impl Recording {
    pub fn elapsed(&self) -> Duration {
        match self.never {}
    }

    pub fn stop(&self) {
        match self.never {}
    }

    pub fn discard(&self) {
        match self.never {}
    }

    pub fn stopping(&self) -> bool {
        match self.never {}
    }

    pub fn paused(&self) -> bool {
        match self.never {}
    }

    pub fn set_paused(&self, _paused: bool) {
        match self.never {}
    }

    pub fn finished(&self) -> Option<Result<Option<Saved>, String>> {
        match self.never {}
    }
}

pub fn start(_region: PhysRect, _path: PathBuf, _audio: Option<Tracks>) -> Result<Recording, String> {
    Err("grabar la pantalla todavía no está en macOS".into())
}

/// `~/Movies/Atic`, donde quedarán las grabaciones.
pub fn videos_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Movies").join("Atic"))
}
