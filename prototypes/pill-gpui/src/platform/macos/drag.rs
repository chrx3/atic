//! Arrastrar texto y archivos hacia otras apps en macOS: `NSDraggingSession`
//! desde la vista del overlay.
//!
//! En Windows `DoDragDrop` es modal y vuelve al soltar; aquí la sesión de
//! AppKit avisa al terminar, así que se espera atendiendo los eventos de la
//! app (lo mismo que hace el loop modal de Windows) hasta que llega el aviso.
//! Quien llama ya corre en una tarea aparte (ver `start_drag` en `main.rs`).

use std::cell::Cell;
use std::time::{Duration, Instant};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{class, define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

#[link(name = "UniformTypeIdentifiers", kind = "framework")]
extern "C" {}

/// Cómo terminó el arrastre (ver `win32/drag.rs`). AppKit dice la operación
/// que eligió el destino: `effect` es 0 si nadie lo aceptó.
pub struct DragOutcome {
    pub dropped: bool,
    pub effect: u32,
}

#[derive(Default)]
struct State {
    /// La operación con que terminó (`NSDragOperation`), cuando termina.
    ended: Cell<Option<usize>>,
}

define_class!(
    /// El origen del arrastre: ofrece copiar y anota cómo terminó.
    #[unsafe(super(NSObject))]
    #[name = "AticPillDragSource"]
    #[ivars = State]
    struct Source;

    impl Source {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn mask(&self, _session: &AnyObject, _context: isize) -> usize {
            // Copiar: un archivo soltado en el Finder no se mueve de su lugar.
            COPY
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &AnyObject, _point: NSPoint, operation: usize) {
            self.ivars().ended.set(Some(operation));
        }
    }
);

impl Source {
    fn new() -> Retained<Self> {
        // AppKit pide que el origen declare `NSDraggingSource`: sin eso la
        // sesión no arranca. `define_class!` no lo declara sin los tipos de
        // AppKit, así que se agrega a la clase en tiempo de ejecución.
        static PROTOCOL: std::sync::Once = std::sync::Once::new();
        PROTOCOL.call_once(|| {
            extern "C" {
                fn objc_getProtocol(name: *const std::ffi::c_char) -> *const std::ffi::c_void;
                fn class_addProtocol(class: *const std::ffi::c_void, protocol: *const std::ffi::c_void) -> bool;
            }
            // SAFETY: nombre terminado en 0; la clase es la nuestra, ya registrada.
            unsafe {
                let protocol = objc_getProtocol(c"NSDraggingSource".as_ptr());
                if !protocol.is_null() {
                    let class: *const objc2::runtime::AnyClass = <Self as objc2::ClassType>::class();
                    class_addProtocol(class.cast(), protocol);
                }
            }
        });
        let this = Self::alloc().set_ivars(State::default());
        // SAFETY: `init` de `NSObject`.
        unsafe { msg_send![super(this), init] }
    }
}

/// `NSDragOperationCopy`.
const COPY: usize = 1;
/// Lo más que se espera a que se suelte.
const LIMIT: Duration = Duration::from_secs(120);

pub fn drag_text(text: &str) -> Result<DragOutcome, String> {
    if text.is_empty() {
        return Err("texto vacío".into());
    }
    let writer = NSString::from_str(text);
    // SAFETY: `NSString` es un `NSPasteboardWriting`.
    let writer: Retained<AnyObject> = unsafe { Retained::cast_unchecked(writer) };
    run(vec![(writer, None)])
}

pub fn drag_files(paths: &[String]) -> Result<DragOutcome, String> {
    if paths.is_empty() {
        return Err("sin archivos que arrastrar".into());
    }
    let mut writers = Vec::new();
    for path in paths {
        // SAFETY: `fileURLWithPath:` devuelve una URL nueva, que es un
        // `NSPasteboardWriting`.
        let url: Retained<AnyObject> = unsafe { msg_send![class!(NSURL), fileURLWithPath: &*NSString::from_str(path)] };
        writers.push((url, Some(path.clone())));
    }
    run(writers)
}

fn run(writers: Vec<(Retained<AnyObject>, Option<String>)>) -> Result<DragOutcome, String> {
    if objc2::MainThreadMarker::new().is_none() {
        return Err("el arrastre se empieza en el hilo principal".into());
    }
    let window = super::win::overlay_window().ok_or("el overlay todavía no tiene ventana")?;
    let source = Source::new();
    // SAFETY: AppKit en el hilo principal; la ventana, su vista y los objetos
    // creados aquí viven hasta el final de la función.
    unsafe {
        let view: Option<Retained<AnyObject>> = msg_send![&*window, contentView];
        let view = view.ok_or("el overlay no tiene vista")?;
        // La ventana deja pasar los clics casi siempre: mientras dura el
        // arrastre tiene que recibirlos.
        let ignored: Bool = msg_send![&*window, ignoresMouseEvents];
        let _: () = msg_send![&*window, setIgnoresMouseEvents: Bool::NO];

        // El cursor en la ventana (AppKit: origen abajo a la izquierda).
        let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let at: NSPoint = msg_send![&*window, convertPointFromScreen: mouse];
        let number: isize = msg_send![&*window, windowNumber];
        let uptime: f64 = {
            let info: Retained<AnyObject> = msg_send![class!(NSProcessInfo), processInfo];
            msg_send![&info, systemUptime]
        };
        let none: *const AnyObject = std::ptr::null();
        // `NSEventTypeLeftMouseDragged`.
        let event: Option<Retained<AnyObject>> = msg_send![
            class!(NSEvent),
            mouseEventWithType: 6usize,
            location: at,
            modifierFlags: 0usize,
            timestamp: uptime,
            windowNumber: number,
            context: none,
            eventNumber: 0isize,
            clickCount: 1isize,
            pressure: 1.0f32
        ];
        let event = event.ok_or("no se pudo armar el evento del arrastre")?;

        let mut items = Vec::new();
        for (index, (writer, path)) in writers.iter().enumerate() {
            let item: Allocated<AnyObject> = msg_send![class!(NSDraggingItem), alloc];
            let item: Retained<AnyObject> = msg_send![item, initWithPasteboardWriter: &**writer];
            let image = preview(path.as_deref());
            let side = 48.0;
            let offset = index as f64 * 6.0;
            let frame = NSRect::new(NSPoint::new(at.x - side / 2.0 + offset, at.y - side / 2.0 - offset), NSSize::new(side, side));
            let _: () = msg_send![&item, setDraggingFrame: frame, contents: &*image];
            items.push(item);
        }
        let refs: Vec<&AnyObject> = items.iter().map(|i| &**i).collect();
        let items = NSArray::from_slice(&refs);
        let session: Option<Retained<AnyObject>> =
            msg_send![&view, beginDraggingSessionWithItems: &*items, event: &*event, source: &*source];
        let Some(session) = session else {
            let _: () = msg_send![&*window, setIgnoresMouseEvents: ignored];
            return Err("AppKit no empezó el arrastre".into());
        };
        let _: () = msg_send![&session, setAnimatesToStartingPositionsOnCancelOrFail: Bool::YES];
        // La ventana de la pill cubre la pantalla y acepta lo que se suelta
        // (GPUI recibe archivos): mientras dura el arrastre no es destino, si
        // no todo caía en ella y nunca en la app de abajo.
        let _: () = msg_send![&*window, setIgnoresMouseEvents: Bool::YES];
        let types: Option<Retained<AnyObject>> = msg_send![&view, registeredDraggedTypes];
        let _: () = msg_send![&view, unregisterDraggedTypes];

        // Atender los eventos hasta que se suelte, como el loop de `DoDragDrop`.
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let mode = NSString::from_str("kCFRunLoopDefaultMode");
        let started = Instant::now();
        while source.ivars().ended.get().is_none() && started.elapsed() < LIMIT {
            let until: Retained<AnyObject> = msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 0.05f64];
            let next: Option<Retained<AnyObject>> =
                msg_send![&app, nextEventMatchingMask: u64::MAX, untilDate: &*until, inMode: &*mode, dequeue: Bool::YES];
            if let Some(next) = next {
                let _: () = msg_send![&app, sendEvent: &*next];
            }
        }
        if let Some(types) = types {
            let _: () = msg_send![&view, registerForDraggedTypes: &*types];
        }
        let _: () = msg_send![&*window, setIgnoresMouseEvents: ignored];
        let operation = source.ivars().ended.get().unwrap_or(0);
        Ok(DragOutcome {
            dropped: operation != 0,
            effect: operation as u32,
        })
    }
}

/// La miniatura: el ícono del archivo (o la imagen misma) o, para texto, el
/// ícono de texto del sistema.
///
/// SAFETY: en el hilo principal.
unsafe fn preview(path: Option<&str>) -> Retained<AnyObject> {
    let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
    let image: Option<Retained<AnyObject>> = match path {
        Some(path) => {
            let is_image = std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "tiff"));
            let own: Option<Retained<AnyObject>> = if is_image {
                let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
                msg_send![image, initWithContentsOfFile: &*NSString::from_str(path)]
            } else {
                None
            };
            own.or_else(|| msg_send![&workspace, iconForFile: &*NSString::from_str(path)])
        }
        None => {
            let name = NSString::from_str("public.plain-text");
            let kind: Option<Retained<AnyObject>> = match objc2::runtime::AnyClass::get(c"UTType") {
                Some(class) => msg_send![class, typeWithIdentifier: &*name],
                None => None,
            };
            kind.and_then(|kind| msg_send![&workspace, iconForContentType: &*kind])
        }
    };
    image.unwrap_or_else(|| msg_send![class!(NSImage), new])
}
