//! Grabar la pantalla desde Capturas.
//!
//! Se mantiene apretado el atajo de Capturas y la mira pasa a elegir qué
//! grabar (`capture.rs`): una ventana, una zona arrastrada o, con Espacio, la
//! pantalla, con las opciones de `RecordOptions` (M, A y P en la mira). Tras
//! una cuenta de tres empieza; el tab muestra el punto rojo con el reloj (el de
//! Reuniones), y un clic en él o el mismo atajo la detienen; el vistazo de
//! Capturas la pausa o la descarta (`peeks.rs`). El MP4, con el
//! micrófono y el sonido del sistema (`screen_audio.rs`), queda en
//! `Videos\Atic` y vuela al estante con su primer cuadro (`shelf.rs`).

use std::path::PathBuf;
use std::time::Instant;

use atic_capture::{Frame, Rect as PhysRect};
use gpui::Context;
use serde::{Deserialize, Serialize};

use crate::platform::screen_record;
use crate::screen_audio::Tracks;
use crate::geometry::Rect;
use crate::{distance, PillShape};

/// Lo que se elige en la mira antes de grabar; se recuerda para la próxima.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordOptions {
    pub mic: bool,
    pub system: bool,
    /// La pill y la UI de Atic salen en el video (si no, se excluyen).
    pub show_pill: bool,
    /// «Cursor suave» (macOS): se graba sin el cursor del sistema y, al
    /// guardar, Atic dibuja uno suavizado; el editor puede agrandarlo.
    pub smooth_cursor: bool,
}

impl Default for RecordOptions {
    fn default() -> Self {
        Self {
            mic: true,
            system: true,
            show_pill: false,
            smooth_cursor: false,
        }
    }
}

impl RecordOptions {
    fn file() -> Option<std::path::PathBuf> {
        crate::paths::file("record-options.json")
    }

    pub fn load() -> Self {
        Self::file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let (Some(path), Ok(text)) = (Self::file(), serde_json::to_string(self)) {
            let _ = std::fs::write(path, text);
        }
    }

    fn tracks(&self) -> Option<Tracks> {
        (self.mic || self.system).then_some(Tracks {
            mic: self.mic,
            system: self.system,
        })
    }
}

/// Lo de `region` que cae en la pantalla que más parte suya cubre. Sin pantallas
/// enumeradas, la zona queda como vino.
fn one_screen(region: PhysRect, monitors: &[atic_capture::MonitorInfo]) -> PhysRect {
    let overlap = |bounds: PhysRect| -> u64 {
        let (left, top) = (region.x.max(bounds.x), region.y.max(bounds.y));
        let (right, bottom) = (region.right().min(bounds.right()), region.bottom().min(bounds.bottom()));
        if right <= left || bottom <= top {
            0
        } else {
            (right - left) as u64 * (bottom - top) as u64
        }
    };
    let Some(best) = monitors.iter().map(|monitor| monitor.bounds).max_by_key(|bounds| overlap(*bounds)) else {
        return region;
    };
    if overlap(best) == 0 {
        return region;
    }
    let (left, top) = (region.x.max(best.x), region.y.max(best.y));
    let (right, bottom) = (region.right().min(best.right()), region.bottom().min(best.bottom()));
    PhysRect::from_ltrb(left, top, right, bottom)
}

/// Algo que salió mal al guardar y que el usuario tiene que saber: la grabación igual
/// quedó. Se muestra en el tab (donde va el reloj) y queda en `ScreenRecording` para
/// quien lo quiera decir con más espacio (`notices_text`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Notice {
    /// El audio no quedó en el video. `kept`: se conservó aparte (`.atic/<nombre>.audio.wav`).
    AudioLost { kept: bool },
    /// No se pudo dibujar el cursor suave: el video quedó sin cursor.
    CursorLost,
}

