//! Atajos globales con `RegisterEventHotKey` de Carbon, como el crate
//! `global-hotkey` que usa Atic en Tauri. No piden permisos, la app con el
//! foco no recibe la tecla y avisan al apretar y al soltar.
//!
//! Se registran en el hilo principal: AppKit entrega los eventos en su loop,
//! que es el de GPUI (`main_thread::run` lleva el trabajo hasta ahí).

use std::ffi::c_void;
use std::sync::Mutex;

pub const CMD: u32 = 0x100;
pub const SHIFT: u32 = 0x200;
pub const OPTION: u32 = 0x800;
pub const CONTROL: u32 = 0x1000;

type Callback = Box<dyn Fn(bool) + Send>;

struct Registered {
    callback: Callback,
    /// El `EventHotKeyRef`, para soltarlo.
    hotkey: usize,
}

/// Lo que hace cada atajo; el índice es su id en Carbon. Un atajo soltado
/// queda como `None` para no mover los ids de los demás.
static HOTKEYS: Mutex<Vec<Option<Registered>>> = Mutex::new(Vec::new());

/// Registra `key` (un `kVK_*`) con `modifiers`; `callback` recibe `true` al
/// apretar y `false` al soltar. Devuelve el id para `unregister`. Solo en el
/// hilo principal.
pub fn register(key: u16, modifiers: u32, callback: impl Fn(bool) + Send + 'static) -> Result<u32, String> {
    install_handler()?;
    let mut hotkeys = HOTKEYS.lock().map_err(|e| e.to_string())?;
    let id = hotkeys.iter().position(Option::is_none).unwrap_or(hotkeys.len()) as u32;
    let hotkey_id = EventHotKeyID {
        signature: SIGNATURE,
        id,
    };
    let mut hotkey: *mut c_void = std::ptr::null_mut();
    // SAFETY: Carbon copia el id; `hotkey` recibe la referencia nueva.
    let status = unsafe {
        RegisterEventHotKey(key as u32, modifiers, hotkey_id, GetApplicationEventTarget(), 0, &mut hotkey)
    };
    if status != 0 {
        return Err(format!("RegisterEventHotKey devolvió {status} (¿lo usa otra app?)"));
    }
    let entry = Some(Registered {
        callback: Box::new(callback),
        hotkey: hotkey as usize,
    });
    if (id as usize) < hotkeys.len() {
        hotkeys[id as usize] = entry;
    } else {
        hotkeys.push(entry);
    }
    Ok(id)
}

/// Suelta todos los atajos. Solo en el hilo principal.
pub fn unregister_all() {
    let Ok(mut hotkeys) = HOTKEYS.lock() else {
        return;
    };
    for entry in hotkeys.drain(..).flatten() {
        // SAFETY: la referencia vino de `RegisterEventHotKey` y se suelta una vez.
        unsafe {
            UnregisterEventHotKey(entry.hotkey as *mut c_void);
        }
    }
}

fn install_handler() -> Result<(), String> {
    static INSTALLED: Mutex<bool> = Mutex::new(false);
    let mut installed = INSTALLED.lock().map_err(|e| e.to_string())?;
    if *installed {
        return Ok(());
    }
    let types = [
        EventTypeSpec {
            class: KEYBOARD_CLASS,
            kind: HOT_KEY_PRESSED,
        },
        EventTypeSpec {
            class: KEYBOARD_CLASS,
            kind: HOT_KEY_RELEASED,
        },
    ];
    // SAFETY: `handler` vive todo el proceso; los tipos se copian.
    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            handler,
            types.len() as u32,
            types.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!("InstallEventHandler devolvió {status}"));
    }
    *installed = true;
    Ok(())
}

extern "C" fn handler(_next: *mut c_void, event: *mut c_void, _data: *mut c_void) -> i32 {
    let mut hotkey_id = EventHotKeyID { signature: 0, id: 0 };
    // SAFETY: el evento es de los tipos pedidos y trae un `EventHotKeyID`.
    let status = unsafe {
        GetEventParameter(
            event,
            DIRECT_OBJECT,
            TYPE_HOT_KEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>(),
            std::ptr::null_mut(),
            &mut hotkey_id as *mut _ as *mut c_void,
        )
    };
    if status != 0 || hotkey_id.signature != SIGNATURE {
        return EVENT_NOT_HANDLED;
    }
    // SAFETY: el evento sigue vivo durante el manejador.
    let pressed = unsafe { GetEventKind(event) } == HOT_KEY_PRESSED;
    if let Ok(hotkeys) = HOTKEYS.lock() {
        if let Some(Some(entry)) = hotkeys.get(hotkey_id.id as usize) {
            (entry.callback)(pressed);
        }
    }
    0
}

const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SIGNATURE: u32 = four_cc(b"pill");
const KEYBOARD_CLASS: u32 = four_cc(b"keyb");
const HOT_KEY_PRESSED: u32 = 5;
const HOT_KEY_RELEASED: u32 = 6;
const DIRECT_OBJECT: u32 = four_cc(b"----");
const TYPE_HOT_KEY_ID: u32 = four_cc(b"hkid");
const EVENT_NOT_HANDLED: i32 = -9874;

#[repr(C)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

type EventHandler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandler,
        count: u32,
        types: *const EventTypeSpec,
        data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        kind: u32,
        actual_kind: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
    fn GetEventKind(event: *mut c_void) -> u32;
}
