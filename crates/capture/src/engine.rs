//! Captura GDI a memoria: monitor/región con `BitBlt` y ventana con
//! `PrintWindow`. Devuelve [`Frame`]s BGRA listos para recortar o codificar.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HDC,
    HGDIOBJ, SRCCOPY,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;

use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::geometry::Rect;
use crate::monitors::MonitorInfo;

/// Lienzo en memoria (DC + bitmap compatibles con la pantalla). RAII: libera
/// todos los recursos GDI al soltarse, incluso en caminos de error.
struct MemCanvas {
    screen_dc: HDC,
    mem_dc: HDC,
    bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    previous: HGDIOBJ,
    width: u32,
    height: u32,
}

impl MemCanvas {
    unsafe fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidDimensions(width, height));
        }
        // DC de toda la pantalla virtual.
        let screen_dc = GetDC(null_mut());
        if screen_dc.is_null() {
            return Err(Error::Gdi("GetDC(NULL) devolvió NULL".into()));
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            ReleaseDC(null_mut(), screen_dc);
            return Err(Error::Gdi("CreateCompatibleDC falló".into()));
        }
        let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
        if bitmap.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(null_mut(), screen_dc);
            return Err(Error::Gdi("CreateCompatibleBitmap falló".into()));
        }
        let previous = SelectObject(mem_dc, bitmap as HGDIOBJ);
        Ok(Self {
            screen_dc,
            mem_dc,
            bitmap,
            previous,
            width,
            height,
        })
    }

    /// Lee los píxeles del bitmap como BGRA top-down.
    unsafe fn read_bgra(&self) -> Result<Vec<u8>> {
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = self.width as i32;
        // Altura negativa → orden top-down (primera fila arriba).
        info.bmiHeader.biHeight = -(self.height as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;

        let mut buffer = vec![0u8; self.width as usize * self.height as usize * 4];
        let lines = GetDIBits(
            self.mem_dc,
            self.bitmap,
            0,
            self.height,
            buffer.as_mut_ptr() as *mut core::ffi::c_void,
            &mut info,
            DIB_RGB_COLORS,
        );
        if lines == 0 {
            return Err(Error::Gdi("GetDIBits no copió filas".into()));
        }
        Ok(buffer)
    }
}

impl Drop for MemCanvas {
    fn drop(&mut self) {
        // SAFETY: cada handle se creó en `new` y se libera exactamente una vez.
        unsafe {
            SelectObject(self.mem_dc, self.previous);
            DeleteObject(self.bitmap as HGDIOBJ);
            DeleteDC(self.mem_dc);
            ReleaseDC(null_mut(), self.screen_dc);
        }
    }
}

/// Captura una región del escritorio virtual (coordenadas físicas) con
/// `BitBlt`. Si `include_cursor`, dibuja el cursor sobre el frame.
pub fn capture_rect(rect: Rect, include_cursor: bool) -> Result<Frame> {
    if rect.is_empty() {
        return Err(Error::InvalidDimensions(rect.width, rect.height));
    }
    // SAFETY: `canvas` gestiona los recursos GDI; las llamadas usan handles
    // válidos y dimensiones acordes al bitmap.
    unsafe {
        let canvas = MemCanvas::new(rect.width, rect.height)?;
        let ok = BitBlt(
            canvas.mem_dc,
            0,
            0,
            rect.width as i32,
            rect.height as i32,
            canvas.screen_dc,
            rect.x,
            rect.y,
            SRCCOPY | CAPTUREBLT,
        );
        if ok == 0 {
            return Err(Error::Gdi("BitBlt falló".into()));
        }
        if include_cursor {
            draw_cursor(canvas.mem_dc, rect.x, rect.y);
        }
        let bgra = canvas.read_bgra()?;
        Ok(Frame::new(rect, bgra))
    }
}

/// Congela cada monitor a memoria de una sola vez (flujo «congelar primero»).
/// Las capturas que fallen se registran y se omiten.
pub fn freeze_monitors(monitors: &[MonitorInfo], include_cursor: bool) -> Vec<Frame> {
    monitors
        .iter()
        .filter_map(
            |monitor| match capture_rect(monitor.bounds, include_cursor) {
                Ok(frame) => Some(frame),
                Err(error) => {
                    tracing::warn!(%error, id = %monitor.id, "no se pudo congelar el monitor");
                    None
                }
            },
        )
        .collect()
}

/// Captura una ventana SOLO con `PrintWindow` (renderiza únicamente la ventana
/// objetivo, sin tocar la pantalla). Devuelve `None` si el resultado sale negro
/// o falla, para que el llamador decida el fallback.
///
/// Útil durante el overlay: `PrintWindow` no captura los overlays, y el
/// fallback (recortar del frame congelado) tampoco.
pub fn print_window(hwnd: isize) -> Result<Option<Frame>> {
    let bounds = crate::windows::window_bounds(hwnd)
        .ok_or_else(|| Error::Gdi("ventana sin límites".into()))?;
    // SAFETY: `canvas` gestiona los recursos; `hwnd` proviene de la enumeración.
    unsafe {
        let canvas = MemCanvas::new(bounds.width, bounds.height)?;
        let ok = PrintWindow(hwnd as HWND, canvas.mem_dc, PW_RENDERFULLCONTENT) != 0;
        let bgra = canvas.read_bgra()?;
        if ok && !is_black(&bgra) {
            Ok(Some(Frame::new(bounds, bgra)))
        } else {
            Ok(None)
        }
    }
}

/// Captura una ventana por su `HWND` (como entero) con `PrintWindow`.
///
/// Si `PrintWindow` falla o devuelve un frame negro (algunas apps GPU/DRM), se
/// degrada a un `BitBlt` del rectángulo de la ventana desde la pantalla.
pub fn capture_window(hwnd: isize) -> Result<Frame> {
    match print_window(hwnd)? {
        Some(frame) => Ok(frame),
        None => {
            let bounds = crate::windows::window_bounds(hwnd)
                .ok_or_else(|| Error::Gdi("ventana sin límites".into()))?;
            capture_rect(bounds, false)
        }
    }
}

/// `true` si todos los píxeles son (casi) negros; corta al primer no-negro.
fn is_black(bgra: &[u8]) -> bool {
    const THRESHOLD: u8 = 8;
    !bgra.iter().any(|&byte| byte > THRESHOLD)
}

/// Dibuja el cursor del sistema sobre el DC, en coordenadas relativas al origen
/// del frame. Best-effort: si algo falla, no dibuja nada.
unsafe fn draw_cursor(dc: HDC, origin_x: i32, origin_y: i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, ICONINFO,
    };

    let mut cursor: CURSORINFO = std::mem::zeroed();
    cursor.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
    if GetCursorInfo(&mut cursor) == 0 || cursor.flags & CURSOR_SHOWING == 0 {
        return;
    }

    let mut icon: ICONINFO = std::mem::zeroed();
    if GetIconInfo(cursor.hCursor, &mut icon) == 0 {
        return;
    }
    let x = cursor.ptScreenPos.x - origin_x - icon.xHotspot as i32;
    let y = cursor.ptScreenPos.y - origin_y - icon.yHotspot as i32;
    DrawIconEx(dc, x, y, cursor.hCursor, 0, 0, 0, null_mut(), DI_NORMAL);

    // Los bitmaps devueltos por GetIconInfo son responsabilidad del llamador.
    if !icon.hbmColor.is_null() {
        DeleteObject(icon.hbmColor as HGDIOBJ);
    }
    if !icon.hbmMask.is_null() {
        DeleteObject(icon.hbmMask as HGDIOBJ);
    }
}
