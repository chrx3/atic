//! El mouse durante una grabación de pantalla: dónde estuvo y cuándo se
//! apretó, para que el editor (`recording_editor.rs`) pueda acercarse a los
//! clics, seguir al cursor y, más adelante, dibujarlo suavizado.
//!
//! Un hilo lee la posición y los botones a 60 Hz y los marca con el reloj de
//! la grabación (`RecordClock`): lo de las pausas no queda, igual que en el
//! video y el audio. No pide permisos: se lee el estado del sistema, no se
//! escuchan los eventos.
//!
//! Se guarda en `.atic/<nombre>.json`, junto al video (`save`), con las
//! posiciones relativas a la zona grabada (0–1): no importan la escala ni la
//! pantalla.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use atic_capture::Rect as PhysRect;
use serde::{Deserialize, Serialize};

use crate::screen_audio::RecordClock;

const TICK: Duration = Duration::from_millis(16);
/// Quieto, igual se anota cada tanto: así el editor sabe que no se movió.
const KEEP_MS: u32 = 500;

/// Una lectura: milisegundos desde que empezó el video, posición en la zona
/// (0–1, puede salirse) y si el botón principal estaba apretado.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample(pub u32, pub f32, pub f32, pub bool);

/// Lo que queda junto a cada grabación.
#[derive(Debug, Serialize, Deserialize)]
pub struct Track {
    pub version: u32,
    /// La zona grabada, en las coordenadas del sistema (puntos en macOS,
    /// píxeles en Windows).
    pub region: [i32; 4],
    pub samples: Vec<Sample>,
    /// Se grabó sin el cursor del sistema («Cursor suave»): el editor lo dibuja.
    #[serde(default)]
    pub cursor_hidden: bool,
    /// Cuándo se escribió (ms; una marca por tecla, sin guardar cuál).
    #[serde(default)]
    pub keys: Vec<u32>,
    /// Dónde estaba el texto que se escribía (ms y posición 0–1 en la zona): el
    /// cursor de texto o, si la app no lo dice, el campo enfocado (Accesibilidad).
    #[serde(default)]
    pub carets: Vec<(u32, f32, f32)>,
    /// La forma del cursor del sistema en cada cambio: (ms, código). 0 flecha, 1 mano,
    /// 2 I-beam, 3 cruz. Solo se anota cuando cambia.
    #[serde(default)]
    pub shapes: Vec<(u32, u8)>,
}

/// Los códigos de `Track::shapes`.
pub const SHAPE_ARROW: u8 = 0;
pub const SHAPE_HAND: u8 = 1;
pub const SHAPE_IBEAM: u8 = 2;
pub const SHAPE_CROSS: u8 = 3;

impl Track {
    /// La forma del cursor en `ms`: el último cambio anotado hasta ahí (0, la flecha,
    /// si no hay datos o aún no había cambiado).
    pub fn shape_at(&self, ms: u32) -> u8 {
        let after = self.shapes.partition_point(|(at, _)| *at <= ms);
        after.checked_sub(1).map_or(SHAPE_ARROW, |i| self.shapes[i].1)
    }

    /// Los momentos en que se apretó el botón (ms).
    pub fn clicks(&self) -> Vec<u32> {
        let mut down = false;
        let mut out = Vec::new();
        for Sample(t, _, _, pressed) in &self.samples {
            if *pressed && !down {
                out.push(*t);
            }
            down = *pressed;
        }
        out
    }
}

/// `.atic/<nombre>.master.mp4`: con «Cursor suave», la grabación sin cursor que
/// usa el editor (la de afuera ya lo tiene dibujado).
pub fn master_path(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    Some(video.parent()?.join(".atic").join(format!("{stem}.master.mp4")))
}

/// `.atic/<nombre>.baking.mp4`: donde se dibuja el cursor mientras dura; al terminar
/// reemplaza al video de afuera de una sola vez (`bake_cursor`).
pub fn baking_path(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    Some(video.parent()?.join(".atic").join(format!("{stem}.baking.mp4")))
}

