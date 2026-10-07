//! El ícono de Atic en la bandeja de Windows, con las entradas de `tray.rs` de
//! la app de Tauri. Va en un hilo propio con una ventana oculta que recibe los
//! clics; las órdenes llegan a la pill por un canal, como los atajos.
//!
//! Si el Explorador se reinicia, la bandeja se recrea vacía y avisa con
//! `TaskbarCreated`: ahí se vuelve a agregar el ícono.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, PostMessageW, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON,
    LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

/// Lo que se pidió desde el ícono.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// La ventana principal de Atic (biblioteca, Ajustes).
    OpenAtic,
    /// El espacio de consolas.
    Consoles,
    Capture,
    Summon,
    Quit,
}

const CALLBACK: u32 = WM_APP + 1;
const ICON_ID: u32 = 1;

/// Las entradas del menú, en orden: id, clave de texto y orden.
const ITEMS: &[(usize, &str, Command)] = &[
    (1, "tray.show", Command::OpenAtic),
    (2, "tray.consoles", Command::Consoles),
    (3, "tray.capture", Command::Capture),
    (4, "tray.summonPill", Command::Summon),
];
const QUIT_ID: usize = 9;

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

fn run() {
    let class = wide("AticPillTray");
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

/// El menú del clic derecho, con los textos en el idioma actual.
fn show_menu(hwnd: HWND) {
    // SAFETY: menú propio, destruido al salir; la ventana es de este hilo.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        for (id, key, _) in ITEMS {
            AppendMenuW(menu, MF_STRING, *id, wide(crate::i18n::t(key)).as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, QUIT_ID, wide(crate::i18n::t("tray.quit")).as_ptr());

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

        if chosen == QUIT_ID {
            send(Command::Quit);
        } else if let Some((_, _, command)) = ITEMS.iter().find(|(id, _, _)| *id == chosen) {
            send(*command);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == CALLBACK {
        match lparam as u32 {
            WM_LBUTTONUP => send(Command::OpenAtic),
            WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
            _ => {}
        }
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
    fn elige_la_entrada_mas_cercana_del_ico_de_atic() {
        let ico = include_bytes!("../../../../../apps/desktop/src-tauri/icons/icon.ico");
        let (offset, len) = pick_entry(ico, 32).expect("el .ico tiene entradas");
        assert!(offset + len <= ico.len());
        assert!(pick_entry(b"no es un ico", 32).is_none());
    }
}