impl Notice {
    /// Lo corto, que cabe en el tab.
    pub fn short(self) -> &'static str {
        match self {
            Notice::AudioLost { .. } => crate::i18n::t("pill.recording.audioLostShort"),
            Notice::CursorLost => crate::i18n::t("pill.recording.cursorLostShort"),
        }
    }

    /// La frase completa.
    pub fn text(self) -> &'static str {
        match self {
            Notice::AudioLost { kept: true } => crate::i18n::t("pill.recording.audioLostKept"),
            Notice::AudioLost { kept: false } => crate::i18n::t("pill.recording.audioLost"),
            Notice::CursorLost => crate::i18n::t("pill.recording.cursorLost"),
        }
    }
}

/// Cuánto se ve cada aviso en el tab.
const NOTICE_SECS: u64 = 4;

/// Qué aviso toca a los `elapsed` segundos de empezar a mostrarlos, de `count`, uno
/// tras otro; `None` cuando ya pasaron todos.
fn notice_slot(elapsed: u64, count: usize) -> Option<usize> {
    let slot = (elapsed / NOTICE_SECS) as usize;
    (slot < count).then_some(slot)
}

#[derive(Default)]
pub struct ScreenRecording {
    /// Se mantuvo el atajo mientras la mira todavía congelaba: abre grabando.
    pub requested: bool,
    recording: Option<screen_record::Recording>,
    /// El mouse mientras graba, para el editor (`screen_cursor.rs`).
    cursor: Option<crate::screen_cursor::CursorTrack>,
    /// Se graba sin cursor: al guardar se dibuja (`bake_cursor`).
    smooth: bool,
    /// Dibujando el cursor después de grabar: cuánto va (0–1, en bits de `f64`).
    baking: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// Lo grabado, en la ventana: de ahí sale volando al estante.
    from: Option<Rect>,
    /// Lo que falló al guardar la última grabación (hasta que empieza otra).
    notices: Vec<Notice>,
    /// Desde cuándo se muestran en el tab; `None` si aún no o ya pasaron.
    notice_since: Option<Instant>,
}

impl ScreenRecording {
    pub fn active(&self) -> bool {
        self.recording.is_some()
    }

    pub fn paused(&self) -> bool {
        self.recording.as_ref().is_some_and(|recording| recording.paused())
    }

    /// El reloj del tab mientras graba.
    /// Lo que dice el tab: el reloj mientras graba y «Guardando…» (con cuánto va,
    /// si se sabe) mientras se cierra el archivo o se dibuja el cursor: con una
    /// grabación larga eso tarda y sin aviso parecía que no había pasado nada.
    pub fn clock(&self) -> Option<String> {
        if let Some(progress) = &self.baking {
            let p = f64::from_bits(progress.load(std::sync::atomic::Ordering::Relaxed));
            return Some(crate::i18n::tf("pill.recording.savingPct", &[("pct", &((p * 100.0).round() as u32))]));
        }
        // Ya guardada: los avisos de lo que falló, uno tras otro, en el lugar del reloj.
        let Some(recording) = self.recording.as_ref() else {
            let slot = notice_slot(self.notice_since?.elapsed().as_secs(), self.notices.len())?;
            return Some(self.notices[slot].short().to_string());
        };
        if recording.stopping() {
            return Some(crate::i18n::t("pill.recording.saving").to_string());
        }
        Some(crate::meetings::stopwatch(recording.elapsed()))
    }

    /// Anota algo que falló al guardar (una vez cada tipo).
    fn warn(&mut self, notice: Notice) {
        if !self.notices.contains(&notice) {
            self.notices.push(notice);
        }
    }

    /// Empieza a mostrar los avisos en el tab; se llama cuando termina de guardar.
    fn show_notices(&mut self) {
        if !self.notices.is_empty() {
            self.notice_since = Some(Instant::now());
        }
    }

    /// Los avisos de la última grabación en una frase por línea, para decirlos con más
    /// espacio que el tab (por ejemplo, en la nota del estante). Siguen ahí hasta que
    /// empieza otra grabación.
    #[allow(dead_code)]
    pub(crate) fn notices_text(&self) -> Option<String> {
        (!self.notices.is_empty()).then(|| self.notices.iter().map(|notice| notice.text()).collect::<Vec<_>>().join("\n"))
    }
}

