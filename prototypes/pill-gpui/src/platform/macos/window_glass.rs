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
//!
//! Un vidrio del tamaño de la ventana no se ve como Liquid Glass (la lente y el
//! brillo están en los bordes de las piezas que flotan): Atic Code pone en su
//! lugar piezas sueltas (`set_pieces`), un vidrio por panel flotante sobre la
//! base de la ventana.

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

/// Quita el vidrio, si estaba (también el de piezas).
pub fn clear(view: &AnyObject) {
    if objc2::MainThreadMarker::new().is_none() {
        return;
    }
    // SAFETY: AppKit en el hilo principal.
    unsafe {
        let parent: Option<Retained<AnyObject>> = msg_send![view, superview];
        let Some(parent) = parent else {
            return;
        };
        for id in [ID, PIECES_ID] {
            if let Some(glass) = find_id(&parent, id) {
                let _: () = msg_send![&glass, removeFromSuperview];
            }
        }
    }
}

/// SAFETY: en el hilo principal.
unsafe fn find(parent: &AnyObject) -> Option<Retained<AnyObject>> {
    find_id(parent, ID)
}

/// SAFETY: en el hilo principal.
unsafe fn find_id(parent: &AnyObject, wanted: &str) -> Option<Retained<AnyObject>> {
    let views: Retained<NSArray<AnyObject>> = msg_send![parent, subviews];
    views.iter().find(|view| {
        let id: Option<Retained<NSString>> = msg_send![&**view, identifier];
        id.is_some_and(|id| id.to_string() == wanted)
    })
}

/// El contenedor de las piezas (`set_pieces`).
const PIECES_ID: &str = "atic-window-pieces";

/// Una pieza bajo la vista de GPUI: un vidrio o un relleno de color, en puntos
/// de la ventana con el origen arriba a la izquierda, como en GPUI.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Piece {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    pub kind: PieceKind,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PieceKind {
    /// `NSGlassEffectView`, el Liquid Glass del sistema, con su estilo
    /// (`NSGlassEffectViewStyle`: 0 regular, 1 claro).
    Glass(isize),
    /// Un color plano en sRGB (`[r, g, b, a]`, de 0 a 1).
    Fill([f64; 4]),
}

/// Pone bajo la vista de GPUI `view` la base de la ventana (el vidrio de
/// ventana completa, si `base`) y encima, en orden, `pieces`: lo que va después
/// queda arriba. Reemplaza el vidrio de ventana completa de `apply`. Sin base,
/// lo que no es pieza queda transparente (un menú en su propia ventana). Sin
/// piezas, quita el contenedor. `false` si no se pudo.
pub fn set_pieces(view: &AnyObject, pieces: &[Piece], light: bool, base: bool) -> bool {
    if objc2::MainThreadMarker::new().is_none() {
        return false;
    }
    // SAFETY: AppKit en el hilo principal; `view` es la vista viva de GPUI.
    unsafe {
        let parent: Option<Retained<AnyObject>> = msg_send![view, superview];
        let Some(parent) = parent else {
            return false;
        };
        let container = find_id(&parent, PIECES_ID);
        if pieces.is_empty() {
            if let Some(container) = container {
                let _: () = msg_send![&container, removeFromSuperview];
            }
            return true;
        }
        // El vidrio de ventana completa queda tapado: fuera.
        if let Some(glass) = find(&parent) {
            let _: () = msg_send![&glass, removeFromSuperview];
        }
        let container = match container {
            Some(container) => container,
            None => {
                let Some(container) = build_container(&parent, base) else {
                    return false;
                };
                let _: () = msg_send![&parent, addSubview: &*container, positioned: BELOW, relativeTo: view];
                container
            }
        };
        // Las esquinas de la ventana: transparente, AppKit no recorta lo de adentro.
        let layer: Option<Retained<AnyObject>> = msg_send![&container, layer];
        if let Some(layer) = layer {
            let _: () = msg_send![&layer, setCornerRadius: corner_radius(&parent)];
        }
        let name = NSString::from_str(if light { "NSAppearanceNameAqua" } else { "NSAppearanceNameDarkAqua" });
        let appearance: Option<Retained<AnyObject>> = msg_send![class!(NSAppearance), appearanceNamed: &*name];
        let _: () = msg_send![&container, setAppearance: appearance.as_deref()];

        let bounds: NSRect = msg_send![&container, bounds];
        let height = bounds.size.height;
        let frame_of = |piece: &Piece| {
            NSRect::new(
                objc2_foundation::NSPoint::new(piece.x, height - piece.y - piece.h),
                objc2_foundation::NSSize::new(piece.w.max(0.), piece.h.max(0.)),
            )
        };
        // Sin base (un menú en su ventana), las piezas van sueltas: el contenedor de
        // fusión, del tamaño de la ventana, se vería él mismo como un vidrio.
        if !base {
            let views: Retained<NSArray<AnyObject>> = msg_send![&container, subviews];
            let all: Vec<&Piece> = pieces.iter().collect();
            sync_views(&container, None, views.iter().collect(), &all, &frame_of);
            return true;
        }
        // Adentro: la base, los rellenos en orden y, encima de todo, el
        // contenedor de vidrios (donde los vidrios cercanos se funden).
        let views: Retained<NSArray<AnyObject>> = msg_send![&container, subviews];
        let views: Vec<Retained<AnyObject>> = views.iter().collect();
        let glasses = match views.last().filter(|view| is_glass_container(view)) {
            Some(glasses) => glasses.clone(),
            None => {
                let Some(glasses) = build_glass_container(bounds) else {
                    return false;
                };
                let _: () = msg_send![&container, addSubview: &*glasses];
                glasses
            }
        };
        let fills: Vec<&Piece> = pieces.iter().filter(|piece| matches!(piece.kind, PieceKind::Fill(_))).collect();
        let current: Vec<Retained<AnyObject>> = views.iter().filter(|view| !is_glass_container(view) && !is_base(view)).cloned().collect();
        sync_views(&container, Some(&glasses), current, &fills, &frame_of);

        let content: Retained<AnyObject> = msg_send![&glasses, contentView];
        let glass: Vec<&Piece> = pieces.iter().filter(|piece| matches!(piece.kind, PieceKind::Glass(_))).collect();
        let views: Retained<NSArray<AnyObject>> = msg_send![&content, subviews];
        sync_views(&content, None, views.iter().collect(), &glass, &frame_of);
    }
    true
}

