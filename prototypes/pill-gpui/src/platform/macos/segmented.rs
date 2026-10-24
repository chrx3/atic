//! Controles segmentados de AppKit (`NSSegmentedControl`) encima de la vista
//! de GPUI: los de verdad, con su Liquid Glass, su brillo al pasar y su
//! selección. GPUI les reserva el lugar y dice dónde van
//! (`crate::native_controls`); el clic vuelve por un canal.
//!
//! Cada control tiene una etiqueta (`tag`) que es su llave aquí. Todo corre en
//! el hilo principal.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use futures::channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{class, define_class, msg_send, sel, AllocAnyThread};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// Un segmento: su texto, su símbolo SF, la ayuda al pasar y si está marcado.
#[derive(Clone, PartialEq)]
pub struct Segment {
    pub label: String,
    pub symbol: &'static str,
    pub tip: String,
    pub selected: bool,
}

struct Control {
    view: Retained<AnyObject>,
    /// Lo último que se le puso, para no tocar AppKit si no cambió.
    shown: Option<(Vec<Segment>, bool, bool)>,
    /// Lo marcado según AppKit: el clic lo cambia antes de que GPUI responda.
    selected: Vec<bool>,
    tx: UnboundedSender<usize>,
}

thread_local! {
    static CONTROLS: RefCell<HashMap<isize, Control>> = RefCell::new(HashMap::new());
    static NEXT: Cell<isize> = const { Cell::new(0x4174_6963) };
    static TARGET: Retained<Target> = Target::new();
}

define_class!(
    /// El destino de los clics: dice qué segmento cambió.
    #[unsafe(super(NSObject))]
    #[name = "AticSegmentedTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &AnyObject) {
            // SAFETY: `sender` es uno de nuestros `NSSegmentedControl`.
            let tag: isize = unsafe { msg_send![sender, tag] };
            CONTROLS.with(|controls| {
                let mut controls = controls.borrow_mut();
                let Some(control) = controls.get_mut(&tag) else {
                    return;
                };
                for (index, before) in control.selected.iter_mut().enumerate() {
                    // SAFETY: el índice está dentro de los segmentos del control.
                    let now: Bool = unsafe { msg_send![sender, isSelectedForSegment: index as isize] };
                    if now.as_bool() != *before {
                        *before = now.as_bool();
                        let _ = control.tx.unbounded_send(index);
                        return;
                    }
                }
            });
        }
    }
);

impl Target {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: `init` de `NSObject`.
        unsafe { msg_send![super(this), init] }
    }
}

/// Un control nuevo, todavía sin lugar: sus clics van a `tx` con el índice
/// del segmento.
pub fn create(tx: UnboundedSender<usize>) -> isize {
    let tag = NEXT.with(|next| {
        let tag = next.get();
        next.set(tag + 1);
        tag
    });
    // SAFETY: AppKit en el hilo principal (GPUI pinta ahí).
    let view: Retained<AnyObject> = unsafe {
        let view: Retained<AnyObject> = msg_send![class!(NSSegmentedControl), new];
        // `NSSegmentSwitchTrackingSelectAny`: cada uno se marca y desmarca solo;
        // la regla (uno u otro) la pone quien responde al clic.
        let _: () = msg_send![&view, setTrackingMode: 1usize];
        // `NSControlSizeLarge`, la altura de los botones de la barra.
        let _: () = msg_send![&view, setControlSize: 3usize];
        let _: () = msg_send![&view, setTag: tag];
        TARGET.with(|target| {
            let _: () = msg_send![&view, setTarget: &**target];
        });
        let _: () = msg_send![&view, setAction: sel!(pick:)];
        let _: () = msg_send![&view, setHidden: Bool::YES];
        view
    };
    CONTROLS.with(|controls| {
        controls.borrow_mut().insert(tag, Control { view, shown: None, selected: Vec::new(), tx });
    });
    tag
}