impl crate::Pill {
    /// Mantener el atajo de Capturas: la mira abierta (o por abrir) pasa a grabar.
    pub(crate) fn capture_held(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.capture.as_ref() {
            view.update(cx, |view, cx| view.set_record(cx));
        } else if self.capture_pending {
            self.screen_rec.requested = true;
        }
    }

    pub(crate) fn start_screen_recording(&mut self, region: PhysRect, options: RecordOptions, cx: &mut Context<Self>) {
        let Some(dir) = screen_record::videos_dir() else {
            eprintln!("grabar: no se encontró la carpeta de Videos");
            return;
        };
        // El video sale de una pantalla y el mouse se mide sobre la misma zona: una
        // grabación que cruzara dos pantallas dejaría el cursor desincronizado y solo
        // mostraría una (el codificador es de un monitor). El recorte es la red por si
        // algo pidiera una zona que cruza.
        let asked = region;
        let region = one_screen(asked, &atic_capture::monitors::enumerate());
        if region != asked {
            tracing::info!(
                pedida = %format!("{}×{} en ({}, {})", asked.width, asked.height, asked.x, asked.y),
                queda = %format!("{}×{} en ({}, {})", region.width, region.height, region.x, region.y),
                "grabar: la zona cruzaba pantallas; se graba una"
            );
        }
        let name = chrono::Local::now().format("Grabación %Y-%m-%d %H-%M-%S.mp4").to_string();
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(!options.show_pill);
        }
        match screen_record::start(region, dir.join(name), options.tracks(), options.show_pill, !options.smooth_cursor) {
            Ok(recording) => {
                self.screen_rec.notices.clear();
                self.screen_rec.notice_since = None;
                println!(
                    "grabar: {}×{} en ({}, {}), {options:?}",
                    region.width, region.height, region.x, region.y
                );
                self.screen_rec.cursor = crate::screen_cursor::CursorTrack::start(region, recording.clock());
                self.screen_rec.smooth = options.smooth_cursor;
                self.screen_rec.recording = Some(recording);
                self.screen_rec.from = self.overlay.as_ref().and_then(|overlay| {
                    let rect = crate::win::RECT {
                        left: region.x,
                        top: region.y,
                        right: region.right(),
                        bottom: region.bottom(),
                    };
                    overlay.to_logical(&rect, self.scale_factor)
                });
                self.rec_clock = self.screen_rec.clock();
            }
            Err(error) => {
                eprintln!("grabar: {error}");
                if let Some(overlay) = self.overlay.as_ref() {
                    overlay.exclude_from_capture(false);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn stop_screen_recording(&mut self, cx: &mut Context<Self>) {
        if let Some(cursor) = self.screen_rec.cursor.as_ref() {
            cursor.halt();
        }
        if let Some(recording) = self.screen_rec.recording.as_ref() {
            recording.stop();
            self.rec_clock = None;
            cx.notify();
        }
    }

    pub(crate) fn toggle_screen_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(recording) = self.screen_rec.recording.as_ref() {
            recording.set_paused(!recording.paused());
            cx.notify();
        }
    }

    /// Termina sin guardar.
    pub(crate) fn discard_screen_recording(&mut self, cx: &mut Context<Self>) {
        self.screen_rec.cursor = None;
        if let Some(recording) = self.screen_rec.recording.as_ref() {
            recording.discard();
            self.rec_clock = None;
            cx.notify();
        }
    }

    /// Grabando, el reloj del tab (y el punto, en la gota o de costado)
    /// detiene la grabación con un clic.
    pub(crate) fn over_rec_clock(&self, p: (f32, f32), now: Instant) -> bool {
        if self.screen_rec.clock().is_none() || !self.over_pill(p, now, 0.0) || self.over_mark(p, now) {
            return false;
        }
        // En el orden elegido del notch cerrado, el reloj está donde le toca.
        if let Some(places) = self.rest_places(now) {
            return places.over(crate::tab_size::RestItem::Clock, p, 6.0);
        }
        match self.shape(now) {
            PillShape::Tab { edge, .. } if !edge.is_vertical() => p.0 < self.mark_center(now).0,
            _ => self.art_center(now).is_some_and(|center| distance(p, center) <= 12.0),
        }
    }

    /// En cada vuelta: el reloj del tab y, cuando el archivo quedó, al
    /// estante con su primer cuadro.
    pub(crate) fn poll_screen_recording(&mut self, cx: &mut Context<Self>) {
        let Some(recording) = self.screen_rec.recording.as_ref() else {
            // Dibujando el cursor: el tab sigue diciendo cuánto falta; y ya guardada, los
            // avisos de lo que falló, hasta que pasan.
            if self.screen_rec.baking.is_some() || self.screen_rec.notice_since.is_some() {
                let clock = self.screen_rec.clock();
                if clock.is_none() {
                    self.screen_rec.notice_since = None;
                }
                if clock != self.rec_clock {
                    self.rec_clock = clock;
                    cx.notify();
                }
            }
            return;
        };
        let Some(result) = recording.finished() else {
            let clock = self.screen_rec.clock();
            if clock != self.rec_clock {
                self.rec_clock = clock;
                cx.notify();
            }
            return;
        };
        self.screen_rec.recording = None;
        self.rec_clock = None;
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(false);
        }
        let cursor = self.screen_rec.cursor.take();
        match result {
            Ok(None) => println!("grabar: descartada"),
            Ok(Some(saved)) => {
                println!("grabar: guardada en {}", saved.path.display());
                // El audio que no entró al video no se pierde en silencio: queda aparte y se avisa.
                #[cfg(target_os = "macos")]
                if let Some(lost) = saved.audio_lost.as_ref() {
                    tracing::warn!(reason = %lost.reason, kept = ?lost.kept, "grabar: la grabación quedó sin audio");
                    self.screen_rec.warn(Notice::AudioLost { kept: lost.kept.is_some() });
                }
                let smooth = std::mem::take(&mut self.screen_rec.smooth);
                let track = cursor.map(|cursor| {
                    let mut track = cursor.finish();
                    track.cursor_hidden = smooth;
                    if let Err(error) = crate::screen_cursor::save(&track, &saved.path) {
                        tracing::warn!(%error, "grabar: no se guardó el registro del mouse");
                    }
                    track
                });
                if smooth {
                    let from = self.screen_rec.from.take();
                    self.bake_cursor(saved, track, from, cx);
                    cx.notify();
                    return;
                }
                let _ = &track;
                let from = self.screen_rec.from.take().unwrap_or_else(|| {
                    let (x, y) = self.mark_center(Instant::now());
                    Rect::new(x, y, 1.0, 1.0)
                });
                match saved.thumb {
                    Some(frame) => self.shelve_video(saved.path, frame, from, cx),
                    None => open_video(saved.path),
                }
                self.screen_rec.show_notices();
            }
            Err(error) => eprintln!("grabar: {error}"),
        }
        cx.notify();
    }
}

/// La miniatura de la grabación (480 px de ancho) en `.atic/`, para listarla en
/// el vistazo de Capturas sin abrir el video.
fn save_thumb(video: &std::path::Path, frame: &Frame) {
    let Some(path) = crate::screen_cursor::thumb_path(video) else { return };
    let (w, h) = (frame.width(), frame.height());
    if w == 0 || h == 0 {
        return;
    }
    let tw = w.min(480);
    let th = ((h as u64 * tw as u64) / w as u64).max(1) as u32;
    let mut small = Vec::new();
    atic_capture::h264::scale_bgra(&frame.bgra, (w, h), (tw, th), &mut small);
    let thumb = Frame::new(atic_capture::Rect::new(0, 0, tw, th), small);
    if let (Some(dir), Ok(jpeg)) = (path.parent(), thumb.to_jpeg(80)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&path, jpeg);
    }
}

