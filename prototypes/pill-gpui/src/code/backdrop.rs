//! El material de Windows 11 detrás de Liquid Glass: Acrylic, o Mica con
//! `CODE_GLASS=mica`, pintado por DWM debajo de la ventana. El desenfoque de
//! GPUI (`WindowBackgroundAppearance::Blurred`) usa el acrylic viejo de
//! `SetWindowCompositionAttribute`, que va lento al arrastrar o redimensionar.

use gpui::Window;

/// Pone el material; `false` si el sistema no lo tiene y hay que quedarse con
/// el desenfoque de GPUI.
#[cfg(windows)]
pub fn apply(window: &Window, light: bool) -> bool {
    let applied = if std::env::var("CODE_GLASS").as_deref() == Ok("mica") {
        window_vibrancy::apply_mica(window, Some(!light))
    } else {
        window_vibrancy::apply_acrylic(window, None)
    };
    if applied.is_err() {
        return false;
    }
    // El material sigue el modo de la ventana, no el de Windows; y sin extender
    // el marco DWM solo lo pinta en el borde, no bajo el contenido.
    set_frame(window, -1, Some(!light));
    true
}

/// Quita el material (al salir de Liquid Glass).
#[cfg(windows)]
pub fn clear(window: &Window) {
    let _ = window_vibrancy::clear_mica(window);
    let _ = window_vibrancy::clear_acrylic(window);
    set_frame(window, 0, None);
}

#[cfg(windows)]
fn set_frame(window: &Window, margin: i32, dark: Option<bool>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::BOOL;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE,
    };
    use windows::Win32::UI::Controls::MARGINS;

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    let margins = MARGINS { cxLeftWidth: margin, cxRightWidth: margin, cyTopHeight: margin, cyBottomHeight: margin };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        if let Some(dark) = dark {
            let dark = BOOL::from(dark);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const BOOL as *const _,
                std::mem::size_of::<BOOL>() as u32,
            );
        }
    }
}

#[cfg(not(windows))]
pub fn apply(_: &Window, _: bool) -> bool {
    false
}

#[cfg(not(windows))]
pub fn clear(_: &Window) {}
