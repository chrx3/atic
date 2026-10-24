//! Menús emergentes nativos (`NSMenu`): los del sistema, con su Liquid Glass,
//! su animación y su teclado, en vez de dibujarlos con GPUI.
//!
//! `popUpMenuPositioningItem:atLocation:inView:` sigue al menú en su propio
//! bucle hasta que se cierra: va fuera de cualquier préstamo de GPUI
//! (`later`). La entrada elegida vuelve por un `oneshot`.

use std::sync::atomic::{AtomicIsize, Ordering};

use futures::channel::oneshot;
use gpui::{Pixels, Point, Window};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{class, define_class, msg_send, sel, AllocAnyThread};
use objc2_foundation::{NSPoint, NSRect, NSString};

/// Lo que va en el menú, en orden.
pub enum Item {
    /// Un título de sección (macOS 14 en adelante).
    Header(String),
    Separator,
    Entry(Entry),
}

/// Una entrada que se puede elegir.
#[derive(Default)]
pub struct Entry {
    pub title: String,
    /// La línea de ayuda bajo el título (macOS 14.4 en adelante).
    pub subtitle: Option<String>,
    /// Con la marca: la opción elegida ahora.
    pub checked: bool,
    /// Un símbolo SF a la izquierda (`folder`, `bubble.left`…).
    pub symbol: Option<&'static str>,
}

impl Entry {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), ..Default::default() }
    }
}

/// La entrada elegida en el menú abierto (`-1`: ninguna).
static CHOSEN: AtomicIsize = AtomicIsize::new(-1);

define_class!(
    /// El destino de las entradas: guarda cuál se eligió.
    #[unsafe(super(NSObject))]
    #[name = "AticPopupMenuTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &AnyObject) {
            // SAFETY: `sender` es el `NSMenuItem` elegido.
            let tag: isize = unsafe { msg_send![sender, tag] };
            CHOSEN.store(tag, Ordering::Relaxed);
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

/// Abre el menú en `at` (coordenadas de `window`). Con una entrada marcada, esa
/// queda bajo el cursor, como los menús emergentes de macOS; si no, el menú
/// cuelga de `at` hacia abajo. Devuelve el índice en `items` de la elegida, o
/// `None` si se cerró sin elegir.
pub fn pop_up(window: &Window, at: Point<Pixels>, items: Vec<Item>, light: bool) -> oneshot::Receiver<Option<usize>> {
    let (tx, rx) = oneshot::channel();
    let Some(view) = super::win::glass_view(window) else {
        let _ = tx.send(None);
        return rx;
    };
    super::win::later(move || {
        // SAFETY: AppKit en el hilo principal, fuera de GPUI; `view` vive en el cierre.
        let chosen = unsafe { show(&view, at, &items, light) };
        let _ = tx.send(chosen);
    });
    rx
}

/// SAFETY: en el hilo principal.
unsafe fn show(view: &AnyObject, at: Point<Pixels>, items: &[Item], light: bool) -> Option<usize> {
    let target = Target::new();
    let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
    let _: () = msg_send![&menu, setAutoenablesItems: Bool::NO];
    // El claro u oscuro de Atic, no el de macOS (como el vidrio de las ventanas).
    let name = NSString::from_str(if light { "NSAppearanceNameAqua" } else { "NSAppearanceNameDarkAqua" });
    let appearance: Option<Retained<AnyObject>> = msg_send![class!(NSAppearance), appearanceNamed: &*name];
    let _: () = msg_send![&menu, setAppearance: appearance.as_deref()];

    let mut marked = None;
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::Header(heading) => {
                let title = NSString::from_str(heading);
                let responds: Bool = msg_send![class!(NSMenuItem), respondsToSelector: sel!(sectionHeaderWithTitle:)];
                if responds.as_bool() {
                    let header: Retained<AnyObject> = msg_send![class!(NSMenuItem), sectionHeaderWithTitle: &*title];
                    let _: () = msg_send![&menu, addItem: &*header];
                }
            }
            Item::Separator => {
                let separator: Retained<AnyObject> = msg_send![class!(NSMenuItem), separatorItem];
                let _: () = msg_send![&menu, addItem: &*separator];
            }
            Item::Entry(entry) => {
                let item = entry_item(entry, index, &target);
                if entry.checked {
                    marked = Some(item.clone());
                }
                let _: () = msg_send![&menu, addItem: &*item];
            }
        }
    }

    // GPUI no voltea su vista: el origen de AppKit está abajo.
    let bounds: NSRect = msg_send![view, bounds];
    let location = NSPoint::new(f32::from(at.x) as f64, bounds.size.height - f32::from(at.y) as f64);
    CHOSEN.store(-1, Ordering::Relaxed);
    let _: Bool = msg_send![&menu, popUpMenuPositioningItem: marked.as_deref(), atLocation: location, inView: view];
    let chosen = CHOSEN.swap(-1, Ordering::Relaxed);
    usize::try_from(chosen).ok().filter(|index| matches!(items.get(*index), Some(Item::Entry(_))))
}

