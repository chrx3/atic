//! Vidrio esmerilado detrás de la pill: el escritorio desenfocado.
//!
//! GPUI solo sabe desenfocar detrás de una ventana entera, y el overlay cubre
//! el monitor: desenfocaría la pantalla completa. Aquí hay una ventana nativa
//! aparte, sin contenido propio (`WS_EX_NOREDIRECTIONBITMAP`), justo debajo
//! del overlay, que se mueve cuando la pill cambia de forma.
//!
//! El efecto de ventana de Windows (acrylic o blur por
//! `SetWindowCompositionAttribute`) no se puede recortar: ignora la región de
//! la ventana y queda un cuadrado detrás de la gota. Por eso se usa
//! Windows.UI.Composition, como el acrylic de WinUI: un visual pintado con el
//! fondo ya desenfocado que da DWM (`HostBackdropBrush`) y un recorte
//! geométrico con antialias (rectángulo redondeado o círculo).
//!
//! Necesita los «Efectos de transparencia» de Windows; sin ellos DWM no da
//! fondo y la pill queda con su tinte. `PILL_GLASS=off` lo apaga.

use windows::core::{w, Interface};
use windows_numerics::Vector2;
use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionEllipseGeometry, CompositionGeometricClip, CompositionRoundedRectangleGeometry,
    Compositor, SpriteVisual,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, SetWindowPos, ShowWindow, HTTRANSPARENT,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_NCHITTEST, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::geometry::Rect;

/// La forma del vidrio, en coordenadas lógicas del overlay.
#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    /// Esquinas en orden: arriba-izquierda, arriba-derecha, abajo-derecha,
    /// abajo-izquierda (como `liquid::rounded_rect_corners`).
    Rounded { rect: Rect, radii: [f32; 4] },
    Circle { center: (f32, f32), r: f32 },
}

pub fn enabled() -> bool {
    std::env::var("PILL_GLASS").as_deref() != Ok("off")
}

/// Lo que se le pidió a Windows la última vez, en píxeles físicos: si no
/// cambió, no se toca nada.
#[derive(Clone, Copy, PartialEq)]
struct Placed {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radii: [i32; 4],
    circle: bool,
}

pub struct Glass {
    hwnd: HWND,
    placed: Option<Placed>,
    visual: SpriteVisual,
    rounded: CompositionRoundedRectangleGeometry,
    ellipse: CompositionEllipseGeometry,
    rounded_clip: CompositionGeometricClip,
    ellipse_clip: CompositionGeometricClip,
    _target: DesktopWindowTarget,
    _queue: Option<DispatcherQueueController>,
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Los clics los atiende el overlay, que está encima.
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// `DWMWA_USE_HOSTBACKDROPBRUSH`: sin esto el pincel del fondo sale vacío.
const USE_HOSTBACKDROPBRUSH: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(17);

impl Glass {
    pub fn new() -> Option<Self> {
        match unsafe { Self::create() } {
            Ok(glass) => Some(glass),
            Err(error) => {
                eprintln!("vidrio: no disponible ({error})");
                None
            }
        }
    }

    unsafe fn create() -> windows::core::Result<Self> {
        // Composition necesita una cola de despacho en este hilo (el de GPUI).
        let queue = match DispatcherQueue::GetForCurrentThread() {
            Ok(_) => None,
            Err(_) => Some(CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?),
        };

        let instance = GetModuleHandleW(None)?;
        let class = w!("AticGpuiGlass");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let on: i32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            USE_HOSTBACKDROPBRUSH,
            &on as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        )?;

        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = interop.CreateDesktopWindowTarget(hwnd, true)?;
        let visual = compositor.CreateSpriteVisual()?;
        visual.SetBrush(&compositor.CreateHostBackdropBrush()?)?;
        let rounded = compositor.CreateRoundedRectangleGeometry()?;
        let ellipse = compositor.CreateEllipseGeometry()?;
        let rounded_clip = compositor.CreateGeometricClipWithGeometry(&rounded)?;
        let ellipse_clip = compositor.CreateGeometricClipWithGeometry(&ellipse)?;
        visual.SetClip(&rounded_clip)?;
        target.SetRoot(&visual)?;
        Ok(Self {
            hwnd,
            placed: None,
            visual,
            rounded,
            ellipse,
            rounded_clip,
            ellipse_clip,
            _target: target,
            _queue: queue,
        })
    }