/// `.atic/<nombre>.audio.wav`: el audio de una grabación que no se pudo juntar con el
/// video (48 kHz, estéreo, 16 bits), para no perderlo.
pub fn audio_path(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    Some(video.parent()?.join(".atic").join(format!("{stem}.audio.wav")))
}

/// `.atic/<nombre>.thumb.jpg`: la miniatura de la grabación (la de su primer
/// cuadro), para listarla sin abrir el video.
pub fn thumb_path(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    Some(video.parent()?.join(".atic").join(format!("{stem}.thumb.jpg")))
}

/// `.atic/<nombre>.json` junto al video.
pub fn track_path(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?;
    Some(video.parent()?.join(".atic").join(stem).with_extension("json"))
}

pub fn load(video: &Path) -> Option<Track> {
    let text = std::fs::read_to_string(track_path(video)?).ok()?;
    serde_json::from_str(&text).ok()
}

/// El registro en curso; `finish` lo cierra.
pub struct CursorTrack {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Recorded>>,
    region: PhysRect,
}

impl CursorTrack {
    pub fn start(region: PhysRect, clock: Arc<RecordClock>) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("grabar mouse".into())
                .spawn(move || sample(region, &clock, &stop))
                .ok()?
        };
        Some(Self { stop, worker: Some(worker), region })
    }

    /// Deja de anotar (al parar la grabación: lo que sigue ya no es video).
    pub fn halt(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn finish(mut self) -> Track {
        self.halt();
        let Recorded { samples, keys, carets, shapes } = self.worker.take().and_then(|worker| worker.join().ok()).unwrap_or_default();
        let r = self.region;
        Track { version: 1, region: [r.x, r.y, r.width as i32, r.height as i32], samples, cursor_hidden: false, keys, carets, shapes }
    }
}

impl Drop for CursorTrack {
    /// Descartada o fallida: el hilo termina igual.
    fn drop(&mut self) {
        self.halt();
    }
}

/// Guarda el registro de `video`.
pub fn save(track: &Track, video: &Path) -> Result<(), String> {
    let path = track_path(video).ok_or("sin nombre para el registro del mouse")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(track).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Lo que junta el hilo mientras graba.
#[derive(Default)]
struct Recorded {
    samples: Vec<Sample>,
    keys: Vec<u32>,
    carets: Vec<(u32, f32, f32)>,
    shapes: Vec<(u32, u8)>,
}

/// Escribiendo, el lugar del texto se lee cada tanto (Accesibilidad es lenta).
const CARET_EVERY_MS: u32 = 250;
/// La forma del cursor se pide cada dos vueltas del hilo (~30 veces por segundo):
/// con la mitad de las muestras basta y cuesta la mitad en el hilo principal.
const SHAPE_EVERY_MS: u32 = 30;

fn sample(region: PhysRect, clock: &RecordClock, stop: &AtomicBool) -> Recorded {
    let (w, h) = (region.width.max(1) as f32, region.height.max(1) as f32);
    let mut rec = Recorded::default();
    let mut presses = key_presses();
    let mut last_caret: Option<u32> = None;
    // Si la lectura de la forma falla una vez, se apaga: se sigue grabando con la flecha.
    let mut shape_reader = shape_probe::Probe::new();
    let mut last_shape: Option<u32> = None;
    while !stop.load(Ordering::Acquire) {
        std::thread::sleep(TICK);
        let Some(at) = clock.now() else {
            presses = key_presses();
            continue;
        };
        let t = at.as_millis() as u32;
        if let Some(probe) = shape_reader.as_ref() {
            if last_shape.is_none_or(|last| t.saturating_sub(last) >= SHAPE_EVERY_MS) {
                last_shape = Some(t);
                match probe.poll() {
                    ShapeRead::Shape(code) => {
                        push_shape(&mut rec.shapes, t, code);
                    }
                    ShapeRead::Waiting => {}
                    ShapeRead::Off => shape_reader = None,
                }
            }
        }
        // El teclado: cuántas teclas se apretaron desde la vuelta anterior.
        let now_presses = key_presses();
        if let (Some(before), Some(after)) = (presses, now_presses) {
            let typed = after.wrapping_sub(before).min(20);
            if typed > 0 {
                rec.keys.extend(std::iter::repeat_n(t, typed as usize));
                if last_caret.is_none_or(|last| t.saturating_sub(last) >= CARET_EVERY_MS) {
                    last_caret = Some(t);
                    if let Some((x, y)) = typing_point() {
                        rec.carets.push((t, (x - region.x as f32) / w, (y - region.y as f32) / h));
                    }
                }
            }
        }
        presses = now_presses;
        let out = &mut rec.samples;
        let Some((x, y, down)) = read() else { continue };
        let s = Sample(t, (x - region.x as f32) / w, (y - region.y as f32) / h, down);
        let changed = out.last().is_none_or(|last| {
            (last.1 - s.1).abs() > 0.0001 || (last.2 - s.2).abs() > 0.0001 || last.3 != s.3 || t.saturating_sub(last.0) >= KEEP_MS
        });
        if changed {
            out.push(s);
        }
    }
    rec
}

/// Cuántas teclas se han apretado desde que empezó la sesión (solo la cuenta; no
/// se lee cuáles, ni pide permiso).
#[cfg(target_os = "macos")]
fn key_presses() -> Option<u32> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCounterForEventType(state: i32, kind: u32) -> u32;
    }
    // `kCGEventSourceStateCombinedSessionState` y `kCGEventKeyDown`.
    Some(unsafe { CGEventSourceCounterForEventType(0, 10) })
}

