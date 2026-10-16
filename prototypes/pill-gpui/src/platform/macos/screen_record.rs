//! Grabar una zona de la pantalla en macOS, con el `screencapture` del
//! sistema (`-v -R`): ScreenCaptureKit por dentro, video H.264, el audio del
//! sistema (`-A`) y el micrófono (`-g`), y la pill fuera del video salvo que
//! se pida (el overlay ya está excluido de las capturas).
//!
//! `screencapture` no pausa: cada pausa cierra un segmento y al seguir empieza
//! otro. Al terminar se juntan en un MP4 con AVFoundation (`AVMutableComposition`
//! y `AVAssetExportSession`), que es también lo que pasa el `.mov` de
//! `screencapture` a `.mp4`.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atic_capture::{Frame, Rect as PhysRect};

use crate::screen_audio::Tracks;

/// Lo que queda al terminar (ver `win32/screen_record.rs`).
pub struct Saved {
    pub path: PathBuf,
    pub thumb: Option<Frame>,
}

struct Inner {
    region: PhysRect,
    path: PathBuf,
    audio: Option<Tracks>,
    /// El segmento que se graba ahora.
    child: Option<Child>,
    segments: Vec<PathBuf>,
    /// Lo grabado en los segmentos cerrados.
    recorded: Duration,
    since: Option<Instant>,
    stopping: bool,
    finished: Option<Result<Option<Saved>, String>>,
}

/// Una grabación en curso.
pub struct Recording {
    inner: Arc<Mutex<Inner>>,
}