    pub fn hide(&mut self) {
        if self.placed.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    /// Pone el vidrio con la forma `shape`, justo debajo de `above` (el
    /// overlay). `origin` es la esquina del área cliente del overlay en
    /// pantalla y `scale` su escala.
    pub fn place(&mut self, shape: Shape, above: HWND, origin: (i32, i32), scale: f32) {
        let (rect, radii, circle) = match shape {
            Shape::Rounded { rect, radii } => (rect, radii, false),
            Shape::Circle { center, r } => (Rect::centered(center, r * 2.0, r * 2.0), [r; 4], true),
        };
        let px = |v: f32| (v * scale).round() as i32;
        let placed = Placed {
            x: origin.0 + px(rect.x),
            y: origin.1 + px(rect.y),
            w: px(rect.w).max(1),
            h: px(rect.h).max(1),
            radii: radii.map(px),
            circle,
        };
        if self.placed == Some(placed) {
            return;
        }
        let reshaped = self.placed.is_none_or(|old| {
            old.w != placed.w || old.h != placed.h || old.radii != placed.radii || old.circle != circle
        });
        let switched = self.placed.is_none_or(|old| old.circle != circle);
        self.placed = Some(placed);
        if reshaped {
            if let Err(error) = self.reshape(&placed, switched) {
                eprintln!("vidrio: {error}");
            }
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(above),
                placed.x,
                placed.y,
                placed.w,
                placed.h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    /// El recorte con antialias. El rectángulo redondeado de Composition
    /// tiene un solo radio: para el tab, con las esquinas del borde rectas,
    /// el rectángulo se alarga hacia ese lado y la ventana corta lo que sobra.
    fn reshape(&self, placed: &Placed, switched: bool) -> windows::core::Result<()> {
        let (w, h) = (placed.w as f32, placed.h as f32);
        self.visual.SetSize(Vector2 { X: w, Y: h })?;
        if placed.circle {
            if switched {
                self.visual.SetClip(&self.ellipse_clip)?;
            }
            self.ellipse.SetCenter(Vector2 { X: w / 2.0, Y: h / 2.0 })?;
            self.ellipse.SetRadius(Vector2 { X: w / 2.0, Y: h / 2.0 })?;
            return Ok(());
        }
        if switched {
            self.visual.SetClip(&self.rounded_clip)?;
        }
        let (offset, size, r) = extended(w, h, placed.radii);
        self.rounded.SetOffset(offset)?;
        self.rounded.SetSize(size)?;
        self.rounded.SetCornerRadius(Vector2 { X: r, Y: r })?;
        Ok(())
    }
}

/// El rectángulo redondeado que, cortado por la ventana de `w × h`, deja
/// rectas las esquinas de radio 0: se alarga `r` hacia ese lado.
fn extended(w: f32, h: f32, radii: [i32; 4]) -> (Vector2, Vector2, f32) {
    let [tl, tr, br, bl] = radii.map(|r| r == 0);
    let r = radii.iter().copied().max().unwrap_or(0) as f32;
    let r = r.min(w / 2.0).min(h / 2.0);
    let (mut x, mut y, mut sw, mut sh) = (0.0, 0.0, w, h);
    if tl && tr {
        y -= r;
        sh += r;
    }
    if bl && br {
        sh += r;
    }
    if tl && bl {
        x -= r;
        sw += r;
    }
    if tr && br {
        sw += r;
    }
    (Vector2 { X: x, Y: y }, Vector2 { X: sw, Y: sh }, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_tab_de_arriba_se_alarga_hacia_el_borde() {
        // Esquinas de arriba rectas: el rectángulo sube y la ventana lo corta.
        let (offset, size, r) = extended(124.0, 40.0, [0, 0, 20, 20]);
        assert_eq!((offset.X, offset.Y), (0.0, -20.0));
        assert_eq!((size.X, size.Y), (124.0, 60.0));
        assert_eq!(r, 20.0);
        // Tab a la izquierda: esquinas izquierdas rectas.
        let (offset, size, _) = extended(40.0, 124.0, [0, 20, 20, 0]);
        assert_eq!((offset.X, size.X), (-20.0, 60.0));
    }
}