/// Cuántas teclas se han apretado desde que empezó la sesión (solo la cuenta; no
/// se lee cuáles, ni pide permiso).
#[cfg(windows)]
fn key_presses() -> Option<u32> {
    use std::cell::Cell;

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    /// De `VK_BACK` a `VK_OEM_CLEAR`: 247 casillas en 4 palabras. Los botones del
    /// mouse (`VK_LBUTTON`…`VK_XBUTTON2`, 0x01–0x06) quedan fuera solos y los
    /// modificadores se saltan abajo.
    const FIRST: u32 = 0x08;
    const LAST: u32 = 0xFE;
    const WORDS: usize = 4;

    /// Qué teclas estaban apretadas en la vuelta anterior (una casilla por `VK`)
    /// y cuántas se apretaron en total. La cuenta la lleva el hilo `grabar mouse`.
    #[derive(Clone, Copy)]
    struct Counter {
        down: [u64; WORDS],
        total: u32,
    }

    thread_local! {
        static COUNTER: Cell<Counter> = Cell::new(Counter { down: [0; WORDS], total: 0 });
    }

    COUNTER.with(|cell| {
        let mut counter = cell.get();
        let mut now = [0u64; WORDS];
        for vk in FIRST..=LAST {
            // Shift, Ctrl, Alt, Win y Bloq Mayús solos no son escribir (Ctrl+C no debe
            // llevar el zoom al texto).
            if matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5) {
                continue;
            }
            // SAFETY: lee el estado de una tecla del sistema; no abre nada ni
            // pide permiso.
            if unsafe { GetAsyncKeyState(vk as i32) } < 0 {
                let bit = (vk - FIRST) as usize;
                now[bit / 64] |= 1 << (bit % 64);
            }
        }
        // Solo los flancos de subida: lo que ya estaba apretado no vuelve a contar.
        let fresh: u32 = now.iter().zip(counter.down.iter()).map(|(now, before)| (*now & !*before).count_ones()).sum();
        counter.total = counter.total.wrapping_add(fresh);
        counter.down = now;
        cell.set(counter);
        Some(counter.total)
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn key_presses() -> Option<u32> {
    None
}

/// Dónde se está escribiendo (en las coordenadas de la zona): el cursor de texto
/// del campo enfocado o, si la app no lo dice, el centro del campo. Con
/// Accesibilidad; sin permiso, nada.
#[cfg(target_os = "macos")]
fn typing_point() -> Option<(f32, f32)> {
    use std::ffi::c_void;
    type Ref = *const c_void;
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementCreateSystemWide() -> Ref;
        fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXUIElementCopyParameterizedAttributeValue(element: Ref, attribute: Ref, parameter: Ref, value: *mut Ref) -> i32;
        fn AXValueGetValue(value: Ref, kind: u32, out: *mut c_void) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: Ref);
    }
    struct Owned(Ref);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: se copió con +1.
                unsafe { CFRelease(self.0) };
            }
        }
    }
    // Los nombres de los atributos: `NSString` tiene puente con `CFString`.
    let name = |s: &str| objc2_foundation::NSString::from_str(s);
    let as_ref = |s: &objc2::rc::Retained<objc2_foundation::NSString>| objc2::rc::Retained::as_ptr(s) as Ref;
    // SAFETY: llamadas de Accesibilidad sobre objetos que se sueltan al salir.
    unsafe {
        if !AXIsProcessTrusted() {
            return None;
        }
        let system = Owned(AXUIElementCreateSystemWide());
        AXUIElementSetMessagingTimeout(system.0, 0.05);
        let copy = |element: Ref, attribute: &str| -> Option<Owned> {
            let mut out: Ref = std::ptr::null();
            let attr = name(attribute);
            let status = AXUIElementCopyAttributeValue(element, as_ref(&attr), &mut out);
            (status == 0 && !out.is_null()).then(|| Owned(out))
        };
        let focused = copy(system.0, "AXFocusedUIElement")?;
        // El cursor de texto: el rectángulo de lo seleccionado (o del punto de inserción).
        if let Some(range) = copy(focused.0, "AXSelectedTextRange") {
            let mut out: Ref = std::ptr::null();
            let attr = name("AXBoundsForRange");
            let status = AXUIElementCopyParameterizedAttributeValue(focused.0, as_ref(&attr), range.0, &mut out);
            if status == 0 && !out.is_null() {
                let bounds = Owned(out);
                let mut rect = Rect::default();
                // `kAXValueCGRectType`.
                if AXValueGetValue(bounds.0, 3, &mut rect as *mut Rect as *mut c_void) && (rect.w > 0.0 || rect.h > 0.0 || rect.x != 0.0) {
                    return Some(((rect.x + rect.w / 2.0) as f32, (rect.y + rect.h / 2.0) as f32));
                }
            }
        }
        // Si no, el centro del campo (solo si es chico: el de un documento entero no dice nada).
        let position = copy(focused.0, "AXPosition")?;
        let size = copy(focused.0, "AXSize")?;
        let mut p = [0f64; 2];
        let mut s = [0f64; 2];
        // `kAXValueCGPointType` y `kAXValueCGSizeType`.
        if !AXValueGetValue(position.0, 1, p.as_mut_ptr() as *mut c_void) || !AXValueGetValue(size.0, 2, s.as_mut_ptr() as *mut c_void) {
            return None;
        }
        (s[1] < 200.0).then(|| ((p[0] + s[0] / 2.0) as f32, (p[1] + s[1] / 2.0) as f32))
    }
}