/// Abre una grabación en el editor (`recording_editor/`).
pub(crate) fn open_recording(path: PathBuf, cx: &mut gpui::App) {
    crate::recording_editor::open(path, cx);
}

/// Las grabaciones más nuevas de la carpeta de videos (`Videos/Atic`), sin las
/// de `.atic` ni las exportadas a medias.
pub(crate) fn recent_recordings(limit: usize) -> Vec<PathBuf> {
    let Some(dir) = screen_record::videos_dir() else { return Vec::new() };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            matches!(ext.as_str(), "mp4" | "mov").then_some(())?;
            let meta = entry.metadata().ok()?;
            (meta.len() > 0).then_some(())?;
            Some((meta.modified().ok()?, path))
        })
        .collect();
    files.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    files.into_iter().take(limit).map(|(_, path)| path).collect()
}

/// La carpeta de las grabaciones.
pub(crate) fn recordings_dir() -> Option<PathBuf> {
    screen_record::videos_dir()
}

/// La forma del cursor del registro (`Track::shapes`) como la dibuja el exportador.
pub fn cursor_shape_of(code: u8) -> atic_capture::stream::CursorShape {
    use atic_capture::stream::CursorShape;
    match code {
        crate::screen_cursor::SHAPE_HAND => CursorShape::Hand,
        crate::screen_cursor::SHAPE_IBEAM => CursorShape::IBeam,
        crate::screen_cursor::SHAPE_CROSS => CursorShape::Cross,
        _ => CursorShape::Arrow,
    }
}

