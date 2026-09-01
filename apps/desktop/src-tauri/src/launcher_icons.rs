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
    // No cachear fallos: un .lnk cuyo destino no resolvió (red desconectada,
    // placeholder de OneDrive) puede funcionar en un intento posterior.
    if value.is_some() {
        cache().lock_or_recover().insert(key, value.clone());
    }
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

/// Corre `f` con COM inicializado en este hilo (pool bloqueante de Tauri o
/// hilos de indexado, que pueden no tenerlo). S_OK/S_FALSE (≥ 0) se balancean
/// con CoUninitialize; RPC_E_CHANGED_MODE (< 0) significa que el hilo ya
/// tiene COM en otro modo y se usa tal cual, sin uninit.
#[cfg(windows)]
pub(crate) fn with_com<T>(f: impl FnOnce() -> T) -> T {
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };

    unsafe {
        let hr = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        let out = f();
        if hr >= 0 {
            CoUninitialize();
        }
        out
    }
}

#[cfg(windows)]
fn extract_windows(path: &Path) -> Option<String> {
    // SHGetFileInfoW exige COM inicializado.
    with_com(|| extract_windows_inner(path))
}

#[cfg(windows)]
fn extract_windows_inner(path: &Path) -> Option<String> {
    use std::ffi::OsStr;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
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

        // Icono monocromo: hbmMask es 1bpp y de doble altura (AND + XOR);
        // leerlo como BGRA de altura simple produce basura. Mejor sin bitmap:
        // el frontend cae al icono Lucide.
        let hbm = icon_info.hbmColor;
        if hbm.is_null() {
            if !icon_info.hbmMask.is_null() {
                DeleteObject(icon_info.hbmMask as HGDIOBJ);
            }
            DestroyIcon(hicon);
            return None;
        }

        let out = bitmap_png_data_url(hbm);
        if !icon_info.hbmMask.is_null() {
            DeleteObject(icon_info.hbmMask as HGDIOBJ);
        }
        DeleteObject(icon_info.hbmColor as HGDIOBJ);
        DestroyIcon(hicon);
        out
    }
}

/// HBITMAP de 32bpp → data URL PNG. No toma posesión del handle: el llamador
/// debe pasarlo válido y liberarlo después.
#[cfg(windows)]
fn bitmap_png_data_url(hbm: windows_sys::Win32::Graphics::Gdi::HBITMAP) -> Option<String> {
    use std::mem::{size_of, zeroed};

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use image::RgbaImage;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };

    unsafe {
        let mut bmp: BITMAP = zeroed();
        if GetObjectW(
            hbm as HGDIOBJ,
            size_of::<BITMAP>() as i32,
            &mut bmp as *mut _ as *mut _,
        ) == 0
        {
            return None;
        }

        let width = bmp.bmWidth.max(1) as u32;
        let height = bmp.bmHeight.abs().max(1) as u32;

        let hdc_screen = GetDC(std::ptr::null_mut());
        if hdc_screen.is_null() {
            return None;
        }
        let hdc = CreateCompatibleDC(hdc_screen);
        if hdc.is_null() {
            ReleaseDC(std::ptr::null_mut(), hdc_screen);
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

        if lines == 0 {
            return None;
        }

        // BGRA → RGBA
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        // Bitmaps de 32bpp sin canal alfa real dejan todos los A en 0 y el
        // PNG saldría completamente transparente; forzamos opaco.
        if pixels.chunks_exact(4).all(|px| px[3] == 0) {
            for px in pixels.chunks_exact_mut(4) {
                px[3] = 255;
            }
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

/// Ídem [`icon_data_url`] para una app del AppsFolder (UWP/Store), por su
/// AppUserModelID: no hay archivo, el shell entrega el bitmap.
pub fn uwp_icon_data_url(aumid: &str) -> Option<String> {
    let key = format!("uwp:{}", aumid.to_lowercase());
    {
        let guard = cache().lock_or_recover();
        if let Some(hit) = guard.get(&key) {
            return hit.clone();
        }
    }
    let value = extract_uwp(aumid);
    // Misma regla que los .lnk: los fallos no se cachean, pueden ser transitorios.
    if value.is_some() {
        cache().lock_or_recover().insert(key, value.clone());
    }
    value
}

#[cfg(windows)]
fn extract_uwp(aumid: &str) -> Option<String> {
    with_com(|| extract_uwp_inner(aumid))
}

#[cfg(not(windows))]
fn extract_uwp(_aumid: &str) -> Option<String> {
    None
}

#[cfg(windows)]
fn extract_uwp_inner(aumid: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::System::Com::IBindCtx;
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT,
    };
    use windows_sys::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};

    if !crate::launcher::aumid_valido(aumid) {
        return None;
    }
    let parse = HSTRING::from(format!("shell:AppsFolder\\{aumid}"));
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&parse, None::<&IBindCtx>).ok()?;
        let hbm = factory
            .GetImage(SIZE { cx: 48, cy: 48 }, SIIGBF_RESIZETOFIT)
            .ok()?;
        let out = bitmap_png_data_url(hbm.0 as _);
        DeleteObject(hbm.0 as HGDIOBJ);
        out
    }
}
