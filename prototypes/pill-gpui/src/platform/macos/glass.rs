//! El vidrio esmerilado detrás de la pill en macOS (`NSVisualEffectView`).
//! El Liquid Glass va en las ventanas normales (`window_glass.rs`), no aquí.
//!
//! Como en Windows, es una ventana nativa aparte, sin clics, justo debajo del
//! overlay, que sigue la forma de la pill. Adentro van dos
//! `NSVisualEffectView` que desenfocan lo que hay detrás de la ventana: la
//! primera es la pieza única y la segunda el bloque de la forma
//! en L. Cada una se recorta con el radio y las esquinas de su capa
//! (`cornerRadius` y `maskedCorners`), sin máscara de imagen.
//!
//! `PILL_GLASS=off` lo apaga; con «Reducir transparencia» en Accesibilidad
//! la pill usa su piel sin vidrio, como Windows sin transparencias.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::win::Handle;
use crate::geometry::Rect;

/// La forma del vidrio, en coordenadas lógicas del overlay.
#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    /// Esquinas en orden: arriba-izquierda, arriba-derecha, abajo-derecha,
    /// abajo-izquierda (como `liquid::rounded_rect_corners`).
    Rounded { rect: Rect, radii: [f32; 4] },
    /// Dos rectángulos redondeados que se tocan: la columna de la tira y el
    /// bloque que sale a su lado en un costado.
    Pair { a: (Rect, [f32; 4]), b: (Rect, [f32; 4]) },
    Circle { center: (f32, f32), r: f32 },
}

pub fn enabled() -> bool {
    std::env::var("PILL_GLASS").as_deref() != Ok("off")
}

/// Sin «Reducir transparencia». Se relee cada segundo: se puede cambiar con
/// la pill abierta.
pub fn backdrop_available() -> bool {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, bool)>> = std::sync::Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return true;
    };
    if let Some((at, on)) = *cache {
        if at.elapsed() < std::time::Duration::from_secs(1) {
            return on;
        }
    }
    // SAFETY: consulta de `NSWorkspace`, segura desde cualquier hilo.
    let reduce: Bool = unsafe {
        let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        msg_send![&workspace, accessibilityDisplayShouldReduceTransparency]
    };
    let on = !reduce.as_bool();
    *cache = Some((std::time::Instant::now(), on));
    on
}

/// `NSVisualEffectMaterialHUDWindow`: oscuro, como el tinte de la pill.
const MATERIAL_HUD: isize = 13;
/// `NSVisualEffectBlendingModeBehindWindow`: desenfoca lo de otras apps.
const BEHIND_WINDOW: isize = 0;
/// `NSVisualEffectStateActive`: la pill nunca es la app activa, pero el
/// vidrio debe verse igual.
const STATE_ACTIVE: isize = 1;
/// `NSWindowBelow`.
const BELOW: isize = -1;

pub struct Glass {
    window: Retained<AnyObject>,
    main: Retained<AnyObject>,
    second: Retained<AnyObject>,
    /// La forma y la esquina del overlay en pantalla: con otro monitor del
    /// mismo tamaño la forma es igual, pero el vidrio tiene que ir allá.
    placed: Option<(Shape, (i32, i32))>,
}

impl Glass {
    /// Solo en el hilo principal (la pill lo crea al armarse).
    pub fn new() -> Option<Self> {
        objc2::MainThreadMarker::new()?;
        // SAFETY: AppKit en el hilo principal; los objetos quedan retenidos
        // en `Glass` y se cierran en `Drop`.
        unsafe {
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
            let window: Allocated<AnyObject> = msg_send![class!(NSWindow), alloc];
            // Sin bordes (`NSWindowStyleMaskBorderless`), con búfer (`NSBackingStoreBuffered`).
            let window: Option<Retained<AnyObject>> =
                msg_send![window, initWithContentRect: frame, styleMask: 0usize, backing: 2usize, defer: Bool::NO];
            let window = window?;
            let clear: Retained<AnyObject> = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![&window, setReleasedWhenClosed: Bool::NO];
            let _: () = msg_send![&window, setOpaque: Bool::NO];
            let _: () = msg_send![&window, setBackgroundColor: &*clear];
            let _: () = msg_send![&window, setHasShadow: Bool::NO];
            let _: () = msg_send![&window, setIgnoresMouseEvents: Bool::YES];
            // En las capturas, como la pill (`NSWindowSharingReadOnly`).
            let _: () = msg_send![&window, setSharingType: 1usize];
            // Sin la animación de aparecer: sigue a la pill cuadro a cuadro.
            let _: () = msg_send![&window, setAnimationBehavior: 2isize];
            let root: Allocated<AnyObject> = msg_send![class!(NSView), alloc];
            let root: Retained<AnyObject> = msg_send![root, initWithFrame: frame];
            let _: () = msg_send![&root, setWantsLayer: Bool::YES];
            let main = effect_view(&root);
            let second = effect_view(&root);
            let _: () = msg_send![&window, setContentView: &*root];
            Some(Self { window, main, second, placed: None })
        }
    }

    pub fn hide(&mut self) {
        if self.placed.take().is_some() {
            let none: *const AnyObject = std::ptr::null();
            // SAFETY: la ventana es nuestra.
            unsafe {
                let _: () = msg_send![&self.window, orderOut: none];
            }
        }
    }

