//! El flip: la ventana del frente se «da vuelta» y detrás aparece el tablero.
//!
//! En Atic es una ventana de WebView2 transparente con la captura de la
//! ventana y una transformación 3D de CSS. GPUI no tiene transformaciones con
//! perspectiva, así que el giro se arma con tiras verticales de la misma
//! captura: cada una se coloca, se estira y se oscurece según su profundidad,
//! y juntas dan la perspectiva. La captura sale de `capture::freeze` y se
//! dibuja en la ventana del overlay que ya existe.
//!
//! La ventana real se esconde mientras dura (como `window_flip.rs`) y se
//! devuelve a su sitio al volver. La cara trasera es el tablero real de Atic,
//! en solo lectura: sus notas de texto y listas.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    actions, div, img, prelude::*, px, rgb, AnyElement, App, Context, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, KeyBinding, MouseDownEvent, RenderImage, SharedString,
    Subscription, Window,
};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowRect, IsWindow, IsZoomed, SetWindowPlacement, ShowWindow, SW_HIDE,
    SW_SHOWNOACTIVATE, WINDOWPLACEMENT,
};

use crate::anim::{cubic_bezier, Tween};
use crate::geometry::Rect;

/// «Flip» en la tira y la rueda.
pub const TOOL: usize = 8;

/// Tiras por tarjeta: con más no se nota diferencia y cada una es un dibujo.
const STRIPS: usize = 120;
/// Tiras en cada esquina redondeada.
const CORNER_STRIPS: usize = 5;
/// Distancia de la cámara, en veces el lado mayor de la tarjeta.
const CAMERA: f32 = 2.4;
const FLIP: Duration = Duration::from_millis(620);
/// Tras mostrar la tarjeta, cuánto esperar para esconder la ventana real: el
/// overlay tiene que haberla dibujado antes o se ve un parpadeo.
const HIDE_AFTER: Duration = Duration::from_millis(60);
/// Desde este ángulo (de 180°) la cara trasera es la interfaz de verdad.
const SETTLED: f32 = std::f32::consts::PI - 0.012;

actions!(flip, [Close]);

const KEY_CONTEXT: &str = "Flip";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some(KEY_CONTEXT))]);
}

fn ease_flip(t: f32) -> f32 {
    cubic_bezier(0.55, 0.0, 0.25, 1.0, t)
}

pub enum FlipEvent {
    /// Ya volvió: se puede soltar la ventana del overlay.
    Closed,
}

/// Dónde estaba la ventana escondida, para devolverla igual: `SW_HIDE` de una
/// maximizada la regresa restaurada y en el centro.
#[derive(Clone, Copy)]
struct Placement {
    show_cmd: u32,
    flags: u32,
    rect: RECT,
}

struct Hidden {
    hwnd: isize,
    placement: Option<Placement>,
}

/// El marco visible de la ventana: `GetWindowRect` incluye bordes invisibles
/// de DWM (unos 7 px a cada lado) y la tarjeta quedaría desfasada.
fn rect_of(hwnd: isize) -> Option<Rect> {
    use windows::Win32::Foundation::{HWND, RECT as WinRect};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};

    let mut frame = WinRect::default();
    let ok = unsafe {
        DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut _ as *mut _,
            std::mem::size_of::<WinRect>() as u32,
        )
    }
    .is_ok();
    let rect = if ok {
        RECT {
            left: frame.left,
            top: frame.top,
            right: frame.right,
            bottom: frame.bottom,
        }
    } else {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd as _, &mut rect) } == 0 {
            return None;
        }
        rect
    };
    Some(Rect::new(
        rect.left as f32,
        rect.top as f32,
        (rect.right - rect.left) as f32,
        (rect.bottom - rect.top) as f32,
    ))
}

/// El escritorio y la barra de tareas también son ventanas con foco, pero no
/// son lo que alguien quiere dar vuelta.
fn is_shell(hwnd: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    let class = String::from_utf16_lossy(&buffer[..len.max(0) as usize]);
    matches!(
        class.as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW"
    )
}

/// Nombre del ejecutable de un proceso, en minúsculas.
fn exe_name(pid: u32) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut buffer = [0u16; 520];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
    }
}

