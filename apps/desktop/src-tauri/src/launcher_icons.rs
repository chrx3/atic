//! Iconos de apps del launcher (Windows: Shell → PNG → data URL).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use atic_core::MutexExt;

fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Devuelve un `data:image/png;base64,…` para el path, o `None` si no hay icono.
pub fn icon_data_url(path: &Path) -> Option<String> {
    let key = path.to_string_lossy().to_lowercase();
    {
        let guard = cache().lock_or_recover();
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let value = extract(path);
    cache().lock_or_recover().insert(key, value.clone());
    value
}

#[cfg(windows)]
fn extract(path: &Path) -> Option<String> {
    extract_windows(path)
}

#[cfg(not(windows))]
fn extract(_path: &Path) -> Option<String> {
    None
}

#[cfg(windows)]
fn extract_windows(path: &Path) -> Option<String> {
    use std::ffi::OsStr;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use image::RgbaImage;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC,
        BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut info: SHFILEINFOW = zeroed();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut info,
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let hicon = info.hIcon;

        let mut icon_info: ICONINFO = zeroed();
        if GetIconInfo(hicon, &mut icon_info) == 0 {
            DestroyIcon(hicon);
            return None;
        }

        let hbm = if !icon_info.hbmColor.is_null() {
            icon_info.hbmColor
        } else {
            icon_info.hbmMask
        };
        if hbm.is_null() {
            if !icon_info.hbmMask.is_null() {
                DeleteObject(icon_info.hbmMask as HGDIOBJ);
            }
            if !icon_info.hbmColor.is_null() {
                DeleteObject(icon_info.hbmColor as HGDIOBJ);
            }
            DestroyIcon(hicon);
            return None;
        }

        let mut bmp: BITMAP = zeroed();
        if GetObjectW(
            hbm as HGDIOBJ,
            size_of::<BITMAP>() as i32,
            &mut bmp as *mut _ as *mut _,
        ) == 0
        {
            if !icon_info.hbmMask.is_null() {
                DeleteObject(icon_info.hbmMask as HGDIOBJ);
            }
            if !icon_info.hbmColor.is_null() {
                DeleteObject(icon_info.hbmColor as HGDIOBJ);
            }
            DestroyIcon(hicon);
            return None;
        }

        let width = bmp.bmWidth.max(1) as u32;
        let height = bmp.bmHeight.abs().max(1) as u32;

        let hdc_screen = GetDC(std::ptr::null_mut());
        if hdc_screen.is_null() {
            if !icon_info.hbmMask.is_null() {
                DeleteObject(icon_info.hbmMask as HGDIOBJ);
            }
            if !icon_info.hbmColor.is_null() {
                DeleteObject(icon_info.hbmColor as HGDIOBJ);
            }
            DestroyIcon(hicon);
            return None;
        }
        let hdc = CreateCompatibleDC(hdc_screen);
        if hdc.is_null() {
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
            if !icon_info.hbmMask.is_null() {
                DeleteObject(icon_info.hbmMask as HGDIOBJ);
            }
            if !icon_info.hbmColor.is_null() {
                DeleteObject(icon_info.hbmColor as HGDIOBJ);
            }
            DestroyIcon(hicon);
            return None;
        }

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            hdc,
            hbm,
            0,
            height,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        DeleteDC(hdc);
        ReleaseDC(std::ptr::null_mut(), hdc_screen);
        if !icon_info.hbmMask.is_null() {
            DeleteObject(icon_info.hbmMask as HGDIOBJ);
        }
        if !icon_info.hbmColor.is_null() {
            DeleteObject(icon_info.hbmColor as HGDIOBJ);
        }
        DestroyIcon(hicon);

        if lines == 0 {
            return None;
        }

        // BGRA → RGBA
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        let img = RgbaImage::from_raw(width, height, pixels)?;
        let mut png = Vec::new();
        {
            let mut cursor = std::io::Cursor::new(&mut png);
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut cursor, image::ImageFormat::Png)
                .ok()?;
        }
        Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
    }
}