    /// Pone el vidrio con la forma `shape`, justo debajo de `above` (la
    /// ventana del overlay). En macOS todo va en puntos: el origen físico y
    /// la escala sobran.
    pub fn place(&mut self, shape: Shape, above: Handle, origin: (i32, i32), _scale: f32) {
        if self.placed == Some((shape, origin)) {
            return;
        }
        let moved = self.placed.is_some_and(|(_, at)| at != origin);
        let Some(above) = above.window() else {
            return;
        };
        let (a, b) = match shape {
            Shape::Rounded { rect, radii } => ((rect, radii), None),
            Shape::Pair { a, b } => (a, Some(b)),
            Shape::Circle { center, r } => ((Rect::centered(center, r * 2.0, r * 2.0), [r; 4]), None),
        };
        // La ventana cubre las dos piezas; cada una va relativa a ella.
        let (x0, y0) = (a.0.x.min(b.map_or(a.0.x, |b| b.0.x)), a.0.y.min(b.map_or(a.0.y, |b| b.0.y)));
        let (x1, y1) = (
            a.0.right().max(b.map_or(a.0.right(), |b| b.0.right())),
            a.0.bottom().max(b.map_or(a.0.bottom(), |b| b.0.bottom())),
        );
        let (w, h) = ((x1 - x0) as f64, (y1 - y0) as f64);
        if w < 1.0 || h < 1.0 {
            self.hide();
            return;
        }
        // SAFETY: AppKit en el hilo principal; `above` es la ventana viva del
        // overlay.
        unsafe {
            // El overlay en coordenadas de AppKit: origen abajo a la izquierda.
            let frame: NSRect = msg_send![&*above, frame];
            let content: NSRect = msg_send![&*above, contentRectForFrameRect: frame];
            let top = content.origin.y + content.size.height;
            let window_rect =
                NSRect::new(NSPoint::new(content.origin.x + x0 as f64, top - y0 as f64 - h), NSSize::new(w, h));

            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: Bool::YES];
            let _: () = msg_send![&self.window, setFrame: window_rect, display: Bool::NO];
            shape_piece(&self.main, a, (x0, y0), h);
            match b {
                Some(b) => {
                    shape_piece(&self.second, b, (x0, y0), h);
                    let _: () = msg_send![&self.second, setHidden: Bool::NO];
                }
                None => {
                    let _: () = msg_send![&self.second, setHidden: Bool::YES];
                }
            }
            let _: () = msg_send![class!(CATransaction), commit];

            if self.placed.is_none() || moved {
                // El mismo nivel y los mismos escritorios que el overlay.
                let level: isize = msg_send![&*above, level];
                let behavior: usize = msg_send![&*above, collectionBehavior];
                let _: () = msg_send![&self.window, setLevel: level];
                let _: () = msg_send![&self.window, setCollectionBehavior: behavior];
                let number: isize = msg_send![&*above, windowNumber];
                let _: () = msg_send![&self.window, orderWindow: BELOW, relativeTo: number];
            }
        }
        self.placed = Some((shape, origin));
    }
}

impl Drop for Glass {
    fn drop(&mut self) {
        // SAFETY: la ventana es nuestra y se cierra una vez.
        unsafe {
            let _: () = msg_send![&self.window, close];
        }
    }
}

/// Un `NSVisualEffectView` con capa, dentro de `parent`.
unsafe fn effect_view(parent: &AnyObject) -> Retained<AnyObject> {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
    let view: Allocated<AnyObject> = msg_send![class!(NSVisualEffectView), alloc];
    let view: Retained<AnyObject> = msg_send![view, initWithFrame: frame];
    let _: () = msg_send![&view, setMaterial: MATERIAL_HUD];
    let _: () = msg_send![&view, setBlendingMode: BEHIND_WINDOW];
    let _: () = msg_send![&view, setState: STATE_ACTIVE];
    let _: () = msg_send![&view, setWantsLayer: Bool::YES];
    let layer: Option<Retained<AnyObject>> = msg_send![&view, layer];
    if let Some(layer) = layer {
        let _: () = msg_send![&layer, setMasksToBounds: Bool::YES];
    }
    let _: () = msg_send![parent, addSubview: &*view];
    view
}

/// Coloca una pieza (en coordenadas del overlay, `y` hacia abajo) dentro de
/// la ventana del vidrio, que mide `window_h` de alto y empieza en `origin`.
unsafe fn shape_piece(view: &AnyObject, (rect, radii): (Rect, [f32; 4]), origin: (f32, f32), window_h: f64) {
    let x = (rect.x - origin.0) as f64;
    let y = window_h - (rect.y - origin.1) as f64 - rect.h as f64;
    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(rect.w as f64, rect.h as f64));
    let _: () = msg_send![view, setFrame: frame];
    // Un solo radio para las esquinas redondas; las vivas quedan fuera de
    // la máscara. En la capa de una vista sin voltear, `MinY` es abajo.
    const MIN_X_MIN_Y: usize = 1;
    const MAX_X_MIN_Y: usize = 2;
    const MIN_X_MAX_Y: usize = 4;
    const MAX_X_MAX_Y: usize = 8;
    let corners = [MIN_X_MAX_Y, MAX_X_MAX_Y, MAX_X_MIN_Y, MIN_X_MIN_Y];
    let radius = radii.iter().copied().fold(0.0f32, f32::max);
    let mask = radii
        .iter()
        .zip(corners)
        .filter(|(r, _)| **r > 0.5)
        .fold(0usize, |mask, (_, corner)| mask | corner);
    let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
    if let Some(layer) = layer {
        let _: () = msg_send![&layer, setCornerRadius: radius as f64];
        let _: () = msg_send![&layer, setMaskedCorners: mask];
    }
}
