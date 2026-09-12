//! Captura Core Graphics: `CGDisplayCreateImage` por monitor y
//! `CGWindowListCreateImage` para una ventana.
//!
//! Las regiones llegan en **puntos** (el espacio global de Quartz). La salida
//! de un solo monitor es nativa (2880 px en Retina); cuando la región cruza
//! monitores con escalas distintas, cada display se reescala a la mayor para
//! que el frame sea una grilla única y las coordenadas no mientan.

use std::ffi::c_void;

use core_graphics::access::ScreenCaptureAccess;
use core_graphics::base::{kCGBitmapByteOrder32Little, kCGImageAlphaPremultipliedFirst};
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::{CGContext, CGInterpolationQuality};
use core_graphics::display::CGDisplay;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::image::CGImage;
use core_graphics::window::{
    self, kCGWindowImageBestResolution, kCGWindowImageBoundsIgnoreFraming,
    kCGWindowImageShouldBeOpaque, kCGWindowListOptionIncludingWindow,
};

use super::is_black;
use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::geometry::Rect;
use crate::monitors::MonitorInfo;

/// Captura una región del escritorio virtual (coordenadas en puntos).
///
/// `include_cursor` se ignora: Core Graphics no pinta el puntero en el frame
/// y componer `NSCursor` queda para más adelante.
pub fn capture_rect(rect: Rect, _include_cursor: bool) -> Result<Frame> {
    ensure_access();
    capture_rect_inner(rect)
}

/// En Mac no hay analogía de `CAPTUREBLT` / ventanas layered: es la misma
/// foto de pantalla. Quien no quiere las flotantes de Atic tiene que
/// esconderlas antes, igual que en el freeze de la mira.
pub fn capture_rect_without_layered(rect: Rect) -> Result<Frame> {
    capture_rect(rect, false)
}

fn capture_rect_inner(rect: Rect) -> Result<Frame> {
    if rect.is_empty() {
        return Err(Error::InvalidDimensions(rect.width, rect.height));
    }

    let Ok(ids) = CGDisplay::active_displays() else {
        return Err(Error::Capture(
            "no se pudieron enumerar las pantallas".into(),
        ));
    };
    let monitors = crate::monitors::enumerate();
    if ids.len() != monitors.len() {
        tracing::warn!(
            displays = ids.len(),
            monitors = monitors.len(),
            "la enumeración de displays y de monitores no coincide"
        );
    }

    // Grilla de salida: la escala del display más denso que toque la región.
    // Un solo monitor queda a resolución nativa; una región mixta se compone
    // en la escala mayor para que cada píxel signifique lo mismo.
    let mut out_scale = 1.0_f64;
    let mut any_target = false;
    for (i, id) in ids.iter().enumerate() {
        let point_bounds = point_bounds_of(*id, monitors.get(i));
        if point_bounds.intersection(&rect).is_some() {
            out_scale = out_scale.max(native_scale_of(CGDisplay::new(*id), &point_bounds));
            any_target = true;
        }
    }
    if !any_target {
        return Err(Error::Capture("la región no toca ninguna pantalla".into()));
    }

    let canvas_bounds = scale_rect(rect, out_scale);
    let mut canvas = Frame::new(
        canvas_bounds,
        vec![0u8; canvas_bounds.width as usize * canvas_bounds.height as usize * 4],
    );
    let mut any = false;

    for (i, id) in ids.into_iter().enumerate() {
        let display = CGDisplay::new(id);
        let point_bounds = point_bounds_of(id, monitors.get(i));
        let Some(inter) = point_bounds.intersection(&rect) else {
            continue;
        };
        let Some(image) = display.image() else {
            tracing::warn!(id, "CGDisplayCreateImage devolvió NULL");
            continue;
        };
        let (width, height, bgra) = cgimage_to_bgra(&image)?;
        let native = if point_bounds.width > 0 {
            width as f64 / f64::from(point_bounds.width)
        } else {
            1.0
        };
        let target = scale_rect(point_bounds, native);
        let native_frame = Frame::new(Rect::new(target.x, target.y, width, height), bgra);
        let Some(cropped) = native_frame.crop(scale_rect(inter, native)) else {
            continue;
        };
        canvas.blend_over_scaled(&cropped, scale_rect(inter, out_scale));
        any = true;
    }

    let permission_missing = missing_permission();
    if !any {
        return Err(if permission_missing {
            Error::Permission
        } else {
            Error::Capture("no se pudo capturar ningún monitor de la región".into())
        });
    }
    if is_black(&canvas.bgra) {
        return Err(if permission_missing {
            Error::Permission
        } else {
            Error::Capture("la captura salió negra".into())
        });
    }
    // Desde macOS 14, sin permiso la captura no sale negra: sale el fondo de
    // pantalla sin ventanas ajenas (y `CGWindowListCopyWindowInfo` tampoco las
    // lista). Si el preflight falla y no hay ninguna ventana de otro proceso
    // en pantalla, el diagnóstico es permiso, no una captura vacía.
    if permission_missing && !foreign_windows_visible(&monitors) {
        return Err(Error::Permission);
    }
    Ok(canvas)
}