/// Una ventana que vale la pena dar vuelta: visible, con título, sin
/// minimizar, de tamaño real (Atic y otras apps dejan ventanas de 1×1 px que
/// cuentan como visibles), que Windows no tenga oculta (otro escritorio
/// virtual), que no sea del escritorio ni de la barra, ni de esta app.
fn is_flippable(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        GWL_EXSTYLE, SM_CXSCREEN, SM_CYSCREEN, WS_EX_TOOLWINDOW,
    };

    let raw = hwnd as windows_sys::Win32::Foundation::HWND;
    unsafe {
        if IsWindowVisible(raw) == 0 || IsIconic(raw) != 0 || is_shell(hwnd) {
            return false;
        }
        if GetWindowLongW(raw, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(raw, &mut pid);
        // Ni esta app ni la de escritorio de Atic: su overlay transparente
        // ocupa toda la pantalla y siempre está al frente.
        if pid == std::process::id()
            || title_of(hwnd).is_empty()
            || matches!(exe_name(pid).as_str(), "atic-desktop.exe" | "atic.exe")
        {
            return false;
        }
        // Con una parte visible en el monitor principal: Atic deja ventanas
        // grandes aparcadas fuera de pantalla.
        let screen = (
            GetSystemMetrics(SM_CXSCREEN) as f32,
            GetSystemMetrics(SM_CYSCREEN) as f32,
        );
        let visible = |r: &Rect| {
            (r.right().min(screen.0) - r.x.max(0.0)).min(r.bottom().min(screen.1) - r.y.max(0.0))
        };
        let size_ok = |r: &Rect| {
            let w = (r.right().min(screen.0) - r.x.max(0.0)).max(0.0);
            let h = (r.bottom().min(screen.1) - r.y.max(0.0)).max(0.0);
            visible(r) > 0.0 && w >= 120.0 && h >= 80.0
        };
        if !rect_of(hwnd).is_some_and(|r| size_ok(&r)) {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        cloaked == 0
    }
}

/// La ventana real más al frente, en orden de apilado.
fn frontmost_real_window() -> Option<isize> {
    use windows_sys::Win32::Foundation::{BOOL, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;

    unsafe extern "system" fn visit(hwnd: windows_sys::Win32::Foundation::HWND, found: LPARAM) -> BOOL {
        if is_flippable(hwnd as isize) {
            *(found as *mut Option<isize>) = Some(hwnd as isize);
            return 0;
        }
        1
    }

    let mut found: Option<isize> = None;
    unsafe {
        EnumWindows(Some(visit), &mut found as *mut _ as LPARAM);
    }
    found
}

fn title_of(hwnd: isize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn hide(hwnd: isize) -> Option<Hidden> {
    if unsafe { IsWindow(hwnd as _) } == 0 {
        return None;
    }
    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    let saved = (unsafe { GetWindowPlacement(hwnd as _, &mut placement) } != 0).then_some(
        Placement {
            show_cmd: placement.showCmd,
            flags: placement.flags,
            rect: placement.rcNormalPosition,
        },
    );
    unsafe {
        let _ = ShowWindow(hwnd as _, SW_HIDE);
    }
    Some(Hidden {
        hwnd,
        placement: saved,
    })
}

fn restore(hidden: &Hidden) {
    if unsafe { IsWindow(hidden.hwnd as _) } == 0 {
        return;
    }
    let Some(saved) = hidden.placement else {
        unsafe {
            let _ = ShowWindow(hidden.hwnd as _, SW_SHOWNOACTIVATE);
        }
        return;
    };
    let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    placement.flags = saved.flags;
    placement.showCmd = saved.show_cmd;
    placement.rcNormalPosition = saved.rect;
    if unsafe { SetWindowPlacement(hidden.hwnd as _, &placement) } == 0 {
        unsafe {
            let _ = ShowWindow(hidden.hwnd as _, SW_SHOWNOACTIVATE);
        }
    }
}

struct Colors {
    back: Hsla,
}

pub struct FlipView {
    image: Arc<RenderImage>,
    /// Tamaño lógico de la pantalla congelada y dónde cae en la ventana.
    screen: (f32, f32),
    offset: (f32, f32),
    /// La tarjeta, en coordenadas lógicas del monitor.
    card: Rect,
    /// 0 = la ventana de frente, π = el tablero de frente.
    angle: Tween,
    /// `PILL_FLIP_ANGLE=0..1`: congela el giro en esa fracción, para revisar
    /// la geometría sin esperar la animación.
    fixed: Option<f32>,
    hidden: Option<Hidden>,
    closing: bool,
    /// El tablero del reverso.
    paper: Entity<crate::flip_board::PaperView>,
    paper_focused: bool,
    /// Radio de las esquinas de la ventana, en lógico; 0 si está maximizada.
    radius: f32,
    /// El monitor, para que el tablero capture sus páginas.
    env: crate::flip_board::Env,
    focus: FocusHandle,
    colors: Colors,
    _paper_events: Subscription,
}

impl EventEmitter<FlipEvent> for FlipView {}

impl Focusable for FlipView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for FlipView {
    /// Si la vista se suelta de golpe, la ventana del usuario no se queda
    /// escondida.
    fn drop(&mut self) {
        if let Some(hidden) = self.hidden.take() {
            restore(&hidden);
        }
    }
}

impl FlipView {
    fn new(
        frozen: &crate::capture::Frozen,
        card: Rect,
        target: Option<isize>,
        radius: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let scale = frozen.scale;
        let mut angle = Tween::new(0.0, FLIP, ease_flip);
        angle.set(std::f32::consts::PI, Instant::now());
        let paper = cx.new(crate::flip_board::PaperView::new);
        let paper_events = cx.subscribe(&paper, |view, _, event: &crate::flip_board::PaperEvent, cx| {
            let crate::flip_board::PaperEvent::Close = event;
            view.start_close(cx);
        });
        let view = Self {
            image: frozen.image.clone(),
            screen: (
                frozen.frame.width() as f32 / scale,
                frozen.frame.height() as f32 / scale,
            ),
            offset: frozen.offset,
            card,
            angle,
            fixed: std::env::var("PILL_FLIP_ANGLE")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .map(|v| v.clamp(0.0, 1.0) * std::f32::consts::PI),
            hidden: None,
            closing: false,
            paper,
            paper_focused: false,
            radius,
            env: crate::flip_board::Env {
                screen: (
                    frozen.frame.width() as f32 / scale,
                    frozen.frame.height() as f32 / scale,
                ),
                offset: frozen.offset,
                dpi: scale,
                origin_phys: (frozen.frame.bounds.x, frozen.frame.bounds.y),
            },
            _paper_events: paper_events,
            focus: cx.focus_handle(),
            colors: Colors {
                back: rgb(0x1b1b19).into(),
            },
        };
        // Esconde la ventana real cuando el overlay ya dibujó la tarjeta.
        // `PILL_FLIP_NOHIDE=1`: no toca la ventana real (pruebas).
        let target = target.filter(|_| std::env::var_os("PILL_FLIP_NOHIDE").is_none());
        if let Some(target) = target {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(HIDE_AFTER).await;
                let _ = this.update(cx, |view, cx| {
                    if view.hidden.is_none() {
                        view.hidden = hide(target);
                        println!("flip: ventana {target:#x} escondida: {}", view.hidden.is_some());
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        view.tick(cx);
        view
    }

    /// Redibuja hasta que el giro termina; al volver, devuelve la ventana y
    /// avisa.
    fn tick(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(8))
                .await;
            let Ok(done) = this.update(cx, |view, cx| {
                let now = Instant::now();
                cx.notify();
                !view.angle.is_running(now)
            }) else {
                break;
            };
            if done {
                let _ = this.update(cx, |view, cx| {
                    if view.closing {
                        if let Some(hidden) = view.hidden.take() {
                            restore(&hidden);
                        }
                        cx.emit(FlipEvent::Closed);
                    }
                });
                break;
            }
        })
        .detach();
    }

    fn close(&mut self, _: &Close, _: &mut Window, cx: &mut Context<Self>) {
        self.start_close(cx);
    }

    fn start_close(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.angle.set(0.0, Instant::now());
        self.tick(cx);
        cx.notify();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Fuera de la tarjeta, o en su ✕, vuelve.
        let x = f32::from(event.position.x) - self.offset.0;
        let y = f32::from(event.position.y) - self.offset.1;
        if !self.card.contains((x, y), 0.0) {
            self.start_close(cx);
        }
    }

    /// Dónde empieza cada tira, como distancia desde el borde izquierdo de la
    /// tarjeta. Cerca de los costados van más finas: ahí se recortan las
    /// esquinas redondeadas y una tira ancha las dejaría en escalones.
    fn boundaries(&self) -> Vec<f32> {
        let w = self.card.w;
        let r = self.radius.min(w / 2.0).min(self.card.h / 2.0);
        let mut b = Vec::with_capacity(STRIPS + 10);
        let corner = if r > 0.5 { r } else { 0.0 };
        if corner > 0.0 {
            b.extend((0..CORNER_STRIPS).map(|i| corner * i as f32 / CORNER_STRIPS as f32));
        }
        let inner = w - corner * 2.0;
        b.extend((0..STRIPS).map(|i| corner + inner * i as f32 / STRIPS as f32));
        if corner > 0.0 {
            b.extend((0..CORNER_STRIPS).map(|i| w - corner + corner * i as f32 / CORNER_STRIPS as f32));
        }
        b.push(w);
        b
    }

    /// Una tira de la tarjeta, ya proyectada, entre `from` y `to` (distancias
    /// desde el borde izquierdo).
    fn strip(&self, from: f32, to: f32, theta: f32) -> AnyElement {
        let card = self.card;
        let w = card.w;
        let camera = CAMERA * card.w.max(card.h);
        let project = |u: f32| {
            let z = u * theta.sin();
            let s = camera / (camera - z);
            (u * theta.cos() * s, s)
        };
        let (u0, u1) = (from - w / 2.0, to - w / 2.0);
        let ((x0, s0), (x1, s1)) = (project(u0), project(u1));
        let s = (s0 + s1) / 2.0;
        let left = x0.min(x1);
        let exact = (x1 - x0).abs();
        // Cada tira se pasa un píxel por cada lado para que entre ellas no
        // queden rendijas; la escala de la imagen sale del ancho exacto.
        const OVERLAP: f32 = 1.0;
        let left = left - OVERLAP;
        let width = exact + OVERLAP * 2.0;
        let front = theta < std::f32::consts::FRAC_PI_2;
        let cx = self.offset.0 + card.x + w / 2.0;
        let cy = self.offset.1 + card.y + card.h / 2.0;

        // Esquinas: cuánto se recorta arriba y abajo según qué tan cerca del
        // costado cae la tira (la ecuación del círculo).
        let r = self.radius.min(w / 2.0).min(card.h / 2.0);
        let mid = (from + to) / 2.0;
        let edge = mid.min(w - mid);
        let inset_src = if r > 0.5 && edge < r {
            r - (r * r - (r - edge) * (r - edge)).max(0.0).sqrt()
        } else {
            0.0
        };
        let inset = inset_src * s;
        let height = (card.h * s - inset * 2.0).max(1.0);

        // Más oscuro cuanto más de canto y más lejos de la cámara.
        let shade = (0.45 * theta.sin().powf(0.8) + (1.0 - s).max(0.0) * 1.6).min(0.72);

        let strip = div()
            .absolute()
            .left(px(cx + left))
            .top(px(cy - card.h * s / 2.0 + inset))
            .w(px(width))
            .h(px(height))
            .overflow_hidden();
        let strip = if front {
            // El recorte de la captura que le toca, estirado a lo que mide la
            // tira.
            let ratio = exact / (to - from).max(0.01);
            strip.child(
                img(self.image.clone())
                    .absolute()
                    .left(px(-(card.x + from) * ratio + OVERLAP))
                    .top(px(-card.y * s - inset))
                    .w(px(self.screen.0 * ratio))
                    .h(px(self.screen.1 * s)),
            )
        } else {
            strip.bg(self.colors.back)
        };
        strip
            .child(
                div()
                    .absolute()
                    .size_full()
                    .bg(gpui::black().opacity(shade)),
            )
            .into_any_element()
    }

    /// El tablero, ya de frente, con las mismas esquinas que la ventana.
    fn back(&self) -> AnyElement {
        let card = self.card;
        div()
            .absolute()
            .left(px(self.offset.0 + card.x))
            .top(px(self.offset.1 + card.y))
            .w(px(card.w))
            .h(px(card.h))
            .child(self.paper.clone())
            .into_any_element()
    }
}

impl Render for FlipView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let theta = self.fixed.unwrap_or_else(|| self.angle.value(now));
        if self.fixed.is_none() && self.angle.is_running(now) {
            window.request_animation_frame();
        }
        let root = div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::close))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::on_mouse_down))
            .size_full();
        if theta >= SETTLED {
            // El tablero necesita saber dónde está y cuánto mide, y el foco.
            let origin = (self.offset.0 + self.card.x, self.offset.1 + self.card.y);
            let size = (self.card.w, self.card.h);
            let radius = self.radius;
            let env = self.env;
            self.paper.update(cx, |paper, _| paper.sync(origin, size, radius, env));
            if !self.paper_focused && !self.closing {
                self.paper_focused = true;
                window.focus(&self.paper.focus_handle(cx));
            }
            root.child(self.back())
        } else {
            let bounds = self.boundaries();
            root.children(
                bounds
                    .windows(2)
                    .filter(|pair| pair[1] > pair[0])
                    .map(|pair| self.strip(pair[0], pair[1], theta)),
            )
        }
    }
}

