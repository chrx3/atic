//! El ícono de Atic en la bandeja de Windows, con las entradas de `tray.rs` de
//! la app de Tauri. Va en un hilo propio con una ventana oculta que recibe los
//! clics; las órdenes llegan a la pill por un canal, como los atajos.
//!
//! Si el Explorador se reinicia, la bandeja se recrea vacía y avisa con
//! `TaskbarCreated`: ahí se vuelve a agregar el ícono.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON,
    LR_DEFAULTCOLOR, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SetMenuDefaultItem, TPM_BOTTOMALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

/// Lo que se pidió desde el ícono.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// La ventana principal: Reuniones.
    OpenAtic,
    /// Los Ajustes de la pill.
    Settings,
    /// El espacio de consolas.
    Consoles,
    Capture,
    Quit,
}

const CALLBACK: u32 = WM_APP + 1;
/// Otra pill que no arrancó (instancia única) pide abrir Atic, como cuando se
/// vuelve a abrir la app de Tauri con ella corriendo.
const WAKE: u32 = WM_APP + 2;
/// Hay un aviso en la cola ([`notify`]).
const NOTIFY: u32 = WM_APP + 3;
/// El clic en el globo de un aviso (`NIN_BALLOONUSERCLICK`).
const BALLOON_CLICK: u32 = 0x0405;
const ICON_ID: u32 = 1;
const CLASS: &str = "AticPillTray";

/// Las entradas del menú: id, clave de texto y orden. Ajustes va primero y en
/// negrita: es lo que abre el clic izquierdo.
const SETTINGS_ID: usize = 1;
const CAPTURE_ID: usize = 2;
const MEETINGS_ID: usize = 3;
const CONSOLES_ID: usize = 4;
const QUIT_ID: usize = 9;
const ITEMS: &[(usize, Command)] = &[
    (SETTINGS_ID, Command::Settings),
    (CAPTURE_ID, Command::Capture),
    (MEETINGS_ID, Command::OpenAtic),
    (CONSOLES_ID, Command::Consoles),
    (QUIT_ID, Command::Quit),
];

static SENDER: OnceLock<Sender<Command>> = OnceLock::new();
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
static ICON: OnceLock<usize> = OnceLock::new();

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Arranca el ícono. Si no se puede (sin Explorador, por ejemplo), el canal
/// simplemente no recibe nada.
pub fn spawn() -> Receiver<Command> {
    let (tx, rx) = mpsc::channel();
    if SENDER.set(tx).is_ok() {
        let started = std::thread::Builder::new().name("bandeja".into()).spawn(run);
        if let Err(error) = started {
            tracing::warn!(%error, "bandeja: no arrancó el hilo");
        }
    }
    rx
}

/// Desde una segunda pill: le pide a la que corre que abra Atic. `false` si no
/// encontró su ícono.
pub fn wake_running() -> bool {
    let class = wide(CLASS);
    // SAFETY: cadena terminada en 0; mensaje sin punteros.
    unsafe {
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        !hwnd.is_null() && PostMessageW(hwnd, WAKE, 0, 0) != 0
    }
}

/// Un aviso del sistema que espera a que el hilo de la bandeja lo muestre.
struct Alert {
    title: String,
    body: String,
    error: bool,
}

static ALERTS: Mutex<VecDeque<Alert>> = Mutex::new(VecDeque::new());

/// Un aviso de Windows (el globo del ícono de la bandeja, que Windows 10 y 11
/// muestran como notificación) con `title` y `body`. Un clic en él abre Atic
/// Code. `false` si el ícono no está (sin la pill, `CODE_ALONE=1`): no hay
/// dónde mostrarlo.
pub fn notify(title: &str, body: &str, error: bool) -> bool {
    let class = wide(CLASS);
    // SAFETY: cadena terminada en 0; mensaje sin punteros.
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return false;
    }
    if let Ok(mut queue) = ALERTS.lock() {
        // No se acumulan avisos que nadie vio.
        while queue.len() >= 8 {
            queue.pop_front();
        }
        queue.push_back(Alert { title: title.to_string(), body: body.to_string(), error });
    }
    // SAFETY: mensaje sin punteros a una ventana de este proceso.
    unsafe { PostMessageW(hwnd, NOTIFY, 0, 0) != 0 }
}

