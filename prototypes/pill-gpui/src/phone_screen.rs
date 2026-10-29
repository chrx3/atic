//! La pantalla del celular en el PC: cuando la app de Android comparte su pantalla
//! (`atic_sync::video`), se abre una ventana que la muestra.
//!
//! El canal (`phone.rs`) deja cada video que llega en `INCOMING`; la pill lo abre en
//! su próximo cuadro (`take_incoming`, con la ventana a mano). Cada ventana decodifica
//! en su propio hilo (OpenH264, `atic_capture::h264`) y pinta solo el último cuadro:
//! si la decodificación se atrasa, salta. Cerrar la ventana le pide al celular que pare.
//!
//! `ATIC_SCREEN_TEST=<archivo.h264>` abre la ventana con un video de archivo, como si
//! llegara del celular (la pill de desarrollo no abre el canal: así se prueba la vista).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atic_capture::h264::Decoder;
use atic_sync::video::{flags, VideoControl, VideoFrame, VideoLink};
use futures::StreamExt;
use gpui::{div, img, prelude::*, px, size, App, Bounds, Context, ObjectFit, RenderImage, SharedString, Window, WindowHandle};

use crate::i18n::t;
use crate::kit::chrome;

/// Alto de la barra de la ventana.
const TOP_H: f32 = 38.;

/// Los videos que llegaron y todavía no tienen ventana.
static INCOMING: Mutex<Vec<VideoLink>> = Mutex::new(Vec::new());

/// Quien recibe el video en el canal (`Desktop::on_video`): lo deja para la pill.
pub fn accept(link: VideoLink) -> Result<(), String> {
    INCOMING.lock().unwrap_or_else(|e| e.into_inner()).push(link);
    Ok(())
}

/// Lo que llegó desde el último cuadro (la pill abre una ventana por cada uno).
pub fn take_incoming() -> Vec<VideoLink> {
    std::mem::take(&mut *INCOMING.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Con `ATIC_SCREEN_TEST`, un video de archivo como si llegara del celular.
pub fn test_link() -> Option<VideoLink> {
    let path = std::env::var_os("ATIC_SCREEN_TEST")?;
    let bytes = std::fs::read(path).ok()?;
    let (tx, frames) = tokio::sync::mpsc::channel(4);
    let (control, _) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        // Las unidades NAL de a una, a 30 cuadros por segundo, en bucle.
        loop {
            let mut pts = 0u64;
            for nal in split_annex_b(&bytes) {
                let kind = nal.get(4).map(|b| b & 0x1f).unwrap_or(0);
                let flag = match kind {
                    7 | 8 => flags::CONFIG,
                    5 => flags::KEY,
                    _ => 0,
                };
                if tx.blocking_send(VideoFrame { flags: flag, pts_us: pts, data: nal.to_vec() }).is_err() {
                    return;
                }
                if kind == 1 || kind == 5 {
                    pts += 33_333;
                    std::thread::sleep(Duration::from_millis(33));
                }
            }
        }
    });
    let hello = atic_sync::video::VideoHello {
        source: atic_sync::video::VideoSource::Screen,
        direction: atic_sync::video::VideoDirection::Send,
        codec: "h264".into(),
        width: 720,
        height: 1600,
        rotation: 0,
    };
    Some(VideoLink { device_id: "prueba".into(), hello, frames, control })
}

/// Las unidades NAL de un Annex-B, cada una con su prefijo `00 00 00 01`.
fn split_annex_b(bytes: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == 0 && bytes[i + 1] == 0 && bytes[i + 2] == 0 && bytes[i + 3] == 1 {
            starts.push(i);
            i += 4;
        } else {
            i += 1;
        }
    }
    starts.iter().enumerate().map(|(n, &s)| &bytes[s..starts.get(n + 1).copied().unwrap_or(bytes.len())]).collect()
}

/// Abre la ventana de un video que llegó.
pub fn open_window(link: VideoLink, cx: &mut App) -> anyhow::Result<WindowHandle<PhoneScreen>> {
    let shape = window_size(link.hello.width, link.hello.height);
    let options = gpui::WindowOptions {
        titlebar: Some(chrome::titlebar(t("pill.phoneScreen.title"), TOP_H)),
        window_min_size: Some(size(px(200.), px(300.))),
        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(None, shape, cx))),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        ..Default::default()
    };
    Ok(cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| PhoneScreen::new(link, cx))
    })?)
}

