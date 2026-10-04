//! Lo que una ventana normal de GPUI necesita en Windows para parecerse a la
//! app: el ícono de Atic en la barra de tareas y en Alt+Tab, la marca en la
//! barra de arriba y los botones de ventana de Windows 11.
//!
//! La barra nativa se esconde (`appears_transparent` en `open_window`) y la
//! barra de arriba de cada vista pasa a ser la de la ventana. GPUI deja que
//! cualquier elemento haga de zona de arrastre o de botón de ventana
//! (`WindowControlArea`) y Windows se ocupa del resto: mover, ajustar a los
//! bordes, el menú de maximizar de Windows 11, minimizar y cerrar.

use std::sync::{Arc, Mutex, OnceLock};

use gpui::{div, img, prelude::*, px, Div, RenderImage, Stateful, Window, WindowControlArea};

use super::console::hsla;
use super::TEXT;

/// El ícono de la app: el `128x128.png` de Tauri.
const ICON_PNG: &[u8] = include_bytes!("../../assets/atic.png");
/// A qué tamaño se prepara para la barra: cerca del doble de lo que se ve, para
/// que al achicarlo no se pierda el trazo fino de la «a».
const LOGO_PX: u32 = 48;
/// Los primeros píxeles de arriba no son zona de arrastre: ahí se agarra el
/// borde para cambiar el alto de la ventana.
const TOP_EDGE: f32 = 5.0;
/// Ancho de cada botón de ventana, el de Windows 11.
const CONTROL_W: f32 = 46.0;

/// El ícono de Atic a `size` px, en RGBA.
fn icon_rgba(size: u32) -> Option<image::RgbaImage> {
    let source = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    Some(if source.width() == size {
        source
    } else {
        image::imageops::resize(&source, size, size, image::imageops::FilterType::Lanczos3)
    })
}

/// De RGBA a BGRA con el alfa directo: es lo que quieren GPUI y `CreateIcon`.
fn bgra(image: image::RgbaImage) -> Option<image::RgbaImage> {
    let (width, height) = image.dimensions();
    let mut pixels = image.into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    image::RgbaImage::from_raw(width, height, pixels)
}

/// El mosaico del ícono es casi del mismo negro que el fondo de la ventana y
/// se pierde: solo se vería la «a», como un círculo. Para la barra se mezcla
/// su negro hacia un mosaico un poco más claro, por luminosidad, para que los
/// bordes suavizados de la letra queden bien.
fn lighten_tile(mut image: image::RgbaImage) -> image::RgbaImage {
    const TILE: [f32; 3] = [42.0, 42.0, 40.0];
    const CREAM: [f32; 3] = [246.0, 246.0, 241.0];
    // El negro y el crema del ícono original.
    const BLACK_LUM: f32 = 12.0;
    const CREAM_LUM: f32 = 246.0;
    for pixel in image.pixels_mut() {
        let [r, g, b, _] = pixel.0;
        let lum = (r as f32 + g as f32 + b as f32) / 3.0;
        let t = ((lum - BLACK_LUM) / (CREAM_LUM - BLACK_LUM)).clamp(0.0, 1.0);
        for channel in 0..3 {
            pixel.0[channel] = (TILE[channel] + (CREAM[channel] - TILE[channel]) * t).round() as u8;
        }
    }
    image
}

/// La marca de Atic para la barra de arriba.
pub fn logo(size: f32) -> impl IntoElement {
    static LOGO: OnceLock<Mutex<Option<Arc<RenderImage>>>> = OnceLock::new();
    let slot = LOGO.get_or_init(|| {
        let buffer = icon_rgba(LOGO_PX).map(lighten_tile).and_then(bgra);
        Mutex::new(buffer.map(|buffer| Arc::new(RenderImage::new([image::Frame::new(buffer)]))))
    });
    let image = slot.lock().ok().and_then(|slot| slot.clone());
    match image {
        Some(image) => img(image).size(px(size)).flex_none().into_any_element(),
        // Sin el PNG decodificado, un cuadrado claro: la barra sigue entera.
        None => div()
            .size(px(size))
            .flex_none()
            .rounded(px(size * 0.26))
            .bg(hsla(0xe9e9e2))
            .into_any_element(),
    }
}

/// La parte de la barra que mueve la ventana (y la maximiza con doble clic).
/// Ocupa todo el espacio libre: los botones de la barra no pueden quedar
/// dentro, porque Windows tomaría el clic como el arrastre de la ventana.
///
/// `occlude` es lo que hace que funcione: la raíz de la vista toma el foco al
/// recibir un clic y, al hacerlo, le avisa a GPUI que el clic ya fue atendido;
/// entonces GPUI no deja que Windows mueva la ventana ni pulse los botones.
/// Tapada por este elemento, la raíz ni se entera.
pub fn drag(height: f32) -> Div {
    div()
        .flex_1()
        .mt(px(TOP_EDGE))
        .h(px(height - TOP_EDGE))
        .window_control_area(WindowControlArea::Drag)
        .occlude()
}

/// Minimizar, maximizar y cerrar, a ras de la esquina de arriba a la derecha.
/// Windows hace la acción; aquí solo se dibujan.
pub fn controls(maximized: bool, height: f32) -> impl IntoElement {
    // Los glifos de «Segoe Fluent Icons», la fuente de íconos de Windows 11.
    let max_glyph = if maximized { "\u{E923}" } else { "\u{E922}" };
    div()
        .flex()
        .flex_none()
        .h(px(height))
        .child(control("win-min", "\u{E921}", WindowControlArea::Min, height, false))
        .child(control("win-max", max_glyph, WindowControlArea::Max, height, false))
        .child(control("win-close", "\u{E8BB}", WindowControlArea::Close, height, true))
}

fn control(
    id: &'static str,
    glyph: &'static str,
    area: WindowControlArea,
    height: f32,
    danger: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(CONTROL_W))
        .h(px(height))
        .flex()
        .items_center()
        .justify_center()
        .font_family("Segoe Fluent Icons")
        .text_size(px(10.))
        .text_color(hsla(TEXT))
        .hover(move |el| {
            if danger {
                el.bg(hsla(0xc42b1c)).text_color(hsla(0xffffff))
            } else {
                el.bg(hsla(0x2a2a28))
            }
        })
        .window_control_area(area)
        .occlude()
        .child(glyph)
}

/// Lo que se le hace a la ventana una vez creada: el ícono de Atic para la
/// barra de tareas y Alt+Tab, y sin el filo que Windows 11 le pinta alrededor.
#[cfg(windows)]
pub fn setup(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIcon, SendMessageW, ICON_BIG, ICON_SMALL, WM_SETICON,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    for (size, which) in [(32u32, ICON_SMALL), (64, ICON_BIG)] {
        let Some(pixels) = icon_rgba(size).and_then(bgra) else {
            continue;
        };
        // Máscara AND vacía: el alfa de los 32 bits es el que manda.
        let mask = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
        let icon = unsafe {
            CreateIcon(None, size as i32, size as i32, 1, 32, mask.as_ptr(), pixels.as_raw().as_ptr())
        };
        if let Ok(icon) = icon {
            unsafe {
                let _ = SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(which as usize)),
                    Some(LPARAM(icon.0 as isize)),
                );
            }
        }
    }
    unsafe {
        let none = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &none as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn setup(_: &Window) {}
