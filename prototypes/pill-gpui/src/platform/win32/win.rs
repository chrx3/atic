//! Lo que GPUI no expone para un overlay en Windows: always-on-top, no robar
//! el foco, dejar pasar los clics fuera de la pill y leer el cursor global.

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::core::BOOL;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use atic_core::MutexExt;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow,
    HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GetClientRect, GetCursorPos, GetWindow, GetWindowLongPtrW, GetWindowRect, SetForegroundWindow,
    SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, WDA_EXCLUDEFROMCAPTURE, WDA_NONE, GWL_EXSTYLE,
    GWLP_WNDPROC, GW_HWNDPREV, HWND_TOPMOST, WM_DPICHANGED, WNDPROC,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT,
};

use crate::geometry::{Edge, Rect};

/// Un monitor, en píxeles físicos de pantalla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub monitor: RECT,
    /// Sin la barra de tareas.
    pub work: RECT,
}

impl Screen {
    fn of(monitor: HMONITOR) -> Option<Self> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetMonitorInfoW(monitor, &mut info) }
            .as_bool()
            .then_some(Self {
                monitor: info.rcMonitor,
                work: info.rcWork,
            })
    }

    /// El monitor cuya esquina de arriba a la izquierda es `(x, y)`.
    pub fn at_origin(x: i32, y: i32) -> Option<Self> {
        let screen = Self::of(unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) })?;
        (screen.monitor.left == x && screen.monitor.top == y).then_some(screen)
    }

    /// Bordes que dan al vacío, sin otro monitor pegado.
    pub fn outer_edges(&self) -> Vec<Edge> {
        let all: Vec<Rect> = all_monitors().iter().map(physical).collect();
        crate::geometry::outer_edges(&physical(&self.monitor), &all)
    }
}

fn physical(rect: &RECT) -> Rect {
    Rect::new(
        rect.left as f32,
        rect.top as f32,
        (rect.right - rect.left) as f32,
        (rect.bottom - rect.top) as f32,
    )
}

/// Todos los monitores, en píxeles físicos.
fn all_monitors() -> Vec<RECT> {
    unsafe extern "system" fn push(_: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<RECT>) };
        list.push(unsafe { *rect });
        true.into()
    }
    let mut list: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

pub struct Overlay {
    hwnd: HWND,
    passthrough: bool,
}