/// El tamaño de la ventana para un cuadro de `w × h`: vertical, del alto de un celular
/// en pantalla; horizontal, de un ancho cómodo. Con su proporción y la barra arriba.
fn window_size(w: u32, h: u32) -> gpui::Size<gpui::Pixels> {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    if h >= w {
        let height = 820f32;
        size(px(((height - TOP_H) * w / h).max(240.)), px(height))
    } else {
        let width = 900f32;
        size(px(width), px(width * h / w + TOP_H))
    }
}

/// El último cuadro decodificado, que deja el hilo y toma la ventana.
type Latest = Arc<Mutex<Option<atic_capture::h264::Frame>>>;

pub struct PhoneScreen {
    image: Option<Arc<RenderImage>>,
    /// Los cuadros reemplazados: se sueltan de la GPU en el próximo cuadro.
    retired: Vec<Arc<RenderImage>>,
    control: tokio::sync::mpsc::UnboundedSender<VideoControl>,
    status: Option<SharedString>,
    /// Las medidas del último cuadro: si cambian (el celular giró), la ventana toma la
    /// forma nueva en el próximo cuadro.
    shape: Option<(u32, u32)>,
    reshape: bool,
}

impl PhoneScreen {
    fn new(link: VideoLink, cx: &mut Context<Self>) -> Self {
        let VideoLink { frames, control, .. } = link;
        let latest: Latest = Arc::default();
        let (wake, mut woken) = futures::channel::mpsc::unbounded::<Result<(), String>>();
        let thread_latest = latest.clone();
        let thread_control = control.clone();
        std::thread::Builder::new()
            .name("atic-pantalla-celular".into())
            .spawn(move || decode_loop(frames, thread_latest, thread_control, wake))
            .ok();
        cx.spawn(async move |this, cx| {
            while let Some(event) = woken.next().await {
                let frame = latest.lock().unwrap_or_else(|e| e.into_inner()).take();
                let alive = this.update(cx, |view, cx| {
                    match event {
                        Ok(()) => {
                            if let Some(frame) = frame {
                                view.show(frame);
                            }
                        }
                        Err(reason) => view.status = Some(reason.into()),
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
            // El celular dejó de mandar.
            let _ = this.update(cx, |view, cx| {
                if view.status.is_none() {
                    view.status = Some(t("pill.phoneScreen.ended").into());
                }
                cx.notify();
            });
        })
        .detach();
        Self { image: None, retired: Vec::new(), control, status: None, shape: None, reshape: false }
    }

    fn show(&mut self, frame: atic_capture::h264::Frame) {
        let shape = (frame.width, frame.height);
        if self.shape.is_some_and(|old| old != shape) {
            self.reshape = true;
        }
        self.shape = Some(shape);
        if let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra) {
            let image = Arc::new(RenderImage::new([image::Frame::new(buffer)]));
            if let Some(old) = self.image.replace(image) {
                self.retired.push(old);
            }
        }
    }
}

impl Drop for PhoneScreen {
    fn drop(&mut self) {
        // Cerrar la ventana: el celular deja de mandar.
        let _ = self.control.send(VideoControl::Stop);
    }
}

/// El hilo de la ventana: decodifica lo que llega y deja el último cuadro. Un cuadro
/// dañado pide un keyframe (y se espera ese para seguir).
fn decode_loop(
    mut frames: tokio::sync::mpsc::Receiver<VideoFrame>,
    latest: Latest,
    control: tokio::sync::mpsc::UnboundedSender<VideoControl>,
    wake: futures::channel::mpsc::UnboundedSender<Result<(), String>>,
) {
    let mut decoder = match Decoder::new() {
        Ok(decoder) => decoder,
        Err(error) => {
            let _ = wake.unbounded_send(Err(error));
            return;
        }
    };
    while let Some(frame) = frames.blocking_recv() {
        if frame.flags & flags::FORMAT != 0 {
            continue;
        }
        match decoder.decode(&frame.data) {
            Ok(Some(decoded)) => {
                *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(decoded);
                if wake.unbounded_send(Ok(())).is_err() {
                    break;
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(%error, "pantalla del celular: cuadro dañado");
                let _ = control.send(VideoControl::RequestKeyframe);
            }
        }
    }
}

impl Render for PhoneScreen {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        for old in self.retired.drain(..) {
            let _ = window.drop_image(old);
        }
        // El celular giró: la ventana toma la forma del cuadro (vertical, del alto de un
        // celular; horizontal, más ancha que alta).
        if let (true, Some((w, h))) = (std::mem::take(&mut self.reshape), self.shape) {
            window.resize(window_size(w, h));
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::black())
            // La barra: `drag` llena una fila, así que va en una de alto fijo.
            .child(div().flex_none().h(px(TOP_H)).flex().child(chrome::drag(TOP_H)))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    .when_some(self.image.clone(), |el, image| el.child(img(image).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Contain)))
                    .when(self.image.is_none() || self.status.is_some(), |el| {
                        let text: SharedString = self.status.clone().unwrap_or_else(|| t("pill.phoneScreen.waiting").into());
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .px(px(24.))
                                .text_center()
                                .text_size(px(13.))
                                .text_color(crate::theme::hsla(crate::theme::MUTED))
                                .child(text),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_las_unidades_nal() {
        let bytes = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 5];
        let nals = split_annex_b(&bytes);
        assert_eq!(nals.len(), 3);
        assert_eq!(nals[0], &[0, 0, 0, 1, 0x67, 1, 2]);
        assert_eq!(nals[2], &[0, 0, 0, 1, 0x65, 4, 5]);
    }
}

// --- La pantalla del PC en el celular --------------------------------------------------

/// Cuadros por segundo de la pantalla del PC, como mucho. Si capturar y codificar
/// tardan más, o la red no da, sale a lo que alcance.
const SHARE_FPS: f32 = 30.;
/// El bitrate de partida y el tope: la conexión lo baja según la red (`atic_sync::video`).
const SHARE_BITRATE: u32 = 4_000_000;

/// Quien comparte la pantalla del PC (`Desktop::on_screen_request`): la pantalla
/// principal, capturada y codificada en su propio hilo, mientras el celular la mire.
pub fn share(request: atic_sync::video::ScreenRequest) -> Result<atic_sync::video::ScreenSource, String> {
    let monitor = atic_capture::monitors::enumerate().into_iter().find(|m| m.is_primary).ok_or("No se encontró la pantalla del PC.")?;
    // Una primera captura dice si hay permiso (en macOS, Grabación de pantalla).
    if atic_capture::engine::capture_monitor_for_stream(&monitor).is_none() {
        return Err(t("pill.phoneScreen.noCapture").to_string());
    }
    let (frames_tx, frames) = tokio::sync::mpsc::channel(4);
    let (control, mut asked) = tokio::sync::mpsc::unbounded_channel();
    tracing::info!(equipo = %request.device_id, ancho = request.max_width, "compartiendo la pantalla del PC con el celular");
    std::thread::Builder::new()
        .name("atic-compartir-pantalla".into())
        .spawn(move || {
            let mut encoder = atic_capture::h264::ScreenEncoder::new(request.max_width, SHARE_BITRATE, SHARE_FPS);
            let started = std::time::Instant::now();
            let period = Duration::from_secs_f32(1. / SHARE_FPS);
            loop {
                let tick = std::time::Instant::now();
                loop {
                    match asked.try_recv() {
                        Ok(VideoControl::RequestKeyframe) => encoder.request_keyframe(),
                        Ok(VideoControl::SetBitrate { bps }) => encoder.set_bitrate(bps),
                        Ok(VideoControl::Stop) | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                            tracing::info!("el celular dejó de mirar la pantalla del PC");
                            return;
                        }
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    }
                }
                // La conexión va atrasada (su cola está llena): este cuadro no se captura.
                // Así no se acumula y el video baja de cuadros por segundo sin atrasarse.
                if frames_tx.capacity() == 0 {
                    std::thread::sleep(period);
                    continue;
                }
                // En macOS, a resolución nominal: cuesta un tercio que la completa.
                let Some(frame) = atic_capture::engine::capture_monitor_for_stream(&monitor) else {
                    std::thread::sleep(period);
                    continue;
                };
                let pts_us = started.elapsed().as_micros() as u64;
                match encoder.encode(&frame.bgra, frame.width(), frame.height()) {
                    Ok(out) => {
                        for (flags, data) in out {
                            // Hay lugar (se miró arriba); si no, espera un poco: el FORMAT y su
                            // keyframe van juntos.
                            if frames_tx.blocking_send(VideoFrame { flags, pts_us, data }).is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "no se pudo codificar la pantalla del PC");
                        return;
                    }
                }
                if let Some(rest) = period.checked_sub(tick.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(atic_sync::video::ScreenSource { frames, control, max_bitrate: SHARE_BITRATE })
}
