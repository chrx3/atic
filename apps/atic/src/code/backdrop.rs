//! El material del sistema detrás de Liquid Glass. En Windows 11, Acrylic, o
//! Mica con `CODE_GLASS=mica`, pintado por DWM debajo de la ventana; en macOS,
//! el Liquid Glass de verdad (`platform/macos/window_glass.rs`). El desenfoque de
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
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_STYLE, WS_SYSMENU};

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
        // Con el marco extendido, DWM pinta los botones nativos de la caption y al
        // maximizar asoman detrás de los nuestros. Sin menú del sistema no los pinta;
        // gpui maneja minimizar, maximizar y cerrar por su cuenta, y Alt+F4 sigue.
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let sysmenu = WS_SYSMENU.0 as isize;
        let style = if margin != 0 { style & !sysmenu } else { style | sysmenu };
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
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

/// El fondo de GPUI que pide el material: Windows lo pinta debajo de una
/// ventana opaca; en macOS va en una vista bajo la de GPUI, transparente.
pub const UNDER: gpui::WindowBackgroundAppearance =
    if cfg!(target_os = "macos") { gpui::WindowBackgroundAppearance::Transparent } else { gpui::WindowBackgroundAppearance::Opaque };

/// En macOS, Liquid Glass del sistema (`window_glass.rs`).
#[cfg(target_os = "macos")]
pub fn apply(window: &Window, light: bool) -> bool {
    crate::platform::win::glass_view(window).is_some_and(|view| crate::platform::window_glass::apply(&view, light))
}

#[cfg(target_os = "macos")]
pub fn clear(window: &Window) {
    if let Some(view) = crate::platform::win::glass_view(window) {
        crate::platform::window_glass::clear(&view);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn apply(_: &Window, _: bool) -> bool {
    false
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn clear(_: &Window) {}

// --- Liquid Glass por piezas (macOS 26): ver `crate::glass_pieces` --------------------

pub use crate::glass_pieces::{commit, forget, slot, Piece};

/// Si Atic Code arma Liquid Glass por piezas: macOS 26 en adelante y el estilo
/// Liquid Glass.
pub fn pieces() -> bool {
    crate::glass_pieces::active(super::style::t().style)
}
