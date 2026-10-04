//! Lo que GPUI no expone para un overlay en Windows: always-on-top, no robar
//! el foco, dejar pasar los clics fuera de la pill y leer el cursor global.

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, SetForegroundWindow,
    SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos, WDA_EXCLUDEFROMCAPTURE, WDA_NONE, SystemParametersInfoW, GWL_EXSTYLE, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

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

    /// El monitor principal entero, en coordenadas lógicas de la ventana.
    pub fn monitor_area(&self, scale_factor: f32) -> Option<crate::geometry::Rect> {
        let origin = self.client_origin()?;
        let (width, height) =
            unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        Some(crate::geometry::Rect::new(
            -origin.x as f32 / scale_factor,
            -origin.y as f32 / scale_factor,
            width as f32 / scale_factor,
            height as f32 / scale_factor,
        ))
    }

    /// Área de trabajo del monitor principal (sin la barra de tareas), en
    /// coordenadas lógicas de la ventana. GPUI no la expone.
    pub fn work_area(&self, scale_factor: f32) -> Option<crate::geometry::Rect> {
        unsafe {
            let mut work = RECT::default();
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut work as *mut RECT as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .ok()?;
            let origin = self.client_origin()?;
            Some(crate::geometry::Rect::new(
                (work.left - origin.x) as f32 / scale_factor,
                (work.top - origin.y) as f32 / scale_factor,
                (work.right - work.left) as f32 / scale_factor,
                (work.bottom - work.top) as f32 / scale_factor,
            ))
        }
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

/// Alt+Z, el atajo de la rueda en Atic. Se sondea porque la ventana nunca
/// tiene el foco; un prototipo no justifica registrar un hotkey global.
pub fn wheel_shortcut_down() -> bool {
    unsafe { GetAsyncKeyState(VK_MENU.0 as i32) < 0 && GetAsyncKeyState('Z' as i32) < 0 }
}