/// Deja en `parent` una vista por pieza, en orden, reusando `current` (las que
/// hay) y poniendo las nuevas bajo `below` si se da.
///
/// SAFETY: en el hilo principal.
unsafe fn sync_views(
    parent: &AnyObject,
    below: Option<&AnyObject>,
    mut current: Vec<Retained<AnyObject>>,
    pieces: &[&Piece],
    frame_of: &dyn Fn(&Piece) -> NSRect,
) {
    for (i, piece) in pieces.iter().enumerate() {
        let frame = frame_of(piece);
        let reuse = current.get(i).filter(|view| is_kind(view, piece.kind)).cloned();
        let view = match reuse {
            Some(view) => view,
            None => {
                let Some(new) = build_piece(piece.kind, frame) else {
                    return;
                };
                match current.get(i) {
                    Some(old) => {
                        let _: () = msg_send![parent, replaceSubview: &**old, with: &*new];
                        current[i] = new.clone();
                    }
                    None => {
                        match below {
                            Some(below) => {
                                let _: () = msg_send![parent, addSubview: &*new, positioned: BELOW, relativeTo: below];
                            }
                            None => {
                                let _: () = msg_send![parent, addSubview: &*new];
                            }
                        }
                        current.push(new.clone());
                    }
                }
                new
            }
        };
        let _: () = msg_send![&view, setFrame: frame];
        let _: () = msg_send![&view, setCornerRadius: piece.radius];
        match piece.kind {
            PieceKind::Glass(style) => {
                let _: () = msg_send![&view, setStyle: style];
            }
            PieceKind::Fill([r, g, b, a]) => {
                let color: Retained<AnyObject> = msg_send![class!(NSColor), colorWithSRGBRed: r, green: g, blue: b, alpha: a];
                let _: () = msg_send![&view, setFillColor: &*color];
            }
        }
    }
    for extra in current.iter().skip(pieces.len()) {
        let _: () = msg_send![&**extra, removeFromSuperview];
    }
}

/// La base del contenedor de piezas.
const BASE_ID: &str = "atic-window-pieces-base";

/// SAFETY: en el hilo principal.
unsafe fn is_base(view: &AnyObject) -> bool {
    let id: Option<Retained<NSString>> = msg_send![view, identifier];
    id.is_some_and(|id| id.to_string() == BASE_ID)
}

