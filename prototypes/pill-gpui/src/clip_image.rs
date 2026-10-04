//! Una imagen al portapapeles como la entienden todas las apps.
//!
//! GPUI la escribe solo como PNG y Paint, Word y compañía esperan un bitmap
//! (`CF_DIB`). Aquí van los dos: el DIB para esas y el PNG (formato registrado
//! «PNG», con alfa) para las que lo prefieren, como Chrome o Slack.

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::Graphics::Gdi::{BITMAPINFOHEADER, BI_RGB};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_DIB: u32 = 8;

pub fn write(width: u32, height: u32, bgra: &[u8], png: &[u8]) -> Result<(), String> {
    let stride = width as usize * 4;
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        // Positivo: el DIB va de abajo hacia arriba.
        biHeight: height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (stride * height as usize) as u32,
        ..Default::default()
    };
    let mut dib = Vec::with_capacity(header.biSize as usize + bgra.len());
    // SAFETY: BITMAPINFOHEADER es `repr(C)` sin relleno.
    dib.extend_from_slice(unsafe {
        std::slice::from_raw_parts(
            &header as *const BITMAPINFOHEADER as *const u8,
            std::mem::size_of::<BITMAPINFOHEADER>(),
        )
    });
    for row in bgra.chunks_exact(stride).rev() {
        dib.extend_from_slice(row);
    }

    unsafe {
        // Otra app puede tenerlo abierto un instante.
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !opened {
            return Err("el portapapeles está ocupado".into());
        }
        let result = (|| {
            EmptyClipboard().map_err(|error| error.to_string())?;
            set(CF_DIB, &dib)?;
            set(RegisterClipboardFormatW(w!("PNG")), png)
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Copia `bytes` a memoria global y se la entrega al portapapeles, que pasa a
/// ser su dueño.
unsafe fn set(format: u32, bytes: &[u8]) -> Result<(), String> {
    let memory: HGLOBAL =
        GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|error| error.to_string())?;
    let target = GlobalLock(memory) as *mut u8;
    if target.is_null() {
        return Err("GlobalLock falló".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
    let _ = GlobalUnlock(memory);
    SetClipboardData(format, Some(HANDLE(memory.0)))
        .map(|_| ())
        .map_err(|error| error.to_string())
}