/// Dónde se está escribiendo: el cursor de texto (`rcCaret`) de la ventana en
/// primer plano, en píxeles de pantalla. Windows lo da sin permiso al hilo que
/// está enfocado; si la app no tiene caret (o está elevada), nada.
#[cfg(windows)]
fn typing_point() -> Option<(f32, f32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO};

    // SAFETY: lecturas del estado del sistema; `info` va con su `cbSize`.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        // Solo interesa el hilo de la ventana (el proceso se ignora).
        let thread = GetWindowThreadProcessId(window, std::ptr::null_mut());
        if thread == 0 {
            return None;
        }
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndCaret.is_null() {
            return None;
        }
        let rc = info.rcCaret;
        // Sin área no hay dónde escribir (algunos campos devuelven un rectángulo vacío).
        if rc.right <= rc.left || rc.bottom <= rc.top {
            return None;
        }
        // `rcCaret` viene en las coordenadas del cliente: al centro, a pantalla.
        let mut p = POINT { x: (rc.left + rc.right) / 2, y: (rc.top + rc.bottom) / 2 };
        if ClientToScreen(info.hwndCaret, &mut p) == 0 {
            return None;
        }
        Some((p.x as f32, p.y as f32))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn typing_point() -> Option<(f32, f32)> {
    None
}