/// Copia `text` en un campo UTF-16 de largo fijo, con el 0 final.
fn fill(field: &mut [u16], text: &str) {
    let max = field.len() - 1;
    let mut units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > max {
        units.truncate(max);
        // No dejar la mitad de un par subrogado.
        if units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
            units.pop();
        }
    }
    field[..units.len()].copy_from_slice(&units);
    field[units.len()] = 0;
}

fn show_alerts(hwnd: HWND) {
    loop {
        let Some(alert) = ALERTS.lock().ok().and_then(|mut queue| queue.pop_front()) else {
            return;
        };
        let mut data = notify_data(hwnd);
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = if alert.error { NIIF_ERROR } else { NIIF_INFO };
        fill(&mut data.szInfoTitle, &alert.title);
        fill(&mut data.szInfo, &alert.body);
        // SAFETY: `data` completa y válida durante la llamada.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }
}

fn run() {
    let class = wide(CLASS);
    // SAFETY: llamadas Win32 en un hilo dedicado, con cadenas terminadas en 0
    // que viven durante la llamada; la ventana vive lo que el hilo.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            tracing::warn!("bandeja: no se registró la clase de ventana");
            return;
        }
        // Ventana de nivel superior pero nunca visible: una «message-only» no
        // recibe el `TaskbarCreated` que se manda a todas.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            tracing::warn!("bandeja: no se creó la ventana");
            return;
        }
        let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        follow_dark_mode();
        add_icon(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        remove_icon(hwnd);
    }
}

fn icon() -> HICON {
    *ICON.get_or_init(|| load_icon(include_bytes!("../../../../../apps/desktop/src-tauri/icons/icon.ico"), 32)
        .map_or(0, |icon| icon as usize)) as HICON
}

/// Crea un `HICON` con la imagen de `ico` más cercana a `size` píxeles. Las
/// entradas de un `.ico` pueden ser PNG o DIB; `CreateIconFromResourceEx`
/// entiende las dos.
fn load_icon(ico: &[u8], size: u32) -> Option<HICON> {
    let (offset, len) = pick_entry(ico, size)?;
    let data = ico.get(offset..offset + len)?;
    // SAFETY: `data` es un trozo válido del archivo durante la llamada.
    let icon = unsafe {
        CreateIconFromResourceEx(
            data.as_ptr(),
            data.len() as u32,
            1,
            0x0003_0000,
            size as i32,
            size as i32,
            LR_DEFAULTCOLOR,
        )
    };
    (!icon.is_null()).then_some(icon)
}

/// El desplazamiento y largo de la entrada del `.ico` de lado más parecido a
/// `size` (0 en el archivo significa 256).
fn pick_entry(ico: &[u8], size: u32) -> Option<(usize, usize)> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?));
    if u16_at(2)? != 1 {
        return None;
    }
    let count = u16_at(4)? as usize;
    (0..count)
        .filter_map(|index| {
            let entry = 6 + index * 16;
            let side = match *ico.get(entry)? {
                0 => 256,
                side => u32::from(side),
            };
            let len = u32_at(entry + 8)? as usize;
            let offset = u32_at(entry + 12)? as usize;
            Some((side.abs_diff(size), offset, len))
        })
        .min_by_key(|(distance, _, _)| *distance)
        .map(|(_, offset, len)| (offset, len))
}

fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: estructura de datos planos; cero es un valor válido.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = ICON_ID;
    data
}

fn add_icon(hwnd: HWND) {
    let mut data = notify_data(hwnd);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = CALLBACK;
    data.hIcon = icon();
    let tip = wide("Atic");
    data.szTip[..tip.len()].copy_from_slice(&tip);
    // SAFETY: `data` completa y válida durante la llamada.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
        tracing::warn!("bandeja: Windows no aceptó el ícono");
    }
}

fn remove_icon(hwnd: HWND) {
    let data = notify_data(hwnd);
    // SAFETY: `data` identifica nuestro ícono.
    unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
}

fn send(command: Command) {
    if let Some(sender) = SENDER.get() {
        let _ = sender.send(command);
    }
}