impl Overlay {
    pub fn attach(window: &Window) -> Option<Self> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return None;
        };
        let hwnd = HWND(win32.hwnd.get() as *mut _);
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let extra = (WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED).0 as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | extra);
            // Windows 11 le pone borde, esquinas redondeadas y sombra a toda la
            // ventana; aquí solo debe verse la pill.
            set_dwm(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0);
            set_dwm(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
            set_dwm(hwnd, DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED.0);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        keep_size_on_dpi_change(hwnd);
        let mut overlay = Self {
            hwnd,
            passthrough: false,
        };
        overlay.set_passthrough(true);
        Some(overlay)
    }

    /// Con `true` los clics atraviesan la ventana hacia lo que haya debajo.
    pub fn set_passthrough(&mut self, passthrough: bool) {
        if self.passthrough == passthrough {
            return;
        }
        self.passthrough = passthrough;
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_TRANSPARENT.0 as isize;
            let style = if passthrough {
                style | flag
            } else {
                style & !flag
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style);
        }
    }

    /// La ventana del overlay nunca toma el foco, salvo mientras un panel con
    /// campo de texto está abierto (como `set_overlay_text_mode` en Atic).
    pub fn set_focusable(&mut self, focusable: bool) {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            let style = if focusable {
                style & !flag
            } else {
                style | flag
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style);
            if focusable {
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
    }

    /// Invisible para las capturas de pantalla mientras se congela, para que
    /// el notch no salga en la foto. Solo ese rato: si no, tampoco saldría en
    /// las grabaciones ni al compartir pantalla.
    pub fn exclude_from_capture(&self, exclude: bool) {
        let affinity = if exclude {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        unsafe {
            let _ = SetWindowDisplayAffinity(self.hwnd, affinity);
        }
    }

    /// Lo que deja la ventana «siempre visible» es su lugar en el orden de
    /// apilamiento, no el estilo `WS_EX_TOPMOST`. Windows a veces la saca de
    /// esa capa sin quitarle el estilo (al cerrar Fotos abierto desde el
    /// estante) y entonces cualquier ventana maximizada la tapa entera. Si
    /// quedó alguna ventana normal encima, la devuelve arriba. Devuelve si
    /// tuvo que hacerlo.
    pub fn keep_topmost(&self) -> bool {
        let topmost = WS_EX_TOPMOST.0 as isize;
        let mut demoted = false;
        let mut above = unsafe { GetWindow(self.hwnd, GW_HWNDPREV) };
        // Tope por si el orden cambia mientras se recorre.
        for _ in 0..2000 {
            let Ok(hwnd) = above else {
                break;
            };
            if unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & topmost == 0 {
                demoted = true;
                break;
            }
            above = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
        }
        if demoted {
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
        demoted
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Esquina del área cliente en pantalla, en píxeles físicos.
    pub fn origin(&self) -> Option<(i32, i32)> {
        self.client_origin().map(|p| (p.x, p.y))
    }

    /// Esquina del área cliente en pantalla: es el origen de las coordenadas
    /// de GPUI. `GetWindowRect` no sirve: incluye bordes invisibles de DWM y
    /// corre todo unos píxeles.
    fn client_origin(&self) -> Option<POINT> {
        let mut origin = POINT::default();
        unsafe { ClientToScreen(self.hwnd, &mut origin) }
            .as_bool()
            .then_some(origin)
    }

    /// El monitor en que está la ventana.
    pub fn screen(&self) -> Option<Screen> {
        Screen::of(unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) })
    }

    /// El monitor bajo el cursor.
    pub fn screen_under_cursor(&self) -> Option<Screen> {
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }.ok()?;
        Screen::of(unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONULL) })
    }

    /// Pasa la ventana a cubrir `screen` entero.
    ///
    /// Desde otro hilo: si el monitor tiene otra escala, Windows le manda a la
    /// ventana `WM_DPICHANGED` y GPUI la redimensiona y avisa a la app, que
    /// ahora mismo está ocupada en este cuadro. Así lo atiende su propio hilo
    /// después. Ese cambio de escala llega más tarde y GPUI le pone a la
    /// ventana el rectángulo que sugiere Windows, escalado desde el monitor
    /// anterior: por eso se reintenta hasta que el área cliente calce.
    pub fn move_to(&self, screen: &Screen) {
        self.cover(screen.monitor);
    }

    /// Como `move_to`, pero a cualquier rectángulo de pantalla (físico): la
    /// mira lo usa para cubrir todos los monitores.
    pub fn cover(&self, target: RECT) {
        *COVER_TARGET.lock_or_recover() = Some(target);
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let hwnd = HWND(hwnd as *mut _);
            for _ in 0..10 {
                if client_rect(hwnd) == Some(target) {
                    break;
                }
                place_client(hwnd, &target);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    }

    /// El área cliente ya es `target`.
    pub fn covers(&self, target: &RECT) -> bool {
        client_rect(self.hwnd) == Some(*target)
    }

    /// Un rectángulo de pantalla en coordenadas lógicas de la ventana.
    pub fn to_logical(&self, rect: &RECT, scale_factor: f32) -> Option<Rect> {
        let origin = self.client_origin()?;
        Some(Rect::new(
            (rect.left - origin.x) as f32 / scale_factor,
            (rect.top - origin.y) as f32 / scale_factor,
            (rect.right - rect.left) as f32 / scale_factor,
            (rect.bottom - rect.top) as f32 / scale_factor,
        ))
    }

    /// Cursor global en coordenadas lógicas de la ventana.
    pub fn cursor(&self, scale_factor: f32) -> Option<(f32, f32)> {
        unsafe {
            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor).ok()?;
            let origin = self.client_origin()?;
            Some((
                (cursor.x - origin.x) as f32 / scale_factor,
                (cursor.y - origin.y) as f32 / scale_factor,
            ))
        }
    }
}