// --- En la pill ------------------------------------------------------------

impl crate::Pill {
    pub(crate) fn start_flip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flip.is_some()
            || self.capture.is_some()
            || self.board.is_some()
            || self.capture_pending
        {
            return;
        }
        self.capture_pending = true;
        let now = Instant::now();
        self.close_wheel();
        self.close_panel(false, cx);
        self.peek.set(0.0, now);
        self.strip.set(0.0, now);
        // `PILL_FLIP_HWND=<hex>`: la ventana a voltear, para pruebas; con
        // `PILL_OPEN` el foco aún no es la ventana del usuario.
        let target = std::env::var("PILL_FLIP_HWND")
            .ok()
            .and_then(|v| isize::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .or_else(|| crate::paste::foreground_target().filter(|&hwnd| is_flippable(hwnd)))
            .or_else(frontmost_real_window);
        // El notch no debe salir en la foto.
        if let Some(overlay) = self.overlay.as_ref() {
            overlay.exclude_from_capture(true);
        }
        let scale = self.scale_factor;
        cx.spawn_in(window, async move |this, cx| {
            let frozen = cx
                .background_spawn(async move { crate::capture::freeze(scale) })
                .await;
            let _ = this.update_in(cx, |pill, window, cx| {
                pill.capture_pending = false;
                if let Some(overlay) = pill.overlay.as_ref() {
                    overlay.exclude_from_capture(false);
                }
                let mut frozen = match frozen {
                    Ok(frozen) => frozen,
                    Err(error) => {
                        eprintln!("flip: no se pudo congelar: {error}");
                        return;
                    }
                };
                frozen.offset = (pill.monitor.x, pill.monitor.y);
                pill.show_flip(frozen, target, window, cx);
            });
        })
        .detach();
    }