fn point_bounds_of(display_id: u32, monitor: Option<&MonitorInfo>) -> Rect {
    monitor
        .map(|m| m.bounds)
        .unwrap_or_else(|| display_rect(CGDisplay::new(display_id)))
}

fn display_rect(display: CGDisplay) -> Rect {
    let bounds = display.bounds();
    Rect::new(
        bounds.origin.x.round() as i32,
        bounds.origin.y.round() as i32,
        bounds.size.width.round().max(0.0) as u32,
        bounds.size.height.round().max(0.0) as u32,
    )
}

fn native_scale_of(display: CGDisplay, point_bounds: &Rect) -> f64 {
    if point_bounds.width == 0 {
        return 1.0;
    }
    let pixel_w = display
        .display_mode()
        .map(|m| m.pixel_width())
        .filter(|&w| w > 0)
        .unwrap_or_else(|| display.pixels_wide());
    let scale = pixel_w as f64 / f64::from(point_bounds.width);
    if scale.is_finite() && scale > 0.1 {
        scale
    } else {
        1.0
    }
}

/// Escala un rect en puntos (origen global incluido) a píxeles.
fn scale_rect(rect: Rect, scale: f64) -> Rect {
    Rect::new(
        (f64::from(rect.x) * scale).round() as i32,
        (f64::from(rect.y) * scale).round() as i32,
        (f64::from(rect.width) * scale).round().max(1.0) as u32,
        (f64::from(rect.height) * scale).round().max(1.0) as u32,
    )
}

/// Captura una ventana recortada a su marco visible.
pub fn capture_window_visual(hwnd: isize) -> Result<Frame> {
    if let Ok(Some(frame)) = print_window(hwnd) {
        return Ok(frame);
    }
    let visual = crate::windows::window_bounds(hwnd)
        .ok_or_else(|| Error::Capture("ventana sin límites".into()))?;
    capture_rect_without_layered(visual)
}

/// Captura solo esa ventana (`CGWindowListCreateImage`). `None` si sale
/// negra o falla, para que el llamador recorte del frame congelado.
pub fn print_window(hwnd: isize) -> Result<Option<Frame>> {
    ensure_access();
    let window_id =
        u32::try_from(hwnd).map_err(|_| Error::Capture("id de ventana inválido".into()))?;
    let Some(bounds) = crate::windows::window_bounds(hwnd) else {
        return Err(if missing_permission() {
            Error::Permission
        } else {
            Error::Capture("ventana sin límites".into())
        });
    };
    if bounds.is_empty() {
        return Ok(None);
    }
    let image = window::create_image(
        cg_rect_null(),
        kCGWindowListOptionIncludingWindow,
        window_id,
        kCGWindowImageBoundsIgnoreFraming
            | kCGWindowImageBestResolution
            | kCGWindowImageShouldBeOpaque,
    );
    let Some(image) = image else {
        return Ok(None);
    };
    let (width, height, bgra) = cgimage_to_bgra(&image)?;
    if is_black(&bgra) {
        return Ok(None);
    }
    // El marco llega en puntos y la imagen en píxeles nativos: se ancla al
    // origen nativo para que el frame sea autoconsistente.
    let native = if bounds.width > 0 {
        width as f64 / f64::from(bounds.width)
    } else {
        1.0
    };
    let target = scale_rect(bounds, native);
    Ok(Some(Frame::new(
        Rect::new(target.x, target.y, width, height),
        bgra,
    )))
}

