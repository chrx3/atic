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
//! geométrico con antialias (rectángulo redondeado o círculo). El tab de un
//! costado con su bloque al lado es una forma de dos piezas: dos visuales con
//! el mismo fondo, cada uno con su recorte, dentro de la misma ventana.
//!
//! Necesita los «Efectos de transparencia» de Windows; sin ellos DWM no da
//! fondo y la pill queda con su tinte. `PILL_GLASS=off` lo apaga.

use windows::core::{w, Interface};
use windows_numerics::{Vector2, Vector3};
use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionEllipseGeometry, CompositionGeometricClip, CompositionRoundedRectangleGeometry,
    Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, RegisterClassW, SetWindowDisplayAffinity, SetWindowPos,
    ShowWindow, HTTRANSPARENT, WDA_EXCLUDEFROMCAPTURE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WM_NCHITTEST, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use crate::geometry::Rect;

/// La forma del vidrio, en coordenadas lógicas del overlay.
#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    /// Esquinas en orden: arriba-izquierda, arriba-derecha, abajo-derecha,
    /// abajo-izquierda (como `liquid::rounded_rect_corners`).
    Rounded { rect: Rect, radii: [f32; 4] },
    /// Dos rectángulos redondeados que se tocan: la columna de la tira y el
    /// bloque que sale a su lado en un costado.
    Pair { a: (Rect, [f32; 4]), b: (Rect, [f32; 4]) },
    Circle { center: (f32, f32), r: f32 },
}

pub fn enabled() -> bool {
    std::env::var("PILL_GLASS").as_deref() != Ok("off")
}

/// Si Windows da el fondo desenfocado. Con los «Efectos de transparencia»
/// apagados (o con el ahorro de energía) `HostBackdropBrush` sale negro, y la
/// pill tiene que usar su piel sin vidrio. Se relee cada segundo: se puede
/// cambiar con la pill abierta.
pub fn backdrop_available() -> bool {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, bool)>> = std::sync::Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return true;
    };
    if let Some((at, on)) = *cache {
        if at.elapsed() < std::time::Duration::from_secs(1) {
            return on;
        }
    }
    let on = windows::UI::ViewManagement::UISettings::new()
        .and_then(|settings| settings.AdvancedEffectsEnabled())
        .unwrap_or(true);
    *cache = Some((std::time::Instant::now(), on));
    on
}

/// Lo que se le pidió a Windows la última vez, en píxeles físicos: si no
/// cambió, no se toca nada.
#[derive(Clone, Copy, PartialEq)]
struct Placed {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// La primera pieza (o la única), relativa a la ventana.
    a: Piece,
    /// La segunda, si la forma es de dos piezas.
    b: Option<Piece>,
    circle: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct Piece {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    radii: [i32; 4],
}

/// Un rectángulo redondeado con su visual, su geometría y su recorte.
struct Layer {
    visual: SpriteVisual,
    rounded: CompositionRoundedRectangleGeometry,
    clip: CompositionGeometricClip,
}

impl Layer {
    fn new(compositor: &Compositor, root: &ContainerVisual) -> windows::core::Result<Self> {
        let visual = compositor.CreateSpriteVisual()?;
        visual.SetBrush(&compositor.CreateHostBackdropBrush()?)?;
        let rounded = compositor.CreateRoundedRectangleGeometry()?;
        let clip = compositor.CreateGeometricClipWithGeometry(&rounded)?;
        visual.SetClip(&clip)?;
        root.Children()?.InsertAtTop(&visual)?;
        Ok(Self { visual, rounded, clip })
    }