pub fn start(region: PhysRect, path: PathBuf, audio: Option<Tracks>) -> Result<Recording, String> {
    if !super::permissions::ensure_screen_capture() {
        return Err("falta el permiso de Grabación de pantalla (Ajustes → Permisos)".into());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut inner = Inner {
        region,
        path,
        audio,
        child: None,
        segments: Vec::new(),
        recorded: Duration::ZERO,
        since: None,
        stopping: false,
        finished: None,
    };
    inner.begin_segment()?;
    Ok(Recording { inner: Arc::new(Mutex::new(inner)) })
}

impl Inner {
    fn begin_segment(&mut self) -> Result<(), String> {
        let segment = std::env::temp_dir().join(format!("atic-grabacion-{}-{}.mov", std::process::id(), self.segments.len()));
        let _ = std::fs::remove_file(&segment);
        let r = self.region;
        let mut command = Command::new("screencapture");
        command.args(["-v", "-x", "-C", &format!("-R{},{},{},{}", r.x, r.y, r.width, r.height)]);
        if let Some(audio) = &self.audio {
            if audio.system {
                command.arg("-A");
            }
            if audio.mic {
                command.arg("-g");
            }
        }
        let child = command
            .arg(&segment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("screencapture: {e}"))?;
        self.child = Some(child);
        self.segments.push(segment);
        self.since = Some(Instant::now());
        Ok(())
    }

    /// Saca el segmento en curso para cerrarlo fuera del candado
    /// (`close_segment`): así la pill no espera a `screencapture`.
    fn take_segment(&mut self) -> Option<Child> {
        let child = self.child.take()?;
        if let Some(since) = self.since.take() {
            self.recorded += since.elapsed();
        }
        Some(child)
    }
}

/// Cierra un segmento: con SIGINT `screencapture` termina y deja el archivo
/// cerrado (como Ctrl+C en la terminal).
fn close_segment(mut child: Child) {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    {
        // SAFETY: el pid es de un hijo que todavía no se esperó.
        unsafe {
            kill(child.id() as i32, 2);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

impl Recording {
    pub fn elapsed(&self) -> Duration {
        let Ok(inner) = self.inner.lock() else {
            return Duration::ZERO;
        };
        inner.recorded + inner.since.map_or(Duration::ZERO, |since| since.elapsed())
    }

    pub fn stop(&self) {
        self.finish(true);
    }

    pub fn discard(&self) {
        self.finish(false);
    }

    fn finish(&self, keep: bool) {
        {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            if inner.stopping {
                return;
            }
            inner.stopping = true;
        }
        let shared = self.inner.clone();
        let _ = std::thread::Builder::new().name("grabar-cerrar".into()).spawn(move || {
            let (child, segments, path, region) = {
                let Ok(mut inner) = shared.lock() else {
                    return;
                };
                (inner.take_segment(), inner.segments.clone(), inner.path.clone(), inner.region)
            };
            if let Some(child) = child {
                close_segment(child);
            }
            let result = if keep { join(&segments, &path, region) } else { Ok(None) };
            for segment in &segments {
                let _ = std::fs::remove_file(segment);
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
        self.inner.lock().is_ok_and(|inner| inner.child.is_none() && !inner.stopping)
    }

    pub fn set_paused(&self, paused: bool) {
        let shared = self.inner.clone();
        // Cerrar un segmento espera a `screencapture`: fuera del hilo de la pill.
        let _ = std::thread::Builder::new().name("grabar-pausa".into()).spawn(move || {
            let child = {
                let Ok(mut inner) = shared.lock() else {
                    return;
                };
                if inner.stopping {
                    return;
                }
                if !paused {
                    if inner.child.is_none() {
                        if let Err(error) = inner.begin_segment() {
                            tracing::warn!(%error, "grabar: no se pudo seguir");
                        }
                    }
                    return;
                }
                inner.take_segment()
            };
            if let Some(child) = child {
                close_segment(child);
            }
        });
    }

    pub fn finished(&self) -> Option<Result<Option<Saved>, String>> {
        self.inner.lock().ok()?.finished.take()
    }
}

/// Junta los segmentos en `path` (MP4) y saca el primer cuadro para el estante.
fn join(segments: &[PathBuf], path: &std::path::Path, region: PhysRect) -> Result<Option<Saved>, String> {
    let segments: Vec<PathBuf> =
        segments.iter().filter(|s| std::fs::metadata(s).is_ok_and(|m| m.len() > 0)).cloned().collect();
    if segments.is_empty() {
        return Err("screencapture no dejó el video (¿falta el permiso de Grabación de pantalla?)".into());
    }
    let path = path.with_extension("mp4");
    let _ = std::fs::remove_file(&path);
    if let Err(error) = av::export(&segments, &path) {
        // Si AVFoundation no pudo: el primer segmento tal cual, en `.mov`.
        tracing::warn!(%error, "grabar: no se pudo exportar a MP4");
        let fallback = path.with_extension("mov");
        std::fs::copy(&segments[0], &fallback).map_err(|e| e.to_string())?;
        return Ok(Some(Saved { path: fallback, thumb: thumb(region) }));
    }
    Ok(Some(Saved { path, thumb: thumb(region) }))
}

/// La zona grabada ahora, como miniatura (el video termina donde se paró).
fn thumb(region: PhysRect) -> Option<Frame> {
    atic_capture::engine::capture_rect(region, false).ok()
}

/// Juntar y exportar con AVFoundation.
mod av {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use objc2::encode::{Encode, Encoding};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::msg_send;
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    // SAFETY: la misma disposición que `CMTime` (`{?=qiIq}`).
    unsafe impl Encode for CMTime {
        const ENCODING: Encoding = Encoding::Struct("?", &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING]);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CMTimeRange {
        start: CMTime,
        duration: CMTime,
    }

    // SAFETY: la misma disposición que `CMTimeRange`.
    unsafe impl Encode for CMTimeRange {
        const ENCODING: Encoding = Encoding::Struct("?", &[CMTime::ENCODING, CMTime::ENCODING]);
    }

    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {
        static kCMTimeZero: CMTime;
        fn CMTimeAdd(a: CMTime, b: CMTime) -> CMTime;
    }

    fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
        AnyClass::get(name).ok_or_else(|| format!("sin {}", name.to_string_lossy()))
    }

    pub fn export(segments: &[PathBuf], out: &Path) -> Result<(), String> {
        objc2::rc::autoreleasepool(|_| unsafe { export_inner(segments, out) })
    }

    /// SAFETY: objetos de AVFoundation creados y usados aquí; la sesión se
    /// espera hasta que termina.
    unsafe fn export_inner(segments: &[PathBuf], out: &Path) -> Result<(), String> {
        let composition: Retained<AnyObject> = msg_send![class(c"AVMutableComposition")?, composition];
        let mut at = kCMTimeZero;
        let none: *const AnyObject = std::ptr::null();
        for segment in segments {
            let url: Retained<AnyObject> =
                msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&segment.to_string_lossy())];
            let asset: Retained<AnyObject> = msg_send![class(c"AVURLAsset")?, URLAssetWithURL: &*url, options: none];
            let duration: CMTime = msg_send![&asset, duration];
            let range = CMTimeRange { start: kCMTimeZero, duration };
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let ok: Bool = msg_send![&composition, insertTimeRange: range, ofAsset: &*asset, atTime: at, error: &mut error];
            if !ok.as_bool() {
                return Err(format!("no se pudo sumar {}", segment.display()));
            }
            at = CMTimeAdd(at, duration);
        }
        let session: Allocated<AnyObject> = msg_send![class(c"AVAssetExportSession")?, alloc];
        let preset = NSString::from_str("AVAssetExportPresetHighestQuality");
        let session: Option<Retained<AnyObject>> = msg_send![session, initWithAsset: &*composition, presetName: &*preset];
        let session = session.ok_or("AVFoundation no armó la exportación")?;
        let url: Retained<AnyObject> = msg_send![class(c"NSURL")?, fileURLWithPath: &*NSString::from_str(&out.to_string_lossy())];
        let _: () = msg_send![&session, setOutputURL: &*url];
        let _: () = msg_send![&session, setOutputFileType: &*NSString::from_str("public.mpeg-4")];
        let _: () = msg_send![&session, setShouldOptimizeForNetworkUse: Bool::YES];
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let done = block2::RcBlock::new(move || {
            let _ = tx.send(());
        });
        let _: () = msg_send![&session, exportAsynchronouslyWithCompletionHandler: &*done];
        rx.recv_timeout(Duration::from_secs(15 * 60)).map_err(|_| "la exportación no terminó".to_string())?;
        // `AVAssetExportSessionStatusCompleted`.
        let status: isize = msg_send![&session, status];
        if status != 3 {
            let error: Option<Retained<AnyObject>> = msg_send![&session, error];
            let text: Option<Retained<NSString>> = error.and_then(|e| msg_send![&e, localizedDescription]);
            return Err(text.map(|t| t.to_string()).unwrap_or_else(|| format!("estado {status}")));
        }
        Ok(())
    }

    /// `ATIC_TEST_SEGMENTS="a.mov:b.mov"` junta dos grabaciones de verdad.
    #[cfg(test)]
    #[test]
    fn junta_segmentos_de_verdad() {
        let Ok(list) = std::env::var("ATIC_TEST_SEGMENTS") else {
            return;
        };
        let segments: Vec<PathBuf> = list.split(':').map(PathBuf::from).collect();
        let out = std::env::temp_dir().join("atic-junta-prueba.mp4");
        let _ = std::fs::remove_file(&out);
        export(&segments, &out).expect("exporta");
        assert!(std::fs::metadata(&out).unwrap().len() > 0);
    }
}

/// `~/Movies/Atic`, donde quedan las grabaciones.
pub fn videos_dir() -> Option<PathBuf> {
    Some(crate::paths::home()?.join("Movies").join("Atic"))
}