/// En Mac no hay árbol de HWNDs: es `print_window`.
pub fn print_window_tree(hwnd: isize) -> Result<Option<Frame>> {
    print_window(hwnd)
}

/// Captura una ventana por su id. Si `print_window` falla, recorta de la
/// pantalla (puede incluir lo que tenga encima).
pub fn capture_window(hwnd: isize) -> Result<Frame> {
    match print_window(hwnd)? {
        Some(frame) => Ok(frame),
        None => {
            let bounds = crate::windows::window_bounds(hwnd)
                .ok_or_else(|| Error::Capture("ventana sin límites".into()))?;
            capture_rect(bounds, false)
        }
    }
}

/// Pide el permiso de grabación de pantalla una vez por ejecución.
///
/// No corta cuando `request()` devuelve false: el diálogo TCC recién
/// apareció y el proceso todavía no lo tiene. La decisión de reportar
/// `Error::Permission` la toman el contenido capturado y `missing_permission`.
fn ensure_access() {
    let access = ScreenCaptureAccess;
    if access.preflight() {
        return;
    }
    let _ = access.request();
}

fn missing_permission() -> bool {
    !ScreenCaptureAccess.preflight()
}

/// ¿Hay ventanas de otros procesos en pantalla? Sin permiso de grabación,
/// macOS 14+ solo lista las propias, así que un inventario vacío delata el
/// TCC denegado (o un escritorio realmente pelado, el mismo remedio).
fn foreign_windows_visible(monitors: &[MonitorInfo]) -> bool {
    let pid = std::process::id();
    !crate::windows::enumerate_candidates(pid, monitors).is_empty()
}

fn cg_rect_null() -> CGRect {
    CGRect {
        origin: CGPoint {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    }
}

/// Dibuja el `CGImage` en un bitmap BGRA top-down (primera fila arriba).
fn cgimage_to_bgra(image: &CGImage) -> Result<(u32, u32, Vec<u8>)> {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return Err(Error::InvalidDimensions(width as u32, height as u32));
    }
    let bytes_per_row = width * 4;
    let mut buffer = vec![0u8; bytes_per_row * height];
    let color_space = CGColorSpace::create_device_rgb();
    let bitmap_info = kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little;
    let ctx = CGContext::create_bitmap_context(
        Some(buffer.as_mut_ptr() as *mut c_void),
        width,
        height,
        8,
        bytes_per_row,
        &color_space,
        bitmap_info,
    );
    // Sin voltear: el buffer del `CGBitmapContext` ya es top-down y
    // `CGDisplayCreateImage` entrega la imagen en ese mismo orden. El
    // `translate/scale` clásico la dejaba espejada verticalmente (capturas y
    // pizarra al revés).
    ctx.set_interpolation_quality(CGInterpolationQuality::CGInterpolationQualityNone);
    ctx.draw_image(
        CGRect::new(
            &CGPoint::new(0.0, 0.0),
            &CGSize::new(width as f64, height as f64),
        ),
        image,
    );
    ctx.flush();
    drop(ctx);
    for px in buffer.chunks_exact_mut(4) {
        px[3] = 255;
    }
    Ok((width as u32, height as u32, buffer))
}