/// El alto de la flecha dibujada, en puntos (`export_macos::drawn_arrow`).
pub const CURSOR_POINTS: f64 = 21.0;

/// El alto del cursor en píxeles del video: la flecha del sistema a la escala
/// de la grabación (Retina: el doble), por `size`.
pub fn cursor_px(track: &crate::screen_cursor::Track, video_w: f64, size: f64) -> f64 {
    let points = track.region[2].max(1) as f64;
    CURSOR_POINTS * (video_w / points) * size
}

impl crate::Pill {
    /// «Cursor suave»: la grabación sin cursor pasa a `.atic/<nombre>.master.mp4`
    /// (la usa el editor) y la de afuera se rehace con el cursor suavizado
    /// dibujado. Al terminar, al estante como siempre.
    fn bake_cursor(&mut self, saved: crate::platform::screen_record::Saved, track: Option<crate::screen_cursor::Track>, from: Option<Rect>, cx: &mut Context<Self>) {
        use atic_capture::stream::{Decor, Export};
        let path = saved.path.clone();
        let from = from.unwrap_or_else(|| {
            let (x, y) = self.mark_center(Instant::now());
            Rect::new(x, y, 1.0, 1.0)
        });
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0f64.to_bits()));
        self.screen_rec.baking = Some(progress.clone());
        let job = move || -> Result<(), String> {
            // Sin el registro del mouse no hay cursor que dibujar: el video no se toca.
            if track.is_none() {
                return Err("sin registro del mouse".into());
            }
            let master = crate::screen_cursor::master_path(&path).ok_or("sin nombre")?;
            let baking = crate::screen_cursor::baking_path(&path).ok_or("sin nombre")?;
            if let Some(dir) = master.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            // El video de afuera no se mueve: el master es otro nombre para el mismo archivo
            // (o una copia, si el disco no admite enlaces), el cursor se dibuja en un
            // temporal y recién al final reemplaza al video de un solo `rename`. Así, pase
            // lo que pase (un fallo, o que el proceso muera a mitad), el video de afuera
            // siempre existe y entero: el sin cursor o el con cursor, nunca a medias.
            let _ = std::fs::remove_file(&master);
            if std::fs::hard_link(&path, &master).is_err() {
                std::fs::copy(&path, &master).map_err(|e| e.to_string())?;
            }
            let (duration, (video_w, _)) = atic_capture::stream::video_info(&master)?;
            let project = crate::edit_project::Project::default();
            let size = track.as_ref().map_or(0.0, |track| cursor_px(track, video_w, 1.0));
            // El de afuera es el que se comparte: se hornea en HEVC (la mitad de peso a
            // igual calidad) y con tope de 1080, igual que una exportación normal. Si el
            // equipo no tiene HEVC, se rehace en H.264: mejor un archivo grande que
            // quedarse sin cursor.
            let bake = |options: atic_capture::stream::ExportOptions| -> Result<(), String> {
                let motion = crate::edit_project::Motion::new(&project, track.as_ref(), duration);
                let progress = progress.clone();
                atic_capture::stream::export(
                    &master,
                    &baking,
                    Export {
                        segments: vec![(0.0, duration)],
                        speeds: Vec::new(),
                        texts: Vec::new(),
                        cursor_px: size,
                        frame: None,
                        options,
                        look: Default::default(),
                        decor: Box::new(move |src, _| Decor {
                            cursor: motion.cursor(src).map(|(x, y)| (x, y, motion.cursor_scale(src))),
                            cursor_shape: cursor_shape_of(motion.cursor_shape(src)),
                            cursor_alpha: motion.cursor_alpha(src),
                            ..Decor::default()
                        }),
                        progress: Box::new(move |p| progress.store(p.to_bits(), std::sync::atomic::Ordering::Relaxed)),
                        cancel: Default::default(),
                    },
                )
            };
            let hevc = atic_capture::stream::ExportOptions {
                codec: atic_capture::stream::Codec::Hevc,
                quality: atic_capture::stream::Quality::Standard,
                max_height: Some(1080),
                ..Default::default()
            };
            let result = bake(hevc.clone()).or_else(|error| {
                tracing::warn!(%error, "grabar: sin HEVC para el cursor; se hornea en H.264");
                let _ = std::fs::remove_file(&baking);
                bake(atic_capture::stream::ExportOptions { codec: atic_capture::stream::Codec::H264, max_height: Some(1080), ..hevc })
            });
            let result = result.and_then(|()| std::fs::rename(&baking, &path).map_err(|e| e.to_string()));
            if result.is_err() {
                // Sin cursor, pero con video: el de afuera sigue siendo el original.
                let _ = std::fs::remove_file(&baking);
            }
            result
        };
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let result = cx.background_executor().spawn(async move { job() }).await;
            match &result {
                Ok(()) => println!("grabar: cursor dibujado en {:.1} s", started.elapsed().as_secs_f32()),
                Err(error) => tracing::warn!(%error, "grabar: no se pudo dibujar el cursor"),
            }
            let _ = this.update(cx, |pill, cx| {
                pill.screen_rec.baking = None;
                pill.rec_clock = None;
                if result.is_err() {
                    pill.screen_rec.warn(Notice::CursorLost);
                }
                pill.screen_rec.show_notices();
                match saved.thumb {
                    Some(frame) => pill.shelve_video(saved.path, frame, from, cx),
                    None => open_video(saved.path),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// El video al estante; la miniatura se codifica en otro hilo (un PNG de
    /// pantalla completa tarda).
    fn shelve_video(&mut self, path: PathBuf, frame: Frame, from: Rect, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let png = cx.background_executor().spawn({
                let frame = frame.clone();
                let path = path.clone();
                async move {
                    save_thumb(&path, &frame);
                    frame.to_png()
                }
            });
            match png.await {
                Ok(png) => {
                    let saved = crate::capture::Saved {
                        path,
                        png,
                        width: frame.width(),
                        height: frame.height(),
                        frame,
                    };
                    let _ = this.update(cx, |pill, cx| {
                        pill.show_shelf(saved, from);
                        cx.notify();
                    });
                }
                Err(error) => {
                    eprintln!("grabar: sin miniatura ({error})");
                    open_video(path);
                }
            }
        })
        .detach();
    }
}