/// Los vidrios a menos de esto se funden: más que el espacio entre las píldoras
/// de una fila (8) las uniría en una sola barra; se funden al acercarse.
const MERGE_SPACING: f64 = 6.0;

/// SAFETY: en el hilo principal.
unsafe fn is_glass_container(view: &AnyObject) -> bool {
    match objc2::runtime::AnyClass::get(c"NSGlassEffectContainerView") {
        Some(class) => {
            let is: Bool = msg_send![view, isKindOfClass: class];
            is.as_bool()
        }
        None => false,
    }
}

/// El `NSGlassEffectContainerView` de los vidrios, del tamaño del contenedor.
///
/// SAFETY: en el hilo principal.
unsafe fn build_glass_container(frame: NSRect) -> Option<Retained<AnyObject>> {
    let glasses: Allocated<AnyObject> = msg_send![class!(NSGlassEffectContainerView), alloc];
    let glasses: Option<Retained<AnyObject>> = msg_send![glasses, initWithFrame: frame];
    let glasses = glasses?;
    let _: () = msg_send![&glasses, setAutoresizingMask: FILL];
    let _: () = msg_send![&glasses, setSpacing: MERGE_SPACING];
    let content: Allocated<AnyObject> = msg_send![class!(NSView), alloc];
    let content: Option<Retained<AnyObject>> = msg_send![content, initWithFrame: frame];
    let content = content?;
    let _: () = msg_send![&content, setAutoresizingMask: FILL];
    let _: () = msg_send![&glasses, setContentView: &*content];
    Some(glasses)
}

/// El contenedor de las piezas, con la base de la ventana adentro.
///
/// SAFETY: en el hilo principal.
unsafe fn build_container(parent: &AnyObject, base: bool) -> Option<Retained<AnyObject>> {
    let frame: NSRect = msg_send![parent, bounds];
    let container: Allocated<AnyObject> = msg_send![class!(NSView), alloc];
    let container: Option<Retained<AnyObject>> = msg_send![container, initWithFrame: frame];
    let container = container?;
    let _: () = msg_send![&container, setWantsLayer: Bool::YES];
    let layer: Option<Retained<AnyObject>> = msg_send![&container, layer];
    if let Some(layer) = layer {
        let _: () = msg_send![&layer, setMasksToBounds: Bool::YES];
    }
    let _: () = msg_send![&container, setAutoresizingMask: FILL];
    let _: () = msg_send![&container, setIdentifier: &*NSString::from_str(PIECES_ID)];
    // La base: el mismo vidrio de ventana completa de `apply`, que deja ver el
    // escritorio (el material de ventana de AppKit, en oscuro, queda casi opaco).
    if base {
        let base = build(parent)?;
        let _: () = msg_send![&base, setIdentifier: &*NSString::from_str(BASE_ID)];
        let _: () = msg_send![&container, addSubview: &*base];
    }
    Some(container)
}

/// SAFETY: en el hilo principal.
unsafe fn is_kind(view: &AnyObject, kind: PieceKind) -> bool {
    let class = match kind {
        PieceKind::Glass(_) => class!(NSGlassEffectView),
        PieceKind::Fill(_) => class!(NSBox),
    };
    let is: Bool = msg_send![view, isKindOfClass: class];
    is.as_bool()
}

/// SAFETY: en el hilo principal.
unsafe fn build_piece(kind: PieceKind, frame: NSRect) -> Option<Retained<AnyObject>> {
    match kind {
        PieceKind::Glass(style) => {
            let view: Allocated<AnyObject> = msg_send![class!(NSGlassEffectView), alloc];
            let view: Option<Retained<AnyObject>> = msg_send![view, initWithFrame: frame];
            let view = view?;
            let _: () = msg_send![&view, setStyle: style];
            Some(view)
        }
        PieceKind::Fill(_) => {
            // `NSBox` a medida: un color con esquinas redondeadas sin tocar `CALayer`.
            let view: Allocated<AnyObject> = msg_send![class!(NSBox), alloc];
            let view: Option<Retained<AnyObject>> = msg_send![view, initWithFrame: frame];
            let view = view?;
            // `NSBoxCustom` y `NSNoTitle`.
            let _: () = msg_send![&view, setBoxType: 4usize];
            let _: () = msg_send![&view, setTitlePosition: 0usize];
            let _: () = msg_send![&view, setBorderWidth: 0.0f64];
            let _: () = msg_send![&view, setContentViewMargins: objc2_foundation::NSSize::new(0., 0.)];
            Some(view)
        }
    }
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