/// El área cliente en pantalla, en píxeles físicos.
fn client_rect(hwnd: HWND) -> Option<RECT> {
    let mut origin = POINT::default();
    let mut size = RECT::default();
    unsafe {
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
        GetClientRect(hwnd, &mut size).ok()?;
    }
    Some(RECT {
        left: origin.x,
        top: origin.y,
        right: origin.x + size.right,
        bottom: origin.y + size.bottom,
    })
}

/// El área cliente que la ventana tiene que ocupar (lo último de `cover`).
static COVER_TARGET: Mutex<Option<RECT>> = Mutex::new(None);
/// El procedimiento de ventana de GPUI, al que se le pasa todo lo demás.
static GPUI_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// Al estirarse sobre pantallas de otra escala, Windows manda
/// `WM_DPICHANGED` con un tamaño sugerido (el actual multiplicado por el
/// cambio de escala) y GPUI lo aplica. Con resoluciones muy distintas ese
/// tamaño ya no cubre la pantalla grande y la mira queda corta. Aquí el
/// sugerido se reemplaza por el destino de `cover`: GPUI igual toma la escala
/// nueva, pero la ventana queda donde tiene que estar.
fn keep_size_on_dpi_change(hwnd: HWND) {
    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_DPICHANGED && lparam.0 != 0 {
            if let Some(target) = *COVER_TARGET.lock_or_recover() {
                if let Some(outer) = outer_for_client(hwnd, &target) {
                    // SAFETY: con WM_DPICHANGED, `lparam` apunta al RECT sugerido.
                    unsafe { *(lparam.0 as *mut RECT) = outer };
                }
            }
        }
        let original = GPUI_WNDPROC.load(Ordering::Relaxed);
        // SAFETY: `original` es el WNDPROC que tenía la ventana.
        unsafe { CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(original), hwnd, msg, wparam, lparam) }
    }
    if GPUI_WNDPROC.load(Ordering::Relaxed) != 0 {
        return;
    }
    let ours: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = proc;
    // SAFETY: se guarda el original antes de que llegue el primer mensaje al
    // nuestro (todo corre en el hilo de la ventana).
    let original = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as isize) };
    GPUI_WNDPROC.store(original, Ordering::Relaxed);
}

/// El rectángulo de ventana cuya área cliente es `target`.
fn outer_for_client(hwnd: HWND, target: &RECT) -> Option<RECT> {
    let mut outer = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut outer) }.ok()?;
    let client = client_rect(hwnd)?;
    Some(RECT {
        left: target.left - (client.left - outer.left),
        top: target.top - (client.top - outer.top),
        right: target.right + (outer.right - client.right),
        bottom: target.bottom + (outer.bottom - client.bottom),
    })
}

/// Mueve la ventana para que su área cliente (lo que dibuja GPUI) sea
/// `target`. La ventana tiene bordes invisibles alrededor; si se ubicara el
/// rectángulo de la ventana, todo quedaría corrido unos píxeles.
fn place_client(hwnd: HWND, target: &RECT) {
    let mut outer = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut outer) }.is_err() {
        return;
    }
    let Some(client) = client_rect(hwnd) else {
        return;
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            target.left - (client.left - outer.left),
            target.top - (client.top - outer.top),
            (target.right - target.left) + (outer.right - outer.left) - (client.right - client.left),
            (target.bottom - target.top) + (outer.bottom - outer.top) - (client.bottom - client.top),
            SWP_NOACTIVATE,
        );
    }
}

unsafe fn set_dwm<T: Copy>(
    hwnd: HWND,
    attribute: windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE,
    value: T,
) {
    let _ = DwmSetWindowAttribute(
        hwnd,
        attribute,
        &value as *const T as *const _,
        std::mem::size_of::<T>() as u32,
    );
}

/// Botón izquierdo apretado ahora mismo, aunque el evento lo reciba otra
/// ventana.
pub fn left_button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 }
}

/// Alt+Z: la rueda mientras Atic tiene los atajos (el de `config.json` es de
/// ella) o si no hay uno válido. Se sondea porque la ventana nunca tiene el
/// foco y `RegisterHotKey` no avisa al soltar.
pub fn wheel_shortcut_down() -> bool {
    unsafe { GetAsyncKeyState(VK_MENU.0 as i32) < 0 && GetAsyncKeyState('Z' as i32) < 0 }
}