    fn show_flip(
        &mut self,
        frozen: crate::capture::Frozen,
        target: Option<isize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // La tarjeta es la ventana que tenía el foco, con su marco visible. Sin
        // ella (no se supo cuál era), la ventana más al frente que hay.
        let scale = frozen.scale;
        let origin = frozen.frame.bounds;
        let screen = Rect::new(
            0.0,
            0.0,
            frozen.frame.width() as f32 / scale,
            frozen.frame.height() as f32 / scale,
        );
        let known = target.and_then(rect_of).map(|r| {
            let card = Rect::new(
                (r.x - origin.x as f32) / scale,
                (r.y - origin.y as f32) / scale,
                r.w / scale,
                r.h / scale,
            );
            // Dentro de la pantalla capturada: lo que sobresale no se vio.
            let (x, y) = (card.x.max(0.0), card.y.max(0.0));
            let (right, bottom) = (card.right().min(screen.w), card.bottom().min(screen.h));
            (
                Rect::new(x, y, (right - x).max(1.0), (bottom - y).max(1.0)),
                SharedString::from(title_of(target.unwrap_or_default())),
            )
        });
        let pick = known.or_else(|| frozen.windows.first().map(|c| (c.rect, c.title.clone())));
        let Some((card, title)) = pick else {
            eprintln!("flip: no hay ninguna ventana que dar vuelta");
            return;
        };
        println!("flip: «{title}» {}×{}", card.w, card.h);
        // Las ventanas de Win11 tienen esquinas de 8 px, salvo maximizadas.
        let radius = target
            .filter(|&hwnd| unsafe { IsZoomed(hwnd as _) } == 0)
            .map_or(0.0, |_| 8.0 / frozen.scale);
        let view = cx.new(|cx| FlipView::new(&frozen, card, target, radius, cx));
        self.flip_events = Some(cx.subscribe(&view, |pill, _, event: &FlipEvent, cx| {
            let FlipEvent::Closed = event;
            pill.end_flip(cx)
        }));
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_passthrough(false);
            overlay.set_focusable(true);
        }
        window.focus(&view.focus_handle(cx));
        self.flip_target = target;
        self.flip = Some(view);
        cx.notify();
    }

    fn end_flip(&mut self, cx: &mut Context<Self>) {
        self.flip = None;
        self.flip_events = None;
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.set_focusable(false);
        }
        if let Some(target) = self.flip_target.take() {
            crate::paste::force_foreground(target);
        }
        cx.notify();
    }
}