/// La posición del cursor (en las coordenadas de la zona) y el botón principal.
#[cfg(target_os = "macos")]
fn read() -> Option<(f32, f32, bool)> {
    use std::ffi::c_void;
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    // Las mismas funciones que `platform/macos/control.rs` y `keys.rs`, con tipos propios.
    #[allow(clashing_extern_declarations)]
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *const c_void;
        fn CGEventGetLocation(event: *const c_void) -> CGPoint;
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    // SAFETY: un evento vacío solo para leer dónde está el cursor (puntos,
    // origen arriba a la izquierda, como la zona); se suelta al tiro.
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return None;
        }
        let p = CGEventGetLocation(event);
        CFRelease(event);
        // `kCGEventSourceStateCombinedSessionState` y el botón izquierdo.
        let down = CGEventSourceButtonState(0, 0);
        Some((p.x as f32, p.y as f32, down))
    }
}

#[cfg(windows)]
fn read() -> Option<(f32, f32, bool)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: lecturas del estado del sistema.
    unsafe {
        if GetCursorPos(&mut p) == 0 {
            return None;
        }
        Some((p.x as f32, p.y as f32, GetAsyncKeyState(VK_LBUTTON as i32) < 0))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn read() -> Option<(f32, f32, bool)> {
    None
}

/// Anota la forma solo si cambió (la primera, si no es la flecha, que es lo que
/// `shape_at` dice sin datos). `true` si agregó una marca.
fn push_shape(shapes: &mut Vec<(u32, u8)>, t: u32, code: u8) -> bool {
    if shapes.last().map_or(SHAPE_ARROW, |last| last.1) == code {
        return false;
    }
    shapes.push((t, code));
    true
}

/// Lo que devuelve el sondeo de la forma del cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ShapeRead {
    /// La forma que tiene el cursor ahora.
    Shape(u8),
    /// Todavía no hay lectura (el hilo principal no ha contestado).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Waiting,
    /// La lectura falló: no se vuelve a intentar.
    Off,
}

/// La firma de un cursor del sistema: el tamaño de su imagen y su punto caliente
/// (puntos). Con eso se distingue la flecha de la mano, el I-beam y la cruz.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct ShapeSig {
    size: (f64, f64),
    spot: (f64, f64),
}

/// Qué código le toca a `current` entre los cursores conocidos: el primero con la
/// misma firma (a media unidad) o, si ninguno calza, la flecha.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn classify_shape(current: &ShapeSig, known: &[(u8, ShapeSig)]) -> u8 {
    const TOLERANCE: f64 = 0.5;
    let near = |a: f64, b: f64| (a - b).abs() <= TOLERANCE;
    known
        .iter()
        .find(|(_, sig)| {
            near(current.size.0, sig.size.0) && near(current.size.1, sig.size.1) && near(current.spot.0, sig.spot.0) && near(current.spot.1, sig.spot.1)
        })
        .map_or(SHAPE_ARROW, |(code, _)| *code)
}

/// La forma del cursor del sistema. AppKit no es seguro fuera del hilo principal
/// (`NSCursor` no es seguro entre hilos y crea objetos autoliberados), así que el
/// hilo `grabar mouse` solo pide la lectura al hilo principal (`dispatch_async`) y
/// lee un atómico con la última respuesta: nunca espera a AppKit ni lo toca. Si
/// el hilo principal está ocupado, no se apilan pedidos (uno en vuelo a la vez).
#[cfg(target_os = "macos")]
mod shape_probe {
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::{Arc, OnceLock};

    use super::{classify_shape, ShapeRead, ShapeSig, SHAPE_ARROW, SHAPE_CROSS, SHAPE_HAND, SHAPE_IBEAM};

    /// Aún sin respuesta, y la lectura falló (no se reintenta).
    const NONE: u8 = 254;
    const FAILED: u8 = 255;

    pub struct Probe {
        latest: Arc<AtomicU8>,
        inflight: Arc<AtomicBool>,
    }

    impl Probe {
        pub fn new() -> Option<Self> {
            Some(Self { latest: Arc::new(AtomicU8::new(NONE)), inflight: Arc::new(AtomicBool::new(false)) })
        }

