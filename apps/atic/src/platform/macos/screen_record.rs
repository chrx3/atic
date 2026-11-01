//! Grabar una zona de la pantalla en macOS como lo hace el sistema (⌘⇧5):
//! ScreenCaptureKit le pasa cada cuadro a AVAssetWriter, que lo codifica en
//! H.264 por hardware, sin que la imagen pase por la CPU
//! (`atic_capture::stream::ScreenRecorder`).
//!
//! El micrófono y el sonido del sistema van con `screen_audio` (los mismos
//! dispositivos de Reuniones) y comparten con el video el reloj de la
//! grabación: pausar es dejar de contar, sin cortar nada. Al terminar, el
//! audio se junta con el video sin volver a codificar la imagen
//! (`atic_capture::stream::mux_pcm`).
//!
//! La pill queda fuera del video salvo que se pida (`show_pill`): se excluyen
//! las ventanas de Atic del filtro de ScreenCaptureKit.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use atic_capture::stream::ScreenRecorder;
use atic_capture::{Frame, Rect as PhysRect};

use crate::screen_audio::{Mixed, RecordClock, ScreenAudio, Tracks, RATE};
use crate::i18n::tf;

/// Lo que queda al terminar (ver `win32/screen_record.rs`).
pub struct Saved {
    pub path: PathBuf,
    pub thumb: Option<Frame>,
    /// Se pidió audio y no quedó en el video: la pill se lo avisa al usuario.
    pub audio_lost: Option<AudioLost>,
}

/// El audio de una grabación que no entró al video.
pub struct AudioLost {
    /// Dónde se conservó (`.atic/<nombre>.audio.wav`); `None` si tampoco se pudo guardar.
    pub kept: Option<PathBuf>,
    /// Qué falló, para el log.
    pub reason: String,
}

struct Inner {
    region: PhysRect,
    path: PathBuf,
    /// La carpeta temporal: el video sin audio y las pistas crudas.
    temp: PathBuf,
    parts: Option<(ScreenRecorder, Option<ScreenAudio>)>,
    /// Se pidió audio pero no abrió (sin permiso, sin dispositivo…): el video sale mudo.
    audio_unavailable: bool,
    stopping: bool,
    finished: Option<Result<Option<Saved>, String>>,
}

/// Una grabación en curso.
pub struct Recording {
    clock: Arc<RecordClock>,
    inner: Arc<Mutex<Inner>>,
}