/// Que el menú siga el tema de Windows (oscuro si Windows está en oscuro).
/// Es lo que hacen el Explorador y Chrome: `SetPreferredAppMode` y
/// `FlushMenuThemes` de `uxtheme.dll`, que Windows exporta solo por número
/// (135 y 136). Si no están, el menú queda claro como siempre.
fn follow_dark_mode() {
    const ALLOW_DARK: i32 = 1;
    // SAFETY: las dos funciones existen desde Windows 10 1903 con esas firmas;
    // si `GetProcAddress` no las encuentra, no se llaman.
    unsafe {
        let uxtheme = LoadLibraryW(wide("uxtheme.dll").as_ptr());
        if uxtheme.is_null() {
            return;
        }
        if let Some(set_mode) = GetProcAddress(uxtheme, 135 as *const u8) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            set_mode(ALLOW_DARK);
        }
        if let Some(flush) = GetProcAddress(uxtheme, 136 as *const u8) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

/// El atajo de la captura, como se lee: «Ctrl+Shift+4».
fn capture_shortcut() -> Option<String> {
    let cfg = atic_core::AppDirs::new().ok().map(|dirs| atic_core::Config::load(&dirs.config_path()))?;
    let text = cfg.screenshot_shortcut.trim();
    (!text.is_empty()).then(|| text.replace("CommandOrControl", "Ctrl").replace("CmdOrCtrl", "Ctrl"))
}

/// El menú del clic derecho, con los textos en el idioma actual.
fn show_menu(hwnd: HWND) {
    let label = |id: usize| -> String {
        match id {
            SETTINGS_ID => crate::i18n::t("pill.tray.settings").to_string(),
            CAPTURE_ID => match capture_shortcut() {
                // El tab alinea el atajo a la derecha, como en cualquier menú.
                Some(keys) => format!("{}\t{keys}", crate::i18n::t("tray.capture")),
                None => crate::i18n::t("tray.capture").to_string(),
            },
            MEETINGS_ID => crate::i18n::t("pill.tray.meetings").to_string(),
            CONSOLES_ID => crate::i18n::t("tray.consoles").to_string(),
            _ => crate::i18n::t("pill.tray.quit").to_string(),
        }
    };
    // SAFETY: menú propio, destruido al salir; la ventana es de este hilo.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let version = format!("Atic {}", env!("ATIC_VERSION"));
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(&version).as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, SETTINGS_ID, wide(&label(SETTINGS_ID)).as_ptr());
        SetMenuDefaultItem(menu, SETTINGS_ID as u32, 0);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        for id in [CAPTURE_ID, MEETINGS_ID, CONSOLES_ID] {
            AppendMenuW(menu, MF_STRING, id, wide(&label(id)).as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, QUIT_ID, wide(&label(QUIT_ID)).as_ptr());

        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Sin esto el menú no se cierra al hacer clic afuera.
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as usize;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        if let Some((_, command)) = ITEMS.iter().find(|(id, _)| *id == chosen) {
            send(*command);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == CALLBACK {
        match lparam as u32 {
            WM_LBUTTONUP => send(Command::Settings),
            BALLOON_CLICK => send(Command::Consoles),
            WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
            _ => {}
        }
        return 0;
    }
    if msg == WAKE {
        send(Command::Settings);
        return 0;
    }
    if msg == NOTIFY {
        show_alerts(hwnd);
        return 0;
    }
    if TASKBAR_CREATED.get() == Some(&msg) {
        add_icon(hwnd);
        return 0;
    }
    // SAFETY: lo demás va al procedimiento por omisión.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_texto_del_globo_cabe_y_termina_en_cero() {
        let mut field = [7u16; 8];
        fill(&mut field, "hola");
        assert_eq!(&field[..5], &[104, 111, 108, 97, 0]);
        // Largo de más: se corta y deja el 0 final.
        fill(&mut field, "abcdefghijk");
        assert_eq!(field[7], 0);
        assert_eq!(String::from_utf16_lossy(&field[..7]), "abcdefg");
        // Un emoji (dos unidades) que no cabe entero se quita entero.
        fill(&mut field, "abcdef\u{1F600}");
        assert_eq!(String::from_utf16_lossy(&field[..6]), "abcdef");
        assert_eq!(field[6], 0);
    }

    #[test]
    fn elige_la_entrada_mas_cercana_del_ico_de_atic() {
        let ico = include_bytes!("../../../../../apps/desktop/src-tauri/icons/icon.ico");
        let (offset, len) = pick_entry(ico, 32).expect("el .ico tiene entradas");
        assert!(offset + len <= ico.len());
        assert!(pick_entry(b"no es un ico", 32).is_none());
    }
}