        /// La última forma conocida y, si no hay un pedido en curso, pide otra al
        /// hilo principal para la próxima vuelta.
        pub fn poll(&self) -> ShapeRead {
            let value = self.latest.load(Ordering::Acquire);
            if value == FAILED {
                return ShapeRead::Off;
            }
            if !self.inflight.swap(true, Ordering::AcqRel) {
                let latest = self.latest.clone();
                let inflight = self.inflight.clone();
                crate::platform::main_thread::run(move || {
                    // Un pánico (por ejemplo, un selector que objc2 rechaza en debug) no
                    // debe tumbar la app: la forma se apaga y la grabación sigue.
                    let read = std::panic::catch_unwind(read_shape).unwrap_or(Err(()));
                    match read {
                        Ok(Some(code)) => latest.store(code, Ordering::Release),
                        Ok(None) => {}
                        Err(()) => latest.store(FAILED, Ordering::Release),
                    }
                    inflight.store(false, Ordering::Release);
                });
            }
            match value {
                NONE => ShapeRead::Waiting,
                code => ShapeRead::Shape(code),
            }
        }
    }

    /// `Ok(Some)` la forma; `Ok(None)` el sistema no dijo cuál (se queda la anterior);
    /// `Err` algo no está donde se esperaba. Solo en el hilo principal.
    fn read_shape() -> Result<Option<u8>, ()> {
        use objc2::rc::{autoreleasepool, Retained};
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2::msg_send;
        use objc2_foundation::{NSPoint, NSSize};

        /// Firma de un `NSCursor`: tamaño de su imagen y punto caliente.
        fn signature(cursor: &AnyObject) -> Option<ShapeSig> {
            // SAFETY: getters de solo lectura de `NSCursor` y `NSImage`, en el hilo principal.
            unsafe {
                let image: Option<Retained<AnyObject>> = msg_send![cursor, image];
                let image = image?;
                let size: NSSize = msg_send![&*image, size];
                let spot: NSPoint = msg_send![cursor, hotSpot];
                Some(ShapeSig { size: (size.width, size.height), spot: (spot.x, spot.y) })
            }
        }

        // Las firmas de los cursores del sistema, una sola vez. `None`: no se pudieron
        // calcular (no se vuelve a intentar).
        static KNOWN: OnceLock<Option<Vec<(u8, ShapeSig)>>> = OnceLock::new();

        autoreleasepool(|_| {
            let class = AnyClass::get(c"NSCursor").ok_or(())?;
            let known = KNOWN
                .get_or_init(|| {
                    // SAFETY: constructores de clase de `NSCursor` (los cursores estándar).
                    let standard = unsafe {
                        let arrow: Option<Retained<AnyObject>> = msg_send![class, arrowCursor];
                        let hand: Option<Retained<AnyObject>> = msg_send![class, pointingHandCursor];
                        let ibeam: Option<Retained<AnyObject>> = msg_send![class, IBeamCursor];
                        let cross: Option<Retained<AnyObject>> = msg_send![class, crosshairCursor];
                        [(SHAPE_ARROW, arrow), (SHAPE_HAND, hand), (SHAPE_IBEAM, ibeam), (SHAPE_CROSS, cross)]
                    };
                    standard.into_iter().map(|(code, cursor)| Some((code, signature(&*cursor?)?))).collect::<Option<Vec<_>>>()
                })
                .as_ref()
                .ok_or(())?;
            // SAFETY: `currentSystemCursor` es un getter de clase de solo lectura (el que se ve ahora).
            let current: Option<Retained<AnyObject>> = unsafe { msg_send![class, currentSystemCursor] };
            Ok(current.and_then(|cursor| signature(&cursor)).map(|sig| classify_shape(&sig, known)))
        })
    }
}

/// La forma del cursor del sistema en Windows: `GetCursorInfo` da el `HCURSOR`
/// que se está viendo y se compara con los estándar de `LoadCursorW`, que son los
/// mismos objetos (coinciden por handle, no por imagen). Se lee desde el hilo
/// `grabar mouse`: no depende de la interfaz de la app.
#[cfg(windows)]
mod shape_probe {
    use std::cell::Cell;

