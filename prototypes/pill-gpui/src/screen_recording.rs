//! Grabar la pantalla desde Capturas.
//!
//! Se mantiene apretado el atajo de Capturas y la mira pasa a elegir qué
//! grabar (`capture.rs`): una ventana, una zona arrastrada o, con Espacio, la
//! pantalla, con las opciones de `RecordOptions` (M, A y P en la mira). Tras
//! una cuenta de tres empieza; el tab muestra el punto rojo con el reloj (el de
//! Reuniones), y un clic en él o el mismo atajo la detienen. El MP4, con el
//! micrófono y el sonido del sistema (`screen_audio.rs`), queda en
//! `Videos\Atic` y se abre con el reproductor del sistema.

use std::time::Instant;

use atic_capture::Rect as PhysRect;
use gpui::Context;
use serde::{Deserialize, Serialize};

use crate::platform::screen_record;
use crate::screen_audio::Tracks;
use crate::{distance, PillShape};

/// Lo que se elige en la mira antes de grabar; se recuerda para la próxima.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordOptions {
    pub mic: bool,
    pub system: bool,
    /// La pill y la UI de Atic salen en el video (si no, se excluyen).
    pub show_pill: bool,
}

impl Default for RecordOptions {
    fn default() -> Self {
        Self {
            mic: true,
            system: true,
            show_pill: false,
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

#[derive(Default)]
pub struct ScreenRecording {
    /// Se mantuvo el atajo mientras la mira todavía congelaba: abre grabando.
    pub requested: bool,
    recording: Option<screen_record::Recording>,
}

impl ScreenRecording {
    pub fn active(&self) -> bool {
        self.recording.is_some()
    }

    /// El reloj del tab mientras graba.
    pub fn clock(&self) -> Option<String> {
        self.recording
            .as_ref()
            .filter(|recording| !recording.stopping())
            .map(|recording| crate::meetings::stopwatch(recording.elapsed()))
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
        let name = chrono::Local::now().format("Grabación %Y-%m-%d %H-%M-%S.mp4").to_string();
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(!options.show_pill);
        }
        match screen_record::start(region, dir.join(name), options.tracks()) {
            Ok(recording) => {
                println!(
                    "grabar: {}×{} en ({}, {}), {options:?}",
                    region.width, region.height, region.x, region.y
                );
                self.screen_rec.recording = Some(recording);
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
        if let Some(recording) = self.screen_rec.recording.as_ref() {
            recording.stop();
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
        match self.shape(now) {
            PillShape::Tab { edge, .. } if !edge.is_vertical() => p.0 < self.mark_center(now).0,
            _ => self.art_center(now).is_some_and(|center| distance(p, center) <= 12.0),
        }
    }

    /// En cada vuelta: el reloj del tab y, cuando el archivo quedó, abrirlo.
    pub(crate) fn poll_screen_recording(&mut self, cx: &mut Context<Self>) {
        let Some(recording) = self.screen_rec.recording.as_ref() else {
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
        match result {
            Ok(path) => {
                println!("grabar: guardada en {}", path.display());
                // En otro hilo: `ShellExecuteW` despacha mensajes mientras
                // abre, y GPUI corría tareas con la pill tomada (pánico).
                std::thread::spawn(move || {
                    if let Err(error) = crate::launcher::shell_open(&path.to_string_lossy()) {
                        eprintln!("grabar: {error}");
                    }
                });
            }
            Err(error) => eprintln!("grabar: {error}"),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opciones_sin_audio_no_piden_pistas() {
        let none = RecordOptions { mic: false, system: false, show_pill: false };
        assert!(none.tracks().is_none());
        let mic = RecordOptions { mic: true, system: false, show_pill: true };
        let tracks = mic.tracks().unwrap();
        assert!(tracks.mic && !tracks.system);
    }

    #[test]
    fn opciones_viejas_o_a_medias_toman_lo_de_siempre() {
        let partial: RecordOptions = serde_json::from_str(r#"{"mic":false}"#).unwrap();
        assert_eq!(partial, RecordOptions { mic: false, ..RecordOptions::default() });
    }
}
