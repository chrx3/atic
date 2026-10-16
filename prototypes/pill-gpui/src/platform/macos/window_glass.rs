//! El vidrio detrás de una ventana normal (Atic Code, Ajustes, Reuniones)
//! cuando el estilo es Liquid Glass: el `NSGlassEffectView` del sistema
//! (macOS 26 en adelante) o, antes, el material translúcido de las ventanas
//! (`NSVisualEffectView`). Va dentro de la vista de contenido, debajo de la
//! de GPUI, que queda transparente y pinta encima sus fondos con la opacidad
//! elegida.
//!
//! El desenfoque de GPUI (`WindowBackgroundAppearance::Blurred`) le quita al
//! `NSVisualEffectView` su tinte y sus reflejos: queda solo un poco de
//! transparencia.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSArray, NSRect, NSString};

/// El nombre que marca nuestra vista, para encontrarla y quitarla.
const ID: &str = "atic-window-glass";
/// `NSWindowBelow`.
const BELOW: isize = -1;
/// `NSViewWidthSizable | NSViewHeightSizable`.
const FILL: usize = 2 | 16;

/// Si el sistema tiene Liquid Glass (macOS 26 en adelante).
pub fn liquid_available() -> bool {
    objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some()
}

/// Pone el vidrio detrás de la vista de GPUI `view`; si ya está, solo le
/// cambia el claro u oscuro. `false` si no se pudo.
pub fn apply(view: &AnyObject, light: bool) -> bool {
    if objc2::MainThreadMarker::new().is_none() {
        return false;
    }
    // SAFETY: AppKit en el hilo principal; `view` es la vista viva de GPUI.
    unsafe {
        let parent: Option<Retained<AnyObject>> = msg_send![view, superview];
        let Some(parent) = parent else {
            return false;
        };
        let glass = match find(&parent) {
            Some(glass) => glass,
            None => {
                let Some(glass) = build(&parent) else {
                    return false;
                };
                let _: () = msg_send![&parent, addSubview: &*glass, positioned: BELOW, relativeTo: view];
                glass
            }
        };
        // El vidrio sigue el claro u oscuro de Atic, no el de macOS.
        let name = NSString::from_str(if light { "NSAppearanceNameAqua" } else { "NSAppearanceNameDarkAqua" });
        let appearance: Option<Retained<AnyObject>> = msg_send![class!(NSAppearance), appearanceNamed: &*name];
        let _: () = msg_send![&glass, setAppearance: appearance.as_deref()];
    }
    true
}

/// Quita el vidrio, si estaba.
pub fn clear(view: &AnyObject) {
    if objc2::MainThreadMarker::new().is_none() {
        return;
    }
    // SAFETY: AppKit en el hilo principal.
    unsafe {
        let parent: Option<Retained<AnyObject>> = msg_send![view, superview];
        if let Some(glass) = parent.as_deref().and_then(|parent| find(parent)) {
            let _: () = msg_send![&glass, removeFromSuperview];
        }
    }
}

/// SAFETY: en el hilo principal.
unsafe fn find(parent: &AnyObject) -> Option<Retained<AnyObject>> {
    let views: Retained<NSArray<AnyObject>> = msg_send![parent, subviews];
    views.iter().find(|view| {
        let id: Option<Retained<NSString>> = msg_send![&**view, identifier];
        id.is_some_and(|id| id.to_string() == ID)
    })
}

/// La vista del vidrio, del tamaño de `parent` y que crece con ella.
///
/// SAFETY: en el hilo principal.
unsafe fn build(parent: &AnyObject) -> Option<Retained<AnyObject>> {
    let frame: NSRect = msg_send![parent, bounds];
    let view: Option<Retained<AnyObject>> = if liquid_available() {
        let view: Allocated<AnyObject> = msg_send![class!(NSGlassEffectView), alloc];
        let view: Option<Retained<AnyObject>> = msg_send![view, initWithFrame: frame];
        if let Some(view) = &view {
            // `NSGlassEffectViewStyleRegular`: se lee mejor que el claro.
            let _: () = msg_send![&**view, setStyle: 0isize];
            let _: () = msg_send![&**view, setCornerRadius: corner_radius(parent)];
        }
        view
    } else {
        let view: Allocated<AnyObject> = msg_send![class!(NSVisualEffectView), alloc];
        let view: Option<Retained<AnyObject>> = msg_send![view, initWithFrame: frame];
        if let Some(view) = &view {
            // `NSVisualEffectMaterialUnderWindowBackground`, detrás de la
            // ventana, siempre activo (también con la ventana de atrás).
            let _: () = msg_send![&**view, setMaterial: 21isize];
            let _: () = msg_send![&**view, setBlendingMode: 0isize];
            let _: () = msg_send![&**view, setState: 1isize];
        }
        view
    };
    let view = view?;
    let _: () = msg_send![&view, setAutoresizingMask: FILL];
    let _: () = msg_send![&view, setIdentifier: &*NSString::from_str(ID)];
    Some(view)
}

/// El radio de las esquinas de la ventana, para que el borde del vidrio siga
/// el de la ventana. AppKit no lo publica: `_cornerRadius` si está.
///
/// SAFETY: en el hilo principal.
unsafe fn corner_radius(parent: &AnyObject) -> f64 {
    let window: Option<Retained<AnyObject>> = msg_send![parent, window];
    let selector: Sel = sel!(_cornerRadius);
    match window {
        Some(window) => {
            let responds: Bool = msg_send![&window, respondsToSelector: selector];
            if responds.as_bool() {
                msg_send![&window, _cornerRadius]
            } else {
                12.0
            }
        }
        None => 12.0,
    }
}