    use windows_sys::core::PCWSTR;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorInfo, LoadCursorW, CURSORINFO, CURSOR_SHOWING, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM,
    };

    use super::{ShapeRead, SHAPE_ARROW, SHAPE_CROSS, SHAPE_HAND, SHAPE_IBEAM};

    pub struct Probe {
        /// El handle de cada cursor estándar y el código que le toca.
        known: [(isize, u8); 4],
        /// La última forma vista: con un cursor de otra app o uno oculto se queda
        /// esta, que es lo que el usuario venía viendo.
        last: Cell<u8>,
    }

    impl Probe {
        /// `None` si el sistema no dio ningún cursor estándar: no hay qué distinguir.
        pub fn new() -> Option<Self> {
            // SAFETY: los cursores del sistema son recursos compartidos: solo se
            // piden sus handles, no se crean ni se destruyen.
            let load = |id: PCWSTR| unsafe { LoadCursorW(std::ptr::null_mut(), id) };
            let known = [
                (load(IDC_ARROW) as isize, SHAPE_ARROW),
                (load(IDC_HAND) as isize, SHAPE_HAND),
                (load(IDC_IBEAM) as isize, SHAPE_IBEAM),
                (load(IDC_CROSS) as isize, SHAPE_CROSS),
            ];
            if known.iter().all(|(handle, _)| *handle == 0) {
                return None;
            }
            Some(Self { known, last: Cell::new(SHAPE_ARROW) })
        }

        pub fn poll(&self) -> ShapeRead {
            let mut info: CURSORINFO = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
            // SAFETY: `info` va con su `cbSize` y es de este hilo.
            if unsafe { GetCursorInfo(&mut info) } == 0 {
                return ShapeRead::Off;
            }
            // Oculto o dibujado por otra app: se queda la última forma vista.
            if info.flags & CURSOR_SHOWING == 0 || info.hCursor.is_null() {
                return ShapeRead::Shape(self.last.get());
            }
            let current = info.hCursor as isize;
            let code = self.known.iter().find(|(handle, _)| *handle == current).map_or(self.last.get(), |(_, code)| *code);
            self.last.set(code);
            ShapeRead::Shape(code)
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod shape_probe {
    use super::ShapeRead;

    /// En otros sistemas no se leen formas: `shape_at` dice siempre la flecha.
    pub struct Probe;

    impl Probe {
        pub fn new() -> Option<Self> {
            None
        }

        pub fn poll(&self) -> ShapeRead {
            ShapeRead::Off
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(shapes: Vec<(u32, u8)>) -> Track {
        Track { version: 1, region: [0, 0, 100, 100], samples: Vec::new(), cursor_hidden: false, keys: Vec::new(), carets: Vec::new(), shapes }
    }

    #[test]
    fn shape_at_sin_datos_es_la_flecha() {
        assert_eq!(track(Vec::new()).shape_at(0), SHAPE_ARROW);
        assert_eq!(track(Vec::new()).shape_at(5_000), SHAPE_ARROW);
    }

    #[test]
    fn shape_at_toma_el_ultimo_cambio_hasta_ese_momento() {
        let t = track(vec![(100, SHAPE_HAND), (400, SHAPE_IBEAM), (900, SHAPE_ARROW)]);
        // Antes del primer cambio, la flecha.
        assert_eq!(t.shape_at(0), SHAPE_ARROW);
        assert_eq!(t.shape_at(99), SHAPE_ARROW);
        // El cambio mismo ya cuenta.
        assert_eq!(t.shape_at(100), SHAPE_HAND);
        assert_eq!(t.shape_at(399), SHAPE_HAND);
        assert_eq!(t.shape_at(400), SHAPE_IBEAM);
        assert_eq!(t.shape_at(899), SHAPE_IBEAM);
        assert_eq!(t.shape_at(900), SHAPE_ARROW);
        // Después del último, se queda.
        assert_eq!(t.shape_at(u32::MAX), SHAPE_ARROW);
        assert_eq!(track(vec![(0, SHAPE_CROSS)]).shape_at(0), SHAPE_CROSS);
    }

    #[test]
    fn solo_se_anota_al_cambiar() {
        let mut shapes = Vec::new();
        // La flecha inicial no se anota: es lo que `shape_at` dice sin datos.
        assert!(!push_shape(&mut shapes, 0, SHAPE_ARROW));
        assert!(!push_shape(&mut shapes, 30, SHAPE_ARROW));
        assert!(push_shape(&mut shapes, 66, SHAPE_HAND));
        assert!(!push_shape(&mut shapes, 99, SHAPE_HAND));
        assert!(!push_shape(&mut shapes, 132, SHAPE_HAND));
        assert!(push_shape(&mut shapes, 165, SHAPE_IBEAM));
        assert!(push_shape(&mut shapes, 198, SHAPE_ARROW));
        assert!(!push_shape(&mut shapes, 231, SHAPE_ARROW));
        assert_eq!(shapes, vec![(66, SHAPE_HAND), (165, SHAPE_IBEAM), (198, SHAPE_ARROW)]);
    }

    #[test]
    fn la_firma_distingue_los_cursores_y_lo_demas_es_flecha() {
        let sig = |w: f64, h: f64, x: f64, y: f64| ShapeSig { size: (w, h), spot: (x, y) };
        let known = [
            (SHAPE_ARROW, sig(17.0, 23.0, 4.0, 4.0)),
            (SHAPE_HAND, sig(32.0, 32.0, 13.0, 8.0)),
            (SHAPE_IBEAM, sig(9.0, 18.0, 4.0, 9.0)),
            (SHAPE_CROSS, sig(24.0, 24.0, 12.0, 12.0)),
        ];
        assert_eq!(classify_shape(&sig(32.0, 32.0, 13.0, 8.0), &known), SHAPE_HAND);
        assert_eq!(classify_shape(&sig(9.0, 18.0, 4.0, 9.0), &known), SHAPE_IBEAM);
        assert_eq!(classify_shape(&sig(24.0, 24.0, 12.0, 12.0), &known), SHAPE_CROSS);
        // A media unidad todavía es el mismo cursor.
        assert_eq!(classify_shape(&sig(31.6, 32.4, 13.3, 7.7), &known), SHAPE_HAND);
        // Uno que no está en la lista (por ejemplo, un cursor de cambiar tamaño) es flecha.
        assert_eq!(classify_shape(&sig(24.0, 24.0, 5.0, 5.0), &known), SHAPE_ARROW);
        assert_eq!(classify_shape(&sig(1.0, 1.0, 0.0, 0.0), &[]), SHAPE_ARROW);
    }

    #[test]
    fn los_clics_son_los_flancos_de_bajada() {
        let track = Track {
            version: 1,
            region: [0, 0, 100, 100],
            samples: vec![
                Sample(0, 0.1, 0.1, false),
                Sample(16, 0.1, 0.1, true),
                Sample(32, 0.2, 0.1, true),
                Sample(48, 0.2, 0.1, false),
                Sample(500, 0.5, 0.5, true),
            ],
            cursor_hidden: false,
            keys: Vec::new(),
            carets: Vec::new(),
            shapes: Vec::new(),
        };
        assert_eq!(track.clicks(), vec![16, 500]);
    }

    #[test]
    fn los_archivos_de_apoyo_van_en_atic_junto_al_video() {
        let video = Path::new("/x/Atic/Grabación 1.mp4");
        assert_eq!(baking_path(video).unwrap(), Path::new("/x/Atic/.atic/Grabación 1.baking.mp4"));
        assert_eq!(audio_path(video).unwrap(), Path::new("/x/Atic/.atic/Grabación 1.audio.wav"));
        assert_eq!(master_path(video).unwrap(), Path::new("/x/Atic/.atic/Grabación 1.master.mp4"));
    }

    #[test]
    fn el_registro_va_en_atic_junto_al_video() {
        let path = track_path(Path::new("/x/Atic/Grabación 1.mp4")).unwrap();
        assert_eq!(path, Path::new("/x/Atic/.atic/Grabación 1.json"));
    }
}