/// Pone los segmentos (si cambiaron) y devuelve el tamaño que pide el control.
/// `icon_only`: solo los símbolos, el texto queda en la ayuda.
pub fn update(tag: isize, segments: &[Segment], icon_only: bool, light: bool) -> (f32, f32) {
    CONTROLS.with(|controls| {
        let mut controls = controls.borrow_mut();
        let Some(control) = controls.get_mut(&tag) else {
            return (0., 0.);
        };
        let key = (segments.to_vec(), icon_only, light);
        if control.shown.as_ref() != Some(&key) {
            // SAFETY: AppKit en el hilo principal.
            unsafe { fill(&control.view, segments, icon_only, light) };
            control.selected = segments.iter().map(|s| s.selected).collect();
            control.shown = Some(key);
        }
        // SAFETY: lo mismo.
        let frame: NSRect = unsafe { msg_send![&control.view, frame] };
        (frame.size.width as f32, frame.size.height as f32)
    })
}

/// SAFETY: en el hilo principal.
unsafe fn fill(view: &AnyObject, segments: &[Segment], icon_only: bool, light: bool) {
    let name = NSString::from_str(if light { "NSAppearanceNameAqua" } else { "NSAppearanceNameDarkAqua" });
    let appearance: Option<Retained<AnyObject>> = msg_send![class!(NSAppearance), appearanceNamed: &*name];
    let _: () = msg_send![view, setAppearance: appearance.as_deref()];
    let _: () = msg_send![view, setSegmentCount: segments.len() as isize];
    for (index, segment) in segments.iter().enumerate() {
        let index = index as isize;
        let label = NSString::from_str(if icon_only { "" } else { &segment.label });
        let _: () = msg_send![view, setLabel: &*label, forSegment: index];
        let symbol = NSString::from_str(segment.symbol);
        let tip = NSString::from_str(&segment.tip);
        let image: Option<Retained<AnyObject>> =
            msg_send![class!(NSImage), imageWithSystemSymbolName: &*symbol, accessibilityDescription: &*tip];
        let _: () = msg_send![view, setImage: image.as_deref(), forSegment: index];
        let _: () = msg_send![view, setToolTip: &*tip, forSegment: index];
        // Que cada uno mida lo que pide su contenido.
        let _: () = msg_send![view, setWidth: 0.0f64, forSegment: index];
        let _: () = msg_send![view, setSelected: Bool::new(segment.selected), forSegment: index];
    }
    let _: () = msg_send![view, sizeToFit];
}

/// Lo pone en `rect` (coordenadas de la vista de GPUI, arriba a la izquierda) o
/// lo esconde. Va encima de la vista de GPUI, como su hermano.
pub fn place(tag: isize, gpui_view: &AnyObject, rect: Option<(f32, f32, f32, f32)>) {
    CONTROLS.with(|controls| {
        let controls = controls.borrow();
        let Some(control) = controls.get(&tag) else {
            return;
        };
        let view = &control.view;
        // SAFETY: AppKit en el hilo principal.
        unsafe {
            let Some((x, y, w, h)) = rect else {
                let _: () = msg_send![view, setHidden: Bool::YES];
                return;
            };
            let Some(parent): Option<Retained<AnyObject>> = msg_send![gpui_view, superview] else {
                return;
            };
            let current: Option<Retained<AnyObject>> = msg_send![view, superview];
            if current.as_deref().map(|p| p as *const AnyObject) != Some(&*parent as *const AnyObject) {
                // `NSWindowAbove`.
                let _: () = msg_send![&parent, addSubview: &**view, positioned: 1isize, relativeTo: gpui_view];
            }
            // GPUI no voltea su vista: el origen de AppKit está abajo.
            let bounds: NSRect = msg_send![gpui_view, bounds];
            let local = NSRect::new(
                NSPoint::new(x as f64, bounds.size.height - (y + h) as f64),
                NSSize::new(w as f64, h as f64),
            );
            let frame: NSRect = msg_send![gpui_view, convertRect: local, toView: &*parent];
            let _: () = msg_send![view, setFrame: frame];
            let _: () = msg_send![view, setHidden: Bool::NO];
        }
    });
}