/// Sin miniatura, el video se abre. En otro hilo: `ShellExecuteW` despacha
/// mensajes mientras abre, y GPUI corría tareas con la pill tomada (pánico).
fn open_video(path: PathBuf) {
    std::thread::spawn(move || {
        if let Err(error) = crate::launcher::shell_open(&path.to_string_lossy()) {
            eprintln!("grabar: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opciones_sin_audio_no_piden_pistas() {
        let none = RecordOptions { mic: false, system: false, show_pill: false, smooth_cursor: false };
        assert!(none.tracks().is_none());
        let mic = RecordOptions { mic: true, system: false, show_pill: true, smooth_cursor: false };
        let tracks = mic.tracks().unwrap();
        assert!(tracks.mic && !tracks.system);
    }

    #[test]
    fn los_avisos_se_muestran_uno_tras_otro_y_se_acaban() {
        // Un aviso: 4 s y fuera.
        assert_eq!(notice_slot(0, 1), Some(0));
        assert_eq!(notice_slot(3, 1), Some(0));
        assert_eq!(notice_slot(4, 1), None);
        // Dos: el primero 4 s, el segundo otros 4.
        assert_eq!(notice_slot(3, 2), Some(0));
        assert_eq!(notice_slot(4, 2), Some(1));
        assert_eq!(notice_slot(7, 2), Some(1));
        assert_eq!(notice_slot(8, 2), None);
        // Sin avisos no hay nada que mostrar.
        assert_eq!(notice_slot(0, 0), None);
    }

    #[test]
    fn un_aviso_no_se_repite() {
        let mut rec = ScreenRecording::default();
        rec.warn(Notice::CursorLost);
        rec.warn(Notice::CursorLost);
        rec.warn(Notice::AudioLost { kept: true });
        assert_eq!(rec.notices, vec![Notice::CursorLost, Notice::AudioLost { kept: true }]);
        // Sin mostrarlos todavía, el tab no dice nada.
        assert_eq!(rec.clock(), None);
        rec.show_notices();
        assert!(rec.notice_since.is_some());
    }

    fn monitor(id: &str, x: i32, y: i32, width: u32, height: u32) -> atic_capture::MonitorInfo {
        let bounds = PhysRect::new(x, y, width, height);
        atic_capture::MonitorInfo { id: id.into(), bounds, work_area: bounds, is_primary: x == 0, scale: 1.0 }
    }

    #[test]
    fn una_grabacion_que_cruza_pantallas_se_queda_en_la_que_mas_cubre() {
        let monitors = [monitor("a", 0, 0, 1920, 1080), monitor("b", 1920, 0, 2560, 1440)];
        // Una zona dentro de una pantalla no cambia.
        let inside = PhysRect::new(100, 100, 800, 600);
        assert_eq!(one_screen(inside, &monitors), inside);
        // Cruzando: se queda con la parte de la pantalla que más cubre (acá la «b»:
        // 480 px contra los 420 que caen en la «a»).
        let crossing = PhysRect::new(1500, 100, 900, 600);
        assert_eq!(one_screen(crossing, &monitors), PhysRect::new(1920, 100, 480, 600));
        // El escritorio entero: la pantalla de 2560×1440 tiene más área, así que se
        // queda con ella entera.
        let both = PhysRect::new(0, 0, 4480, 1440);
        assert_eq!(one_screen(both, &monitors), PhysRect::new(1920, 0, 2560, 1440));
        // Sin pantallas no se toca nada.
        assert_eq!(one_screen(crossing, &[]), crossing);
    }

    #[test]
    fn opciones_viejas_o_a_medias_toman_lo_de_siempre() {
        let partial: RecordOptions = serde_json::from_str(r#"{"mic":false}"#).unwrap();
        assert_eq!(partial, RecordOptions { mic: false, ..RecordOptions::default() });
    }
}
