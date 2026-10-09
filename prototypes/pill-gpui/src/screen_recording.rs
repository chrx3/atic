//! Grabar la pantalla desde Capturas.
//!
//! Se mantiene apretado el atajo de Capturas y la mira pasa a elegir qué
//! grabar (`capture.rs`): una ventana, una zona arrastrada o, con Enter, la
//! pantalla. Mientras graba, el tab muestra el punto rojo con el reloj (el de
//! Reuniones) y el mismo atajo la detiene. La pill queda excluida de las
//! capturas para no salir en el video. El MP4, con el micrófono y el sonido del
//! sistema (`screen_audio.rs`), queda en `Videos\Atic` y se abre con el
//! reproductor del sistema.

use atic_capture::Rect as PhysRect;
use gpui::Context;

use crate::platform::screen_record;
use crate::screen_audio::Tracks;

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

    pub(crate) fn start_screen_recording(&mut self, region: PhysRect, cx: &mut Context<Self>) {
        let Some(dir) = screen_record::videos_dir() else {
            eprintln!("grabar: no se encontró la carpeta de Videos");
            return;
        };
        let name = chrono::Local::now().format("Grabación %Y-%m-%d %H-%M-%S.mp4").to_string();
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(true);
        }
        // Por ahora siempre con micrófono y sonido del sistema; elegirlos en la
        // mira viene después.
        let audio = Some(Tracks { mic: true, system: true });
        match screen_record::start(region, dir.join(name), audio) {
            Ok(recording) => {
                println!("grabar: {}×{} en ({}, {})", region.width, region.height, region.x, region.y);
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