/// SAFETY: en el hilo principal.
unsafe fn entry_item(entry: &Entry, tag: usize, target: &Target) -> Retained<AnyObject> {
    let title = NSString::from_str(&entry.title);
    let empty = NSString::from_str("");
    let item: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
    let item: Retained<AnyObject> = msg_send![item, initWithTitle: &*title, action: sel!(pick:), keyEquivalent: &*empty];
    let _: () = msg_send![&item, setTarget: target];
    let _: () = msg_send![&item, setTag: tag as isize];
    if let Some(subtitle) = &entry.subtitle {
        let responds: Bool = msg_send![&item, respondsToSelector: sel!(setSubtitle:)];
        if responds.as_bool() {
            let _: () = msg_send![&item, setSubtitle: &*NSString::from_str(subtitle)];
        }
    }
    if let Some(symbol) = entry.symbol {
        let name = NSString::from_str(symbol);
        let image: Option<Retained<AnyObject>> =
            msg_send![class!(NSImage), imageWithSystemSymbolName: &*name, accessibilityDescription: Option::<&NSString>::None];
        if let Some(image) = image {
            let _: () = msg_send![&item, setImage: &*image];
        }
    }
    if entry.checked {
        // `NSControlStateValueOn`.
        let _: () = msg_send![&item, setState: 1isize];
    }
    item
}

/// Pide confirmar algo que no se deshace (`NSAlert`): `button` hace la acción y
/// va marcado como destructivo; «Cancelar» la deja. `true` si se confirmó.
pub fn confirm(message: &str, info: &str, button: &str, light: bool) -> oneshot::Receiver<bool> {
    let (tx, rx) = oneshot::channel();
    let (message, info, button) = (message.to_string(), info.to_string(), button.to_string());
    super::win::later(move || {
        // SAFETY: AppKit en el hilo principal, fuera de GPUI.
        let confirmed = unsafe {
            let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
            // `NSAlertStyleWarning`.
            let _: () = msg_send![&alert, setAlertStyle: 0usize];
            let _: () = msg_send![&alert, setMessageText: &*NSString::from_str(&message)];
            let _: () = msg_send![&alert, setInformativeText: &*NSString::from_str(&info)];
            let name = NSString::from_str(if light { "NSAppearanceNameAqua" } else { "NSAppearanceNameDarkAqua" });
            let appearance: Option<Retained<AnyObject>> = msg_send![class!(NSAppearance), appearanceNamed: &*name];
            let _: () = msg_send![&alert, setAppearance: appearance.as_deref()];
            let action: Retained<AnyObject> = msg_send![&alert, addButtonWithTitle: &*NSString::from_str(&button)];
            let responds: Bool = msg_send![&action, respondsToSelector: sel!(setHasDestructiveAction:)];
            if responds.as_bool() {
                let _: () = msg_send![&action, setHasDestructiveAction: Bool::YES];
            }
            let _: Retained<AnyObject> = msg_send![&alert, addButtonWithTitle: &*NSString::from_str(crate::i18n::t("pill.settings.cancel"))];
            // `NSAlertFirstButtonReturn`.
            let response: isize = msg_send![&alert, runModal];
            response == 1000
        };
        let _ = tx.send(confirmed);
    });
    rx
}
