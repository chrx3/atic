//! Íconos de apps: de un archivo (`.exe`, `.lnk`) o de una app de Store por su
//! AppUserModelID. Mismo camino que `launcher_icons.rs` de Atic, pero sin pasar
//! por PNG: GPUI dibuja BGRA directo, que es lo que entrega Windows.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use gpui::RenderImage;

/// Una vez por archivo o app: hay pocos distintos y se piden seguido.
fn cache() -> &'static Mutex<HashMap<String, Option<Arc<RenderImage>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<RenderImage>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(key: String, extract: impl FnOnce() -> Option<RenderImage>) -> Option<Arc<RenderImage>> {
    if let Some(hit) = cache().lock().ok()?.get(&key) {
        return hit.clone();
    }
    let icon = with_com(extract).map(Arc::new);
    cache().lock().ok()?.insert(key, icon.clone());
    icon
}

/// Ícono de un `.exe` o un acceso directo. Del `.lnk` se usa el ícono de lo
/// que apunta: el del acceso directo trae la flechita encima.
pub fn icon_for(path: &Path) -> Option<Arc<RenderImage>> {
    cached(path.to_string_lossy().to_lowercase(), || unsafe {
        let is_link = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
        is_link
            .then(|| link_target(path))
            .flatten()
            .and_then(|target| file_icon(&target))
            .or_else(|| file_icon(path))
    })
}

/// El ejecutable al que apunta un acceso directo (para saber si está abierto).
pub fn resolve_link(path: &Path) -> Option<std::path::PathBuf> {
    with_com(|| unsafe { link_target(path) })
}

/// Lo que apunta un acceso directo, si existe.
#[cfg(windows)]
unsafe fn link_target(path: &Path) -> Option<std::path::PathBuf> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    link.cast::<IPersistFile>()
        .ok()?
        .Load(&HSTRING::from(path.as_os_str()), STGM_READ)
        .ok()?;
    let mut buffer = [0u16; 1024];
    link.GetPath(&mut buffer, std::ptr::null_mut(), 0).ok()?;
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let target = std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..len]));
    target.exists().then_some(target)
}

#[cfg(not(windows))]
unsafe fn link_target(_: &Path) -> Option<std::path::PathBuf> {
    None
}

/// Ícono de una app del AppsFolder (Store): no hay archivo, lo da el shell.
pub fn icon_for_aumid(aumid: &str) -> Option<Arc<RenderImage>> {
    cached(format!("uwp:{}", aumid.to_lowercase()), || unsafe { aumid_icon(aumid) })
}

/// COM en el hilo que pide el ícono; si ya estaba iniciado, da igual.
#[cfg(windows)]
pub fn with_com<T>(f: impl FnOnce() -> T) -> T {
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    let initialized =
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;
    let out = f();
    if initialized {
        unsafe { CoUninitialize() };
    }
    out
}

#[cfg(not(windows))]
pub fn with_com<T>(f: impl FnOnce() -> T) -> T {
    f()
}

#[cfg(not(windows))]
unsafe fn file_icon(_: &Path) -> Option<RenderImage> {
    None
}

#[cfg(not(windows))]
unsafe fn aumid_icon(_: &str) -> Option<RenderImage> {
    None
}

#[cfg(windows)]
unsafe fn file_icon(path: &Path) -> Option<RenderImage> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
    use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
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
    // Monocromo: la máscara es de doble alto; no vale la pena.
    let image = if icon_info.hbmColor.is_null() {
        None
    } else {
        bitmap_image(icon_info.hbmColor as isize, false)
    };
    if !icon_info.hbmMask.is_null() {
        DeleteObject(icon_info.hbmMask as HGDIOBJ);
    }
    if !icon_info.hbmColor.is_null() {
        DeleteObject(icon_info.hbmColor as HGDIOBJ);
    }
    DestroyIcon(hicon);
    image
}

#[cfg(windows)]
unsafe fn aumid_icon(aumid: &str) -> Option<RenderImage> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ};
    use windows::Win32::System::Com::IBindCtx;
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT,
    };

    if aumid.is_empty() || aumid.starts_with('\\') || aumid.chars().any(char::is_control) {
        return None;
    }
    let parse = HSTRING::from(format!("shell:AppsFolder\\{aumid}"));
    let factory: IShellItemImageFactory =
        SHCreateItemFromParsingName(&parse, None::<&IBindCtx>).ok()?;
    let hbm = factory
        .GetImage(SIZE { cx: 48, cy: 48 }, SIIGBF_RESIZETOFIT)
        .ok()?;
    // El shell lo entrega premultiplicado; GPUI lo quiere directo.
    let image = bitmap_image(hbm.0 as isize, true);
    let _ = DeleteObject(HGDIOBJ(hbm.0));
    image
}

/// Un HBITMAP de 32 bpp a imagen de GPUI (BGRA, alfa directo). No se queda
/// con el handle: lo libera quien lo pasó.
#[cfg(windows)]
unsafe fn bitmap_image(hbm: isize, premultiplied: bool) -> Option<RenderImage> {
    use std::mem::{size_of, zeroed};

    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
    };

    let hbm = hbm as HBITMAP;
    let mut bmp: BITMAP = zeroed();
    if GetObjectW(hbm as HGDIOBJ, size_of::<BITMAP>() as i32, &mut bmp as *mut _ as *mut _) == 0 {
        return None;
    }
    let width = bmp.bmWidth.max(1) as u32;
    let height = bmp.bmHeight.abs().max(1) as u32;
    let screen = GetDC(std::ptr::null_mut());
    let dc = CreateCompatibleDC(screen);
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..zeroed()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let lines = GetDIBits(
        dc,
        hbm,
        0,
        height,
        pixels.as_mut_ptr() as *mut _,
        &mut bmi,
        DIB_RGB_COLORS,
    );
    DeleteDC(dc);
    ReleaseDC(std::ptr::null_mut(), screen);
    if lines == 0 {
        return None;
    }
    if pixels.chunks_exact(4).all(|px| px[3] == 0) {
        // 32 bpp sin alfa real: todo en 0 sería invisible.
        for px in pixels.chunks_exact_mut(4) {
            px[3] = 255;
        }
    } else if premultiplied {
        for px in pixels.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
    }
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(RenderImage::new([image::Frame::new(buffer)]))
}