pub fn start(region: PhysRect, path: PathBuf, audio: Option<Tracks>, show_pill: bool, show_cursor: bool) -> Result<Recording, String> {
    if !super::permissions::ensure_screen_capture() {
        return Err("falta el permiso de Grabación de pantalla (Ajustes → Permisos)".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let temp = std::env::temp_dir().join(format!("atic-grabar-{stamp}"));
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    let clock = Arc::new(RecordClock::default());
    // El audio abre antes que el video; lo que llegue antes de empezar se descarta.
    let audio_wanted = audio.is_some();
    let audio = audio.and_then(|tracks| match ScreenAudio::start(tracks, temp.clone(), clock.clone()) {
        Ok(audio) => Some(audio),
        Err(error) => {
            tracing::warn!(%error, "grabar: sin audio");
            None
        }
    });
    let now = clock.clone();
    let recorder = match ScreenRecorder::start(region, &temp.join("video.mp4"), !show_pill, show_cursor, Box::new(move || now.now())) {
        Ok(recorder) => recorder,
        Err(error) => {
            if let Some(audio) = audio {
                let _ = audio.finish(std::time::Duration::ZERO);
            }
            let _ = std::fs::remove_dir_all(&temp);
            return Err(error);
        }
    };
    clock.begin(Instant::now());
    let inner = Inner {
        region,
        path,
        temp,
        audio_unavailable: audio_wanted && audio.is_none(),
        parts: Some((recorder, audio)),
        stopping: false,
        finished: None,
    };
    Ok(Recording { clock, inner: Arc::new(Mutex::new(inner)) })
}

impl Recording {
    /// El reloj de la grabación: lo comparten el video, el audio y el mouse
    /// (`screen_cursor.rs`).
    pub fn clock(&self) -> Arc<RecordClock> {
        self.clock.clone()
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.clock.recorded()
    }

    pub fn stop(&self) {
        self.finish(true);
    }

    pub fn discard(&self) {
        self.finish(false);
    }

    /// Cerrar el MP4 y juntar el audio espera: fuera del hilo de la pill.
    fn finish(&self, keep: bool) {
        let parts = {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            if inner.stopping {
                return;
            }
            inner.stopping = true;
            inner.parts.take()
        };
        let Some((recorder, audio)) = parts else {
            return;
        };
        let duration = self.clock.recorded();
        let shared = self.inner.clone();
        let _ = std::thread::Builder::new().name("grabar-cerrar".into()).spawn(move || {
            let (region, path, temp, audio_unavailable) = {
                let Ok(inner) = shared.lock() else {
                    return;
                };
                (inner.region, inner.path.clone(), inner.temp.clone(), inner.audio_unavailable)
            };
            let result = if keep {
                save(recorder, audio, duration, region, &path, &temp, audio_unavailable)
            } else {
                drop(recorder);
                // El audio se cierra siempre: si no, el micrófono queda abierto.
                if let Some(audio) = audio {
                    let _ = audio.finish(duration);
                }
                Ok(None)
            };
            // Si falló el guardado, el temporal se queda: adentro están el video y el
            // audio completos. Borrarlo sería perder la sesión por un fallo de traslado;
            // mejor que el aviso diga dónde quedó. Descartar sí borra siempre.
            let result = match result {
                Err(error) if keep => Err(format!("{error} · {}", tf("pill.recording.keptTemp", &[("path", &temp.display().to_string())]))),
                other => other,
            };
            if result.is_ok() || !keep {
                let _ = std::fs::remove_dir_all(&temp);
            }
            if let Ok(mut inner) = shared.lock() {
                inner.finished = Some(result);
            }
        });
    }

    pub fn stopping(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.stopping)
    }

    pub fn paused(&self) -> bool {
        self.clock.paused()
    }

    /// Pausar es dejar de contar: los cuadros que llegan no van al video ni al audio.
    pub fn set_paused(&self, paused: bool) {
        if !self.stopping() {
            self.clock.set_paused(paused);
        }
    }

    pub fn finished(&self) -> Option<Result<Option<Saved>, String>> {
        self.inner.lock().ok()?.finished.take()
    }
}

/// Cierra el video, le junta el audio y lo deja en `path`.
fn save(
    recorder: ScreenRecorder,
    audio: Option<ScreenAudio>,
    duration: std::time::Duration,
    region: PhysRect,
    path: &Path,
    temp: &Path,
    audio_unavailable: bool,
) -> Result<Option<Saved>, String> {
    let video = temp.join("video.mp4");
    let recorded = recorder.finish(duration);
    let mixed = audio.map(|audio| audio.finish(duration));
    if !recorded? {
        return Err("no llegó ningún cuadro de la pantalla".into());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let saved = |audio_lost: Option<AudioLost>| Ok(Some(Saved { path: path.to_path_buf(), thumb: thumb(region), audio_lost }));
    let mut audio_lost = audio_unavailable.then(|| AudioLost { kept: None, reason: "el audio no se pudo abrir".into() });
    match mixed {
        Some(Ok(Some(mixed))) => match atic_capture::stream::mux_pcm(&video, &mixed.path, mixed.frames, RATE, path) {
            Ok(()) => return saved(None),
            Err(error) => {
                tracing::warn!(%error, "grabar: no se pudo juntar el audio; queda sin él");
                let _ = std::fs::remove_file(path);
                // La carpeta temporal se borra al volver: el audio se guarda ahora.
                audio_lost = Some(AudioLost { kept: keep_audio(&mixed, path), reason: error });
            }
        },
        Some(Err(error)) => {
            tracing::warn!(%error, "grabar: el audio falló; queda sin él");
            audio_lost = Some(AudioLost { kept: None, reason: error });
        }
        Some(Ok(None)) | None => {}
    }
    move_file(&video, path)?;
    saved(audio_lost)
}

/// El audio que no entró al video queda como WAV en `.atic/<nombre>.audio.wav`, junto
/// a la grabación, para que no se pierda.
fn keep_audio(mixed: &Mixed, video: &Path) -> Option<PathBuf> {
    let out = crate::screen_cursor::audio_path(video)?;
    match crate::screen_audio::save_wav(&mixed.path, mixed.frames, &out) {
        Ok(()) => {
            tracing::info!(audio = %out.display(), "grabar: el audio quedó aparte");
            Some(out)
        }
        Err(error) => {
            tracing::warn!(%error, "grabar: tampoco se pudo guardar el audio aparte");
            None
        }
    }
}

/// Mueve el temporal a su lugar; si están en discos distintos, lo copia.
fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// La zona grabada ahora, como miniatura (el video termina donde se paró).
fn thumb(region: PhysRect) -> Option<Frame> {
    atic_capture::engine::capture_rect(region, false).ok()
}

/// `~/Movies/Atic`, donde quedan las grabaciones.
pub fn videos_dir() -> Option<PathBuf> {
    Some(crate::paths::home()?.join("Movies").join("Atic"))
}