    fn shape(&self, piece: &Piece) -> windows::core::Result<()> {
        let (w, h) = (piece.w as f32, piece.h as f32);
        self.visual.SetOffset(Vector3 { X: piece.x as f32, Y: piece.y as f32, Z: 0.0 })?;
        self.visual.SetSize(Vector2 { X: w, Y: h })?;
        let (offset, size, r) = extended(w, h, piece.radii);
        self.rounded.SetOffset(offset)?;
        self.rounded.SetSize(size)?;
        self.rounded.SetCornerRadius(Vector2 { X: r, Y: r })?;
        Ok(())
    }
}

pub struct Glass {
    hwnd: HWND,
    placed: Option<Placed>,
    /// La primera pieza; también es la gota (con el recorte de elipse).
    main: Layer,
    /// La segunda pieza, escondida si la forma es de una.
    second: Layer,
    ellipse: CompositionEllipseGeometry,
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
            // Cubre todo el overlay: `LAYERED | TRANSPARENT` deja pasar los
            // clics a cualquier ventana de abajo (`HTTRANSPARENT` solo llega a
            // las del mismo hilo).
            WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | WS_EX_TOPMOST
                | WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT,
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
        // Siempre fuera de capturas, grabaciones y pantalla compartida: no
        // tiene contenido propio, y ahí el fondo desenfocado sale negro.
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);

        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = interop.CreateDesktopWindowTarget(hwnd, true)?;
        let root = compositor.CreateContainerVisual()?;
        let main = Layer::new(&compositor, &root)?;
        let second = Layer::new(&compositor, &root)?;
        second.visual.SetIsVisible(false)?;
        let ellipse = compositor.CreateEllipseGeometry()?;
        let ellipse_clip = compositor.CreateGeometricClipWithGeometry(&ellipse)?;
        target.SetRoot(&root)?;
        Ok(Self {
            hwnd,
            placed: None,
            main,
            second,
            ellipse,
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
        let (a, b, circle) = match shape {
            Shape::Rounded { rect, radii } => ((rect, radii), None, false),
            Shape::Pair { a, b } => (a, Some(b), false),
            Shape::Circle { center, r } => ((Rect::centered(center, r * 2.0, r * 2.0), [r; 4]), None, true),
        };
        // La ventana queda fija sobre el área cliente del overlay y solo se
        // mueven los recortes: mover la ventana y cambiar el recorte en el
        // mismo cuadro no llegaban juntos a la pantalla, y el vidrio quedaba
        // corrido de la piel. Los cambios de Composition sí salen todos en un
        // mismo lote. Cada pieza va en píxeles físicos desde el origen del
        // overlay, así las dos se tocan sin una raya entre ellas.
        let mut client = RECT::default();
        if unsafe { GetClientRect(above, &mut client) }.is_err() {
            return;
        }
        let px = |v: f32| (v * scale).round() as i32;
        let pair = b.is_some();
        let piece = |(rect, radii): (Rect, [f32; 4])| Piece {
            x: px(rect.x),
            y: px(rect.y),
            // Sola, como siempre; de a dos, de borde a borde para que se toquen.
            w: if pair { px(rect.right()) - px(rect.x) } else { px(rect.w) }.max(1),
            h: if pair { px(rect.bottom()) - px(rect.y) } else { px(rect.h) }.max(1),
            radii: radii.map(px),
        };
        let placed = Placed {
            x: origin.0,
            y: origin.1,
            w: (client.right - client.left).max(1),
            h: (client.bottom - client.top).max(1),
            a: piece(a),
            b: b.map(piece),
            circle,
        };
        if self.placed == Some(placed) {
            return;
        }
        let reshaped = self
            .placed
            .is_none_or(|old| old.a != placed.a || old.b != placed.b || old.circle != circle);
        let switched = self.placed.is_none_or(|old| old.circle != circle);
        // La ventana solo se toca al mostrarse o si el overlay cambió de
        // monitor o de tamaño.
        let moved = self
            .placed
            .is_none_or(|old| (old.x, old.y, old.w, old.h) != (placed.x, placed.y, placed.w, placed.h));
        self.placed = Some(placed);
        if reshaped {
            if let Err(error) = self.reshape(&placed, switched) {
                eprintln!("vidrio: {error}");
            }
        }
        if moved {
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
    }

    /// El recorte con antialias. El rectángulo redondeado de Composition
    /// tiene un solo radio: para el tab, con las esquinas del borde rectas,
    /// el rectángulo se alarga hacia ese lado y el visual corta lo que sobra.
    fn reshape(&self, placed: &Placed, switched: bool) -> windows::core::Result<()> {
        let a = &placed.a;
        if placed.circle {
            if switched {
                self.main.visual.SetClip(&self.ellipse_clip)?;
            }
            let (w, h) = (a.w as f32, a.h as f32);
            self.main.visual.SetOffset(Vector3 { X: a.x as f32, Y: a.y as f32, Z: 0.0 })?;
            self.main.visual.SetSize(Vector2 { X: w, Y: h })?;
            self.ellipse.SetCenter(Vector2 { X: w / 2.0, Y: h / 2.0 })?;
            self.ellipse.SetRadius(Vector2 { X: w / 2.0, Y: h / 2.0 })?;
        } else {
            if switched {
                self.main.visual.SetClip(&self.main.clip)?;
            }
            self.main.shape(a)?;
        }
        match &placed.b {
            Some(b) => {
                self.second.shape(b)?;
                self.second.visual.SetIsVisible(true)?;
            }
            None => self.second.visual.SetIsVisible(false)?,
        }
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
