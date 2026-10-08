//! El espacio de agentes: una ventana con un plano infinito donde viven las
//! consolas, como la pizarra de agentes de Atic pero nativa.
//!
//! Lo que cambia respecto de `AgentsBoard.svelte` (ver
//! `docs/RENDIMIENTO_PIZARRA_AGENTES.md`):
//! - Las terminales son `alacritty_terminal` dibujado por GPUI: un solo
//!   renderer para todas, sin un contexto WebGL por consola.
//! - El zoom no escala un bitmap: el texto se compone al tamaño real, nítido
//!   a cualquier zoom, y el mouse no necesita corrección.
//! - Lo que queda fuera de la vista no se dibuja (sigue procesando su salida).
//! - Zoom semántico: de lejos cada consola es una tarjeta con su estado y sus
//!   últimas líneas, no una terminal ilegible.
//! - Sin blur detrás de los paneles: era el mayor costo de GPU en Atic.
//!
//! `SPACE_DEMO=1` abre 6 consolas (4 escupiendo 30 líneas/s, como la prueba
//! del diagnóstico) y `SPACE_BENCH=1` mueve la cámara sola y mide.

pub(crate) mod chrome;
pub(crate) mod console;
mod folders;
mod input;
mod mando;
mod persist;

/// Lo que el notch necesita del espacio: cuántos agentes hay vivos y escribirles.
pub use persist::{agent_consoles, send_to_agents};

use std::path::PathBuf;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::CursorShape;
use futures::StreamExt;
use gpui::{
    canvas, div, point, prelude::*, px, quad, size, App, Bounds, BoxShadow, ClickEvent, Context,
    ContentMask, Corners, FocusHandle, Focusable, Font, FontStyle, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollWheelEvent,
    SharedString, TextRun, Window,
};

use console::{Console, GridSize, Launch};

/// Tamaño de la letra de las consolas con zoom 1.
const FONT_SIZE: f32 = 13.0;
const LINE_HEIGHT: f32 = 1.3;
/// Encabezado de la tarjeta y margen del texto, en unidades del plano.
const HEADER: f32 = 30.0;
const PAD: f32 = 8.0;
const RADIUS: f32 = 14.0;
/// Esquina para cambiar el tamaño.
const GRIP: f32 = 14.0;
/// Zoom semántico. Desde `TEXT_ZOOM` la terminal es texto (letra de 8,5 px
/// o más); entre `LIVE_ZOOM` y `TEXT_ZOOM`, la silueta de su salida en vivo;
/// por debajo, una tarjeta con el agente y sus últimas líneas.
const TEXT_ZOOM: f32 = 0.65;
const LIVE_ZOOM: f32 = 0.3;
const MIN_ZOOM: f32 = 0.12;
const MAX_ZOOM: f32 = 2.0;
/// Sin salida por este tiempo, la consola está «lista».
const WORKING_FOR: Duration = Duration::from_millis(1500);
/// Es también la barra de la ventana (la nativa se esconde): mide lo mismo que
/// la del Mando.
const TOOLBAR_H: f32 = 48.0;
const FONT_FAMILY: &str = "Cascadia Mono";

const BG: u32 = 0x0f0f0e;
const CARD: u32 = console::BACKGROUND;
const HEADER_BG: u32 = 0x1d1d1b;
const TEXT: u32 = 0xf0f0ea;
const MUTED: u32 = 0x9a9a90;
const FAINT: u32 = 0x5a5a54;
/// El encabezado de la tarjeta enfocada: se distingue por ser más claro, sin marco.
const HEADER_ON: u32 = 0x2d2d2a;
const WORKING: u32 = 0xe8b04b;
const READY: u32 = 0x6cc48a;

/// Un rectángulo en unidades del plano.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Area {
    fn contains(&self, (x, y): (f32, f32)) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// `pantalla = plano × zoom + desplazamiento`, como `agentBoard.ts`.
#[derive(Clone, Copy, Debug)]
struct Camera {
    x: f32,
    y: f32,
    zoom: f32,
}

impl Camera {
    fn to_screen(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (x * self.zoom + self.x, y * self.zoom + self.y)
    }

    fn to_board(&self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.x) / self.zoom, (y - self.y) / self.zoom)
    }

    fn area(&self, area: &Area) -> Bounds<Pixels> {
        let (x, y) = self.to_screen((area.x, area.y));
        Bounds::new(point(px(x), px(y)), size(px(area.w * self.zoom), px(area.h * self.zoom)))
    }

    /// Zoom alrededor de un punto de la pantalla: lo que está bajo el cursor
    /// se queda bajo el cursor.
    fn zoom_at(&mut self, zoom: f32, anchor: (f32, f32)) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let board = self.to_board(anchor);
        self.zoom = zoom;
        self.x = anchor.0 - board.0 * zoom;
        self.y = anchor.1 - board.1 * zoom;
    }
}

pub struct Card {
    id: u64,
    console: Console,
    area: Area,
    label: SharedString,
    /// Con qué agente se abrió (`claude`, `codex`…), para el logo.
    agent: Option<&'static str>,
    /// La carpeta donde trabaja: el Mando la dice bajo el nombre.
    cwd: Option<PathBuf>,
    /// Sigue trabajando pero no se muestra en el Mando: se dejó «en segundo
    /// plano» y vuelve desde la bandeja. La pizarra las muestra todas.
    background: bool,
    /// La marca de una consola de agente con los hooks de Atic
    /// (`agent_prompts`): sus permisos se contestan desde la bandeja.
    token: Option<String>,
}

/// Qué se está arrastrando.
#[derive(Clone, Copy)]
enum Drag {
    Pan { from: (f32, f32), camera: Camera },
    Move { id: u64, from: (f32, f32), area: Area },
    Resize { id: u64, from: (f32, f32), area: Area },
}

/// Lo que mide la cámara del cuadro, en píxeles de pantalla.
#[derive(Clone, Copy)]
struct Cell {
    w: f32,
    h: f32,
}

/// `SPACE_BENCH=1`: la cámara se mueve sola por fases y se miden los
/// intervalos entre cuadros, como la prueba del diagnóstico.
struct Bench {
    started: Instant,
    base: Camera,
    phase: usize,
    gaps: Vec<f32>,
    paints: Vec<f32>,
    builds: Vec<f32>,
    last: Option<Instant>,
}

const BENCH_PHASES: [(&str, f32); 4] = [
    ("quieto", 3.0),
    ("pan", 5.0),
    ("zoom", 5.0),
    ("zoom semántico", 5.0),
];

pub struct SpaceView {
    focus: FocusHandle,
    cards: Vec<Card>,
    focused: Option<u64>,
    camera: Camera,
    drag: Option<Drag>,
    next_id: u64,
    wake: futures::channel::mpsc::UnboundedSender<()>,
    /// Celda con zoom 1, medida con la fuente real.
    cell: Option<Cell>,
    viewport: (f32, f32),
    /// Se está moviendo la cámara: el texto usa tamaños redondeados, que GPUI
    /// ya tiene compuestos del cuadro anterior.
    moving_until: Instant,
    bench: Option<Bench>,
    meter: crate::meter::Meter,
    paint_ms: std::rc::Rc<std::cell::Cell<f32>>,
    build_ms: f32,
    fonts: [Font; 4],
    pub default_cwd: Option<PathBuf>,
    /// Las carpetas del espacio y la activa (`folders.rs`).
    folders: folders::Folders,
    /// Ajustar todo en el primer cuadro, cuando ya se conoce el tamaño.
    fit_pending: bool,
    /// Mando (cartas, consola enfocada y bandeja) o pizarra.
    view: mando::View,
    mando: mando::State,
    /// La tecla muerta que Windows está componiendo (`´` antes de la `a`).
    marked: Option<String>,
}

impl Focusable for SpaceView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Abre la ventana del espacio, con una consola si viene.
pub fn open_window(
    open: Option<Open>,
    cx: &mut App,
) -> anyhow::Result<gpui::WindowHandle<SpaceView>> {
    let options = gpui::WindowOptions {
        // Sin la barra de Windows: la de cada vista es la de la ventana (ver
        // `chrome`). El título sigue sirviendo para la barra de tareas.
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("Atic · Espacio".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_min_size: Some(size(px(820.), px(560.))),
        window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1360.), px(860.)),
            cx,
        ))),
        focus: true,
        show: true,
        kind: gpui::WindowKind::Normal,
        ..Default::default()
    };
    let handle = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| {
            let mut view = SpaceView::new(window, cx);
            if let Some(open) = open {
                view.open(open, cx);
            }
            view
        })
    })?;
    persist::register(handle, cx);
    persist::recycle_test(handle, cx);
    Ok(handle)
}

/// Lo que se abre en una tarjeta nueva.
#[derive(Clone)]
pub struct Open {
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub agent: Option<&'static str>,
}

impl Open {
    /// Un CLI de agente en `cmd /k`, que deja el prompt al terminar (como la
    /// consola de Atic).
    pub fn agent(cli: &'static str, name: &str, line: &str, cwd: Option<PathBuf>) -> Self {
        Self {
            label: name.into(),
            program: "cmd.exe".into(),
            args: vec!["/K".into(), line.into()],
            cwd,
            agent: Some(cli),
        }
    }

    pub fn shell(cwd: Option<PathBuf>) -> Self {
        Self {
            label: "PowerShell".into(),
            program: powershell(),
            args: vec!["-NoLogo".into()],
            cwd,
            agent: None,
        }
    }
}

/// El tamaño de letra más cercano en la escala `13 × 1,06ⁿ`.
fn snap_size(size: f32) -> f32 {
    let step = 1.06f32.ln();
    let n = ((size / FONT_SIZE).ln() / step).round();
    FONT_SIZE * (n * step).exp()
}

fn font() -> Font {
    let mut font = gpui::font(FONT_FAMILY);
    font.fallbacks = Some(gpui::FontFallbacks::from_fonts(vec!["Consolas".into()]));
    font
}

fn powershell() -> String {
    let pwsh = std::env::var_os("ProgramFiles")
        .map(|dir| PathBuf::from(dir).join("PowerShell").join("7").join("pwsh.exe"));
    match pwsh.filter(|path| path.exists()) {
        Some(path) => path.to_string_lossy().into_owned(),
        None => "powershell.exe".into(),
    }
}

impl SpaceView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (wake, mut woken) = futures::channel::mpsc::unbounded::<()>();
        // Los avisos del PTY se juntan: un render por tanda, no por aviso.
        cx.spawn(async move |this, cx| {
            while woken.next().await.is_some() {
                while woken.try_recv().is_ok() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        // «Trabajando» → «listo» pasa sin salida nueva: se revisa cada medio
        // segundo.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut view = Self {
            focus,
            cards: Vec::new(),
            focused: None,
            camera: Camera {
                x: 40.0,
                y: TOOLBAR_H + 30.0,
                zoom: 1.0,
            },
            drag: None,
            next_id: 1,
            wake,
            cell: None,
            viewport: (1280.0, 800.0),
            moving_until: Instant::now(),
            bench: std::env::var_os("SPACE_BENCH").map(|_| Bench {
                started: Instant::now(),
                base: Camera {
                    x: 40.0,
                    y: TOOLBAR_H + 30.0,
                    zoom: 1.0,
                },
                phase: 0,
                gaps: Vec::new(),
                paints: Vec::new(),
                builds: Vec::new(),
                last: None,
            }),
            meter: crate::meter::Meter::new("espacio"),
            paint_ms: Default::default(),
            build_ms: 0.0,
            fonts: fonts(),
            default_cwd: None,
            folders: folders::Folders::load(),
            fit_pending: false,
            // El Mando es la vista de siempre; `SPACE_VIEW=pizarra` abre la otra.
            view: match std::env::var("SPACE_VIEW").as_deref() {
                Ok("pizarra") => mando::View::Pizarra,
                _ => mando::View::Mando,
            },
            mando: mando::State::default(),
            marked: None,
        };
        view.measure(window);
        // Si se cerró otra ventana del espacio, sus consolas siguieron
        // corriendo: se recogen aquí. Y al soltarse esta, dejará las suyas.
        let restored = persist::restore(&mut view, cx);
        cx.on_release(persist::stash).detach();
        if !restored && std::env::var_os("SPACE_DEMO").is_some() {
            view.demo(cx);
        }
        // `SPACE_MANDO_DEMO=1`: cuatro consolas que fingen ser agentes.
        if !restored && std::env::var_os("SPACE_MANDO_DEMO").is_some() {
            mando::demo(&mut view, cx);
        }
        // `SPACE_SHELL=1`: una PowerShell para probar a mano.
        if std::env::var_os("SPACE_SHELL").is_some() {
            view.open(Open::shell(None), cx);
        }
        view
    }

    fn font(&self) -> Font {
        font()
    }

    /// El ancho de una celda y el alto de una línea con zoom 1.
    fn measure(&mut self, window: &mut Window) {
        let text = window.text_system();
        let id = text.resolve_font(&self.font());
        let w = text
            .advance(id, px(FONT_SIZE), 'm')
            .map(|s| f32::from(s.width))
            .unwrap_or(FONT_SIZE * 0.6);
        self.cell = Some(Cell {
            w,
            h: (FONT_SIZE * LINE_HEIGHT).round(),
        });
    }

    fn cell(&self) -> Cell {
        self.cell.unwrap_or(Cell {
            w: FONT_SIZE * 0.6,
            h: FONT_SIZE * LINE_HEIGHT,
        })
    }

    /// Cuántas columnas y filas caben en una tarjeta.
    fn grid_for(&self, area: &Area) -> GridSize {
        let cell = self.cell();
        GridSize {
            cols: (((area.w - PAD * 2.0) / cell.w).floor() as usize).max(20),
            rows: (((area.h - HEADER - PAD) / cell.h).floor() as usize).max(5),
        }
    }

    fn area_for(&self, cols: usize, rows: usize) -> Area {
        let cell = self.cell();
        Area {
            x: 0.0,
            y: 0.0,
            w: cols as f32 * cell.w + PAD * 2.0,
            h: rows as f32 * cell.h + HEADER + PAD,
        }
    }

    /// Abre una consola nueva en el centro de lo que se ve, corrida si ahí ya
    /// hay otra (como `agentBoard.ts`).
    pub fn open(&mut self, open: Open, cx: &mut Context<Self>) {
        let mut area = self.area_for(100, 28);
        let center = self.camera.to_board((self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0));
        area.x = center.0 - area.w / 2.0;
        area.y = center.1 - area.h / 2.0;
        while self
            .cards
            .iter()
            .any(|c| (c.area.x - area.x).abs() < 8.0 && (c.area.y - area.y).abs() < 8.0)
        {
            area.x += 36.0;
            area.y += 36.0;
        }
        self.open_at(open, area, cx);
    }

    fn open_at(&mut self, open: Open, area: Area, cx: &mut Context<Self>) {
        let cell = self.cell();
        // Si no se pidió carpeta, la del proceso: es donde el PTY ya arrancaba.
        // Sin carpeta pedida: la activa del espacio, la de quien lo abrió o la
        // del proceso, donde el PTY ya arrancaba.
        let cwd = open
            .cwd
            .or_else(|| self.folders.active().cloned())
            .or_else(|| self.default_cwd.clone())
            .or_else(|| std::env::current_dir().ok());
        let mut args = open.args;
        let mut env = Vec::new();
        // Un agente en una carpeta del espacio trabaja también en las otras.
        if let (Some(agent), Some(dir), [flag, line]) = (open.agent, cwd.as_ref(), args.as_mut_slice()) {
            if flag.eq_ignore_ascii_case("/K") {
                *line = folders::with_add_dirs(agent, line, &self.folders.extras(dir));
            }
        }
        // Agentes con hooks: la línea los lleva y el entorno, la marca.
        let token = match (open.agent, args.as_slice()) {
            (Some(agent), [flag, line]) if flag.eq_ignore_ascii_case("/K") => {
                crate::agent_prompts::prepare(agent, line).map(|(hooked, token)| {
                    args[1] = hooked;
                    env.push((crate::agent_prompts::token_var().to_string(), token.clone()));
                    token
                })
            }
            _ => None,
        };
        let launch = Launch {
            program: open.program,
            args,
            cwd: cwd.clone(),
            env,
        };
        match Console::spawn(
            launch,
            self.grid_for(&area),
            (cell.w.round() as u16, cell.h as u16),
            self.wake.clone(),
        ) {
            Ok(console) => {
                if let (Some(token), Some(agent)) = (&token, open.agent) {
                    crate::agent_prompts::register(token, agent, console.writer());
                }
                let id = self.next_id;
                self.next_id += 1;
                self.cards.push(Card {
                    id,
                    console,
                    area,
                    label: open.label.into(),
                    agent: open.agent,
                    cwd,
                    background: false,
                    token,
                });
                self.focused = Some(id);
                // Que se vea entera si quedó fuera.
                self.reveal(id);
            }
            Err(error) => eprintln!("espacio: no se pudo abrir la consola: {error}"),
        }
        cx.notify();
    }

    fn demo(&mut self, cx: &mut Context<Self>) {
        let flood = "while ($true) { 1..30 | ForEach-Object { Write-Host ((\"linea {0:d2} {1:o} \" -f $_, (Get-Date)) + ('·' * 60)) -ForegroundColor (@('Green','Cyan','Yellow','Magenta')[$_ % 4]) }; Start-Sleep -Milliseconds 1000 }";
        let base = self.area_for(96, 26);
        for index in 0..6 {
            let (col, row) = (index % 3, index / 3);
            let area = Area {
                x: col as f32 * (base.w + 40.0),
                y: row as f32 * (base.h + 40.0),
                ..base
            };
            let open = if index < 4 {
                Open {
                    label: format!("Salida {}", index + 1),
                    program: powershell(),
                    args: vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into(), flood.into()],
                    cwd: None,
                    agent: None,
                }
            } else {
                Open::shell(None)
            };
            self.open_at(open, area, cx);
        }
        self.fit_pending = true;
    }

    fn card(&self, id: u64) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == id)
    }

    fn card_mut(&mut self, id: u64) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.id == id)
    }

    fn raise(&mut self, id: u64) {
        if let Some(index) = self.cards.iter().position(|c| c.id == id) {
            let card = self.cards.remove(index);
            self.cards.push(card);
        }
        self.focused = Some(id);
    }

    /// Mueve la cámara lo justo para que la tarjeta se vea.
    fn reveal(&mut self, id: u64) {
        let Some(card) = self.card(id) else {
            return;
        };
        let bounds = self.camera.area(&card.area);
        let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let (vw, vh) = self.viewport;
        if left < 0.0 || top < TOOLBAR_H || left + w > vw || top + h > vh {
            if w > vw || h > vh - TOOLBAR_H {
                self.fit();
            } else {
                self.camera.x += (vw - w) / 2.0 - left;
                self.camera.y += (vh + TOOLBAR_H - h) / 2.0 - top;
            }
        }
    }

    /// Todas las tarjetas a la vista.
    fn fit(&mut self) {
        if self.cards.is_empty() {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for card in &self.cards {
            x0 = x0.min(card.area.x);
            y0 = y0.min(card.area.y);
            x1 = x1.max(card.area.x + card.area.w);
            y1 = y1.max(card.area.y + card.area.h);
        }
        let (vw, vh) = (self.viewport.0 - 60.0, self.viewport.1 - TOOLBAR_H - 60.0);
        let zoom = (vw / (x1 - x0)).min(vh / (y1 - y0)).clamp(MIN_ZOOM, 1.0);
        self.camera = Camera {
            zoom,
            x: (self.viewport.0 - (x1 - x0) * zoom) / 2.0 - x0 * zoom,
            y: TOOLBAR_H + (self.viewport.1 - TOOLBAR_H - (y1 - y0) * zoom) / 2.0 - y0 * zoom,
        };
        self.moved();
    }

    fn moved(&mut self) {
        self.moving_until = Instant::now() + Duration::from_millis(180);
    }

    fn close(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(token) = self.card(id).and_then(|c| c.token.clone()) {
            crate::agent_prompts::unregister(&token);
        }
        self.cards.retain(|c| c.id != id);
        if self.focused == Some(id) {
            self.focused = self.cards.last().map(|c| c.id);
        }
        cx.notify();
    }

    // --- Mouse ---------------------------------------------------------------

    /// La tarjeta de más arriba bajo el punto de pantalla.
    fn card_at(&self, p: (f32, f32)) -> Option<&Card> {
        let board = self.camera.to_board(p);
        self.cards.iter().rev().find(|c| c.area.contains(board))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let p = (f32::from(event.position.x), f32::from(event.position.y));
        if p.1 < TOOLBAR_H {
            return;
        }
        window.focus(&self.focus);
        let board = self.camera.to_board(p);
        if let Some(card) = self.card_at(p) {
            let (id, area) = (card.id, card.area);
            self.raise(id);
            let local = (board.0 - area.x, board.1 - area.y);
            if local.1 <= HEADER && local.0 >= area.w - HEADER {
                self.close(id, cx);
                return;
            }
            if local.0 >= area.w - GRIP && local.1 >= area.h - GRIP {
                self.drag = Some(Drag::Resize { id, from: p, area });
            } else if local.1 <= HEADER || self.camera.zoom < LIVE_ZOOM {
                // De lejos la tarjeta entera se arrastra; doble clic acerca.
                if event.click_count == 2 {
                    self.zoom_to(id);
                } else {
                    self.drag = Some(Drag::Move { id, from: p, area });
                }
            }
        } else if event.click_count == 2 {
            self.fit();
        } else {
            self.drag = Some(Drag::Pan {
                from: p,
                camera: self.camera,
            });
        }
        cx.notify();
    }

    /// Acerca la cámara a una tarjeta, a zoom 1.
    fn zoom_to(&mut self, id: u64) {
        let Some(card) = self.card(id) else {
            return;
        };
        let area = card.area;
        self.camera.zoom = 1.0;
        self.camera.x = (self.viewport.0 - area.w) / 2.0 - area.x;
        self.camera.y = (self.viewport.1 + TOOLBAR_H - area.h) / 2.0 - area.y;
        self.moved();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            return self.release(cx);
        }
        let p = (f32::from(event.position.x), f32::from(event.position.y));
        let zoom = self.camera.zoom;
        match drag {
            Drag::Pan { from, camera } => {
                self.camera.x = camera.x + p.0 - from.0;
                self.camera.y = camera.y + p.1 - from.1;
                self.moved();
            }
            Drag::Move { id, from, area } => {
                if let Some(card) = self.card_mut(id) {
                    card.area.x = area.x + (p.0 - from.0) / zoom;
                    card.area.y = area.y + (p.1 - from.1) / zoom;
                }
            }
            Drag::Resize { id, from, area } => {
                let min = self.area_for(40, 8);
                if let Some(card) = self.card_mut(id) {
                    card.area.w = (area.w + (p.0 - from.0) / zoom).max(min.w);
                    card.area.h = (area.h + (p.1 - from.1) / zoom).max(min.h);
                }
            }
        }
        cx.notify();
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        if let Some(Drag::Resize { id, .. }) = self.drag.take() {
            // El PTY cambia de tamaño al soltar, no en cada movimiento: cada
            // cambio hace que la TUI se redibuje entera.
            let cell = self.cell();
            if let Some(index) = self.cards.iter().position(|c| c.id == id) {
                let grid = self.grid_for(&self.cards[index].area);
                self.cards[index]
                    .console
                    .resize(grid, (cell.w.round() as u16, cell.h as u16));
            }
        }
        cx.notify();
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let p = (f32::from(event.position.x), f32::from(event.position.y));
        let delta = event.delta.pixel_delta(px(self.cell().h * self.camera.zoom));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control {
            let factor = (dy / 400.0).exp();
            self.camera.zoom_at(self.camera.zoom * factor, p);
            self.moved();
        } else if let Some(card) = self
            .card_at(p)
            .filter(|_| self.camera.zoom >= LIVE_ZOOM)
        {
            let lines = (dy / (self.cell().h * self.camera.zoom)).round() as i32;
            if lines != 0 {
                card.console.scroll(lines);
            }
        } else {
            self.camera.x += dx;
            self.camera.y += dy;
            self.moved();
        }
        cx.notify();
    }

    // --- Teclado -------------------------------------------------------------

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        // Atajos del espacio (los mismos de la pizarra de Atic).
        if m.control && !m.alt {
            match ks.key.as_str() {
                "0" => return self.fit_notify(cx),
                "=" | "+" => return self.zoom_keys(1.2, cx),
                "-" => return self.zoom_keys(1.0 / 1.2, cx),
                "w" if m.shift => {
                    if let Some(id) = self.focused {
                        self.close(id, cx);
                    }
                    return;
                }
                "t" if m.shift => {
                    self.open(Open::shell(None), cx);
                    return;
                }
                "tab" => {
                    self.cycle(if m.shift { -1 } else { 1 }, cx);
                    return;
                }
                "v" => {
                    if let (Some(text), Some(card)) = (
                        cx.read_from_clipboard().and_then(|item| item.text()),
                        self.focused.and_then(|id| self.card(id)),
                    ) {
                        card.console.paste(&text);
                    }
                    return;
                }
                _ => {}
            }
        }
        let Some(card) = self.focused.and_then(|id| self.card(id)) else {
            return;
        };
        let app_cursor = card.console.term.lock().mode().contains(TermMode::APP_CURSOR);
        if let Some(bytes) = console::key_bytes(ks, app_cursor) {
            // Escribir vuelve al final si se estaba mirando el historial.
            card.console.scroll(i32::MIN / 2);
            card.console.write(bytes);
            cx.stop_propagation();
        }
    }

    fn fit_notify(&mut self, cx: &mut Context<Self>) {
        self.fit();
        cx.notify();
    }

    fn zoom_keys(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center = (self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0);
        self.camera.zoom_at(self.camera.zoom * factor, center);
        self.moved();
        cx.notify();
    }

    fn cycle(&mut self, step: i32, cx: &mut Context<Self>) {
        if self.cards.is_empty() {
            return;
        }
        let mut ids: Vec<u64> = self.cards.iter().map(|c| c.id).collect();
        ids.sort();
        let at = self
            .focused
            .and_then(|id| ids.iter().position(|&i| i == id))
            .unwrap_or(0) as i32;
        let next = ids[(at + step).rem_euclid(ids.len() as i32) as usize];
        self.raise(next);
        self.reveal(next);
        cx.notify();
    }

    // --- Medición --------------------------------------------------------------

    /// Mueve la cámara según la fase y junta los intervalos entre cuadros.
    fn bench_step(&mut self, now: Instant) -> bool {
        let Some(bench) = self.bench.as_mut() else {
            return false;
        };
        if let Some(last) = bench.last {
            bench.gaps.push(now.duration_since(last).as_secs_f32() * 1000.0);
            bench.paints.push(self.paint_ms.get());
            bench.builds.push(self.build_ms);
        }
        bench.last = Some(now);
        let mut t = now.duration_since(bench.started).as_secs_f32();
        let mut phase = 0;
        while phase < BENCH_PHASES.len() && t > BENCH_PHASES[phase].1 {
            t -= BENCH_PHASES[phase].1;
            phase += 1;
        }
        if phase != bench.phase {
            report(BENCH_PHASES[bench.phase].0, &bench.gaps, &bench.paints, &bench.builds);
            bench.gaps.clear();
            bench.paints.clear();
            bench.builds.clear();
            bench.phase = phase;
            bench.last = None;
        }
        if phase >= BENCH_PHASES.len() {
            self.bench = None;
            eprintln!("espacio: medición terminada");
            return false;
        }
        if bench.phase == 0 && bench.gaps.len() == 1 {
            bench.base = self.camera;
        }
        let base = bench.base;
        let center = (self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0);
        match BENCH_PHASES[phase].0 {
            "pan" => {
                self.camera = base;
                self.camera.x += (t * 1.6).sin() * 300.0;
                self.camera.y += (t * 1.1).sin() * 120.0;
            }
            "zoom" => {
                self.camera = base;
                self.camera.zoom_at(base.zoom * (1.0 + 0.45 * (t * 1.5).sin()), center);
            }
            "zoom semántico" => {
                self.camera = base;
                // Cruza el umbral: de terminal en vivo a tarjeta y de vuelta.
                let zoom = 0.62 + 0.38 * (t * 1.2).sin();
                self.camera.zoom_at(zoom, center);
            }
            _ => {}
        }
        self.moved();
        true
    }

    // --- Dibujo ---------------------------------------------------------------

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(28.))
                .px(px(13.))
                .flex()
                .items_center()
                .rounded(px(14.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(console::hsla(TEXT))
                .bg(console::hsla(0x232321))
                .hover(|el| el.bg(console::hsla(0x2e2e2b)))
                .cursor_pointer()
                .child(label)
        };
        let working = self
            .cards
            .iter()
            .filter(|c| !c.console.exited() && c.console.quiet_for() < WORKING_FOR)
            .count();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TOOLBAR_H))
            .pl(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(console::hsla(0x151514))
            .font_family("Segoe UI")
            .child(chrome::logo(28.0))
            .child(div().mx(px(4.)).child(mando::tabs(self.view, cx)))
            .child(button("space-claude", "+ Claude").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open(Open::agent("claude", "Claude Code", "claude", None), cx)
            })))
            .child(button("space-codex", "+ Codex").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open(Open::agent("codex", "Codex", "codex", None), cx)
            })))
            .child(button("space-shell", "+ PowerShell").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open(Open::shell(None), cx)
            })))
            .child(folders::bar(self, cx))
            .child(chrome::drag(TOOLBAR_H))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(console::hsla(MUTED))
                    .child(format!(
                        "{} · {working} trabajando",
                        mando::plural(self.cards.len(), "consola", "consolas")
                    )),
            )
            .child(button("space-fit", "Ajustar").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.fit_notify(cx)
            })))
            .child(
                div()
                    .w(px(48.))
                    .mr(px(8.))
                    .text_size(px(11.))
                    .text_color(console::hsla(MUTED))
                    .child(format!("{:.0} %", self.camera.zoom * 100.0)),
            )
            .child(chrome::controls(self.mando.is_maximized(), TOOLBAR_H))
    }

    /// Lo que se dibuja de cada tarjeta, armado antes de pintar para no tener
    /// el `Term` bloqueado mientras se compone el texto.
    fn snapshot(&self, now: Instant) -> Vec<CardDraw> {
        let viewport = Bounds::new(
            point(px(0.), px(TOOLBAR_H)),
            size(px(self.viewport.0), px(self.viewport.1 - TOOLBAR_H)),
        );
        let live = self.camera.zoom >= LIVE_ZOOM;
        let moving = now < self.moving_until || self.bench.is_some();
        // En movimiento la letra va en escalones del 6 %: unos pocos tamaños
        // que GPUI rasteriza una vez y reusa. Quieta, el tamaño exacto.
        let exact = FONT_SIZE * self.camera.zoom;
        let font_size = if moving { snap_size(exact) } else { exact };
        self.cards
            .iter()
            .filter_map(|card| {
                let bounds = self.camera.area(&card.area);
                if !bounds.intersects(&viewport) {
                    return None;
                }
                card.console.take_dirty();
                let quiet = card.console.quiet_for();
                let status = if card.console.exited() {
                    Status::Exited
                } else if quiet < WORKING_FOR {
                    Status::Working
                } else {
                    Status::Ready
                };
                // Lo mismo que dice la carta del Mando: la tarea que informa el
                // agente o la carpeta donde trabaja, no la ruta del shell.
                let title = mando::card_sub(card);
                let body = if live {
                    Body::Grid(self.grid_rows(card))
                } else {
                    Body::Summary(card.console.last_lines(4))
                };
                Some(CardDraw {
                    bounds,
                    zoom: self.camera.zoom,
                    font_size,
                    label: card.label.clone(),
                    title: title.into(),
                    agent: card.agent,
                    focused: self.focused == Some(card.id),
                    status,
                    body,
                })
            })
            .collect()
    }

    /// Las filas visibles de la terminal: texto, tramos de estilo, fondos y
    /// el cursor.
    fn grid_rows(&self, card: &Card) -> Grid {
        let term = card.console.term.lock();
        let content = term.renderable_content();
        let colors = content.colors;
        let rows = term.grid().screen_lines();
        let cols = term.grid().columns();
        let offset = content.display_offset as i32;
        let mut out: Vec<Row> = (0..rows).map(|_| Row::default()).collect();
        for indexed in content.display_iter {
            let row = (indexed.point.line.0 + offset) as usize;
            let col = indexed.point.column.0;
            let Some(slot) = out.get_mut(row) else {
                continue;
            };
            let cell = indexed.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let mut fg = console::resolve(cell.fg, colors);
            let mut bg = console::resolve(cell.bg, colors);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.intersects(Flags::DIM) {
                fg = console::dim(fg);
            }
            if bg != console::BACKGROUND {
                let wide = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
                match slot.backgrounds.last_mut() {
                    Some((start, len, color)) if *color == bg && *start + *len == col => {
                        *len += wide
                    }
                    _ => slot.backgrounds.push((col, wide, bg)),
                }
            }
            if cell.flags.intersects(Flags::ALL_UNDERLINES) {
                slot.underlines.push((col, fg));
            }
            {
                while slot.cols < col {
                    slot.text.push(' ');
                    extend_run(slot, 1, console::FOREGROUND, 0, &self.fonts);
                    slot.cols += 1;
                }
                let c = if cell.flags.contains(Flags::HIDDEN) || cell.c == '\0' { ' ' } else { cell.c };
                let before = slot.text.len();
                slot.text.push(c);
                let style = cell.flags.contains(Flags::BOLD) as u8
                    | (cell.flags.contains(Flags::ITALIC) as u8) << 1;
                extend_run(slot, slot.text.len() - before, fg, style, &self.fonts);
                slot.cols += 1;
                if c != ' ' {
                    slot.ink.push((col, fg));
                }
            }
        }
        let cursor = (content.cursor.shape != CursorShape::Hidden)
            .then(|| {
                let row = content.cursor.point.line.0 + offset;
                (row >= 0 && (row as usize) < rows)
                    .then_some((row as usize, content.cursor.point.column.0.min(cols - 1)))
            })
            .flatten();
        drop(term);
        Grid {
            rows: out,
            cursor,
            cols,
        }
    }
}

fn p95(values: &[f32]) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted
        .get(((sorted.len() as f32 * 0.95) as usize).min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

fn report(phase: &str, gaps: &[f32], paints: &[f32], builds: &[f32]) {
    if gaps.is_empty() {
        return;
    }
    let mut sorted = gaps.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let p95 = sorted[((sorted.len() as f32 * 0.95) as usize).min(sorted.len() - 1)];
    let total: f32 = gaps.iter().sum();
    let mut paint = paints.to_vec();
    paint.sort_by(|a, b| a.total_cmp(b));
    let paint_p95 = paint[((paint.len() as f32 * 0.95) as usize).min(paint.len() - 1)];
    eprintln!(
        "espacio [{phase}]: {:.1} cuadros/s, {:.0} % de intervalos > 12,5 ms, p95 {:.1} ms, peor {:.1} ms, pintar p95 {:.2} ms, armar p95 {:.2} ms",
        gaps.len() as f32 / (total / 1000.0),
        gaps.iter().filter(|&&g| g > 12.5).count() as f32 * 100.0 / gaps.len() as f32,
        p95,
        sorted.last().copied().unwrap_or(0.0),
        paint_p95,
        self::p95(builds)
    );
}

/// Agrega `len` bytes al último tramo si tiene el mismo estilo.
fn extend_run(slot: &mut Row, len: usize, fg: u32, style: u8, fonts: &[Font; 4]) {
    if let Some((last_fg, last_style)) = slot.run_key {
        if last_fg == fg && last_style == style {
            if let Some(run) = slot.runs.last_mut() {
                run.len += len;
                return;
            }
        }
    }
    slot.runs.push(TextRun {
        len,
        font: fonts[style as usize & 3].clone(),
        color: console::hsla(fg),
        background_color: None,
        underline: None,
        strikethrough: None,
    });
    slot.run_key = Some((fg, style));
}

#[derive(Default)]
struct Row {
    /// El renglón entero, una letra por columna, y sus tramos de estilo.
    text: String,
    runs: Vec<TextRun>,
    run_key: Option<(u32, u8)>,
    cols: usize,
    /// Las columnas con letra y su color: la silueta del renglón.
    ink: Vec<(usize, u32)>,
    /// (columna, celdas, color) de los fondos que no son el de la terminal.
    backgrounds: Vec<(usize, usize, u32)>,
    underlines: Vec<(usize, u32)>,
}

/// Las cuatro variantes de la fuente: normal, negrita, cursiva y ambas.
fn fonts() -> [Font; 4] {
    let base = font();
    let variant = |bold: bool, italic: bool| {
        let mut font = base.clone();
        if bold {
            font.weight = FontWeight::BOLD;
        }
        if italic {
            font.style = FontStyle::Italic;
        }
        font
    };
    [
        variant(false, false),
        variant(true, false),
        variant(false, true),
        variant(true, true),
    ]
}

struct Grid {
    rows: Vec<Row>,
    cursor: Option<(usize, usize)>,
    cols: usize,
}

enum Body {
    Grid(Grid),
    Summary(Vec<String>),
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Working,
    Ready,
    Exited,
}

struct CardDraw {
    bounds: Bounds<Pixels>,
    zoom: f32,
    font_size: f32,
    label: SharedString,
    title: SharedString,
    agent: Option<&'static str>,
    focused: bool,
    status: Status,
    body: Body,
}

fn shadow(alpha: f32, zoom: f32) -> BoxShadow {
    BoxShadow {
        color: gpui::black().opacity(alpha),
        offset: point(px(0.), px(8. * zoom)),
        blur_radius: px(24. * zoom),
        spread_radius: px(0.),
    }
}

impl CardDraw {
    fn paint(self, cell: Cell, window: &mut Window, cx: &mut App) {
        let z = self.zoom;
        let b = self.bounds;
        let radius = px(RADIUS * z);
        // Plano y sin bordes: la tarjeta se separa del plano por su color. La
        // sombra se queda, suave: las tarjetas se montan unas sobre otras y,
        // sin ella, dos consolas del mismo fondo se confundirían.
        window.paint_shadows(b, Corners::all(radius), &[shadow(0.35, z)]);
        window.paint_quad(quad(
            b,
            Corners::all(radius),
            console::hsla(CARD),
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
        // Encabezado: estado, nombre, título que puso la app y cerrar. Las
        // esquinas de arriba son las de la tarjeta, con el mismo radio y sin
        // margen: así no sobresale nada. La enfocada lo lleva más claro.
        let header_h = HEADER * z;
        let header = Bounds::new(b.origin, size(b.size.width, px(header_h)));
        window.paint_quad(quad(
            header,
            Corners {
                top_left: radius,
                top_right: radius,
                bottom_left: px(0.),
                bottom_right: px(0.),
            },
            console::hsla(if self.focused { HEADER_ON } else { HEADER_BG }),
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
        let dot = match self.status {
            Status::Working => WORKING,
            Status::Ready => READY,
            Status::Exited => FAINT,
        };
        let cy = f32::from(b.origin.y) + header_h / 2.0;
        let x0 = f32::from(b.origin.x);
        let dot_r = 3.5 * z;
        window.paint_quad(quad(
            Bounds::new(point(px(x0 + 12. * z - dot_r), px(cy - dot_r)), size(px(dot_r * 2.), px(dot_r * 2.))),
            Corners::all(px(dot_r)),
            console::hsla(dot),
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
        let label_size = (12.0 * z).max(1.0);
        if label_size >= 4.0 {
            let mut text = self.label.to_string();
            if !self.title.is_empty() {
                text = format!("{text}  ·  {}", self.title);
            }
            if self.status == Status::Exited {
                text.push_str("  ·  terminó");
            }
            paint_text(
                &text,
                label_size,
                if self.focused { TEXT } else { MUTED },
                (x0 + 24. * z, cy - label_size * 0.65),
                Some(f32::from(b.size.width) - 60. * z),
                window,
                cx,
            );
            // La × de cerrar.
            paint_text(
                "×",
                label_size * 1.3,
                FAINT,
                (f32::from(b.right()) - 22. * z, cy - label_size * 0.85),
                None,
                window,
                cx,
            );
        }
        let content = Bounds::new(
            point(b.origin.x, b.origin.y + px(header_h)),
            size(b.size.width, b.size.height - px(header_h)),
        );
        let body = self.body;
        let font_size = self.font_size;
        let focused = self.focused;
        let agent = self.agent;
        window.with_content_mask(Some(ContentMask { bounds: content }), |window| match body {
            Body::Grid(grid) => paint_grid(&grid, content, cell, z, font_size, focused, window, cx),
            Body::Summary(lines) => paint_summary(&lines, content, z, agent, window, cx),
        });
        // La esquina para cambiar el tamaño.
        let grip = GRIP * z * 0.6;
        let corner = (f32::from(b.right()) - 5. * z, f32::from(b.bottom()) - 5. * z);
        for i in 0..2 {
            let off = i as f32 * grip * 0.45;
            let mut path = gpui::PathBuilder::stroke(px((1.2 * z).max(0.6)));
            path.move_to(point(px(corner.0 - grip + off), px(corner.1)));
            path.line_to(point(px(corner.0), px(corner.1 - grip + off)));
            if let Ok(path) = path.build() {
                window.paint_path(path, console::hsla(FAINT));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_grid(
    grid: &Grid,
    content: Bounds<Pixels>,
    cell: Cell,
    z: f32,
    font_size: f32,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let cw = cell.w * z;
    let lh = cell.h * z;
    let origin = (f32::from(content.origin.x) + PAD * z, f32::from(content.origin.y) + 2. * z);
    let mask = window.content_mask().bounds;
    let baseline = lh * 0.78;
    let silhouette = z < TEXT_ZOOM;
    for (index, row) in grid.rows.iter().enumerate() {
        let y = origin.1 + index as f32 * lh;
        // Filas fuera de la vista (tarjeta cortada por el borde): ni se tocan.
        if y + lh < f32::from(mask.origin.y) || y > f32::from(mask.bottom()) {
            continue;
        }
        for &(col, len, color) in &row.backgrounds {
            window.paint_quad(gpui::fill(
                Bounds::new(point(px(origin.0 + col as f32 * cw), px(y)), size(px(len as f32 * cw), px(lh))),
                console::hsla(color),
            ));
        }
        if row.text.trim_end().is_empty() {
            continue;
        }
        if silhouette {
            // De lejos: cada tramo con letra es una barra de su color. Se ve
            // la salida moverse sin dibujar texto ilegible.
            let mut start: Option<(usize, usize, u32)> = None;
            for &(col, color) in row.ink.iter().chain(std::iter::once(&(usize::MAX, 0))) {
                match start {
                    Some((from, to, c)) if c == color && to + 1 >= col => start = Some((from, col, c)),
                    _ => {
                        if let Some((from, to, c)) = start {
                            window.paint_quad(gpui::fill(
                                Bounds::new(
                                    point(px(origin.0 + from as f32 * cw), px(y + lh * 0.3)),
                                    size(px((to - from + 1) as f32 * cw), px(lh * 0.45)),
                                ),
                                console::hsla(c).opacity(0.55),
                            ));
                        }
                        start = (col != usize::MAX).then_some((col, col, color));
                    }
                }
            }
            continue;
        }
        // El renglón compuesto de una vez: GPUI lo reusa en los cuadros
        // siguientes si no cambió. `force_width` pone cada letra en su
        // columna aunque venga de otra fuente.
        let line = window.text_system().shape_line(
            row.text.clone().into(),
            px(font_size),
            &row.runs,
            Some(px(cw)),
        );
        let _ = line.paint(point(px(origin.0), px(y)), px(lh), window, cx);
        for &(col, color) in &row.underlines {
            window.paint_quad(gpui::fill(
                Bounds::new(
                    point(px(origin.0 + col as f32 * cw), px(y + baseline + 2. * z)),
                    size(px(cw), px(z.max(1.0))),
                ),
                console::hsla(color),
            ));
        }
    }
    if let Some((row, col)) = grid.cursor {
        let bounds = Bounds::new(
            point(px(origin.0 + col.min(grid.cols) as f32 * cw), px(origin.1 + row as f32 * lh)),
            size(px(cw), px(lh)),
        );
        if focused {
            window.paint_quad(gpui::fill(bounds, console::hsla(console::FOREGROUND).opacity(0.75)));
        } else {
            window.paint_quad(gpui::outline(bounds, console::hsla(MUTED), gpui::BorderStyle::Solid));
        }
    }
}

/// De lejos: el agente y las últimas líneas, en grande, para reconocerla.
fn paint_summary(
    lines: &[String],
    content: Bounds<Pixels>,
    z: f32,
    agent: Option<&'static str>,
    window: &mut Window,
    cx: &mut App,
) {
    let x = f32::from(content.origin.x) + 16. * z;
    let mut y = f32::from(content.origin.y) + 16. * z;
    let name = agent.unwrap_or("consola");
    let big = 34. * z;
    if big >= 3.0 {
        paint_text(name, big, TEXT, (x, y), None, window, cx);
        y += big * 1.6;
    }
    let small = 22. * z;
    if small >= 3.0 {
        for line in lines {
            let line: String = line.trim().chars().take(70).collect();
            paint_text(&line, small, MUTED, (x, y), Some(f32::from(content.size.width) - 32. * z), window, cx);
            y += small * 1.45;
        }
    }
}

fn paint_text(
    text: &str,
    font_size: f32,
    color: u32,
    (x, y): (f32, f32),
    max_width: Option<f32>,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let mut text: String = text.to_string();
    // Recorte simple por ancho estimado: el título no debe pisar la ×.
    if let Some(max) = max_width {
        let fits = (max / (font_size * 0.56)).max(1.0) as usize;
        if text.chars().count() > fits {
            text = text.chars().take(fits.saturating_sub(1)).collect::<String>() + "…";
        }
    }
    let run = TextRun {
        len: text.len(),
        font: gpui::font("Segoe UI"),
        color: console::hsla(color),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.into(), px(font_size), &[run], None);
    let _ = line.paint(point(px(x), px(y)), px(font_size * 1.3), window, cx);
}

/// La grilla del fondo, que se mueve con la cámara: unas pocas líneas
/// tenues. Con puntos eran miles de quads por cuadro (el mismo costo que el
/// diagnóstico encontró en el fondo de la pizarra de Atic).
fn paint_backdrop(camera: Camera, viewport: (f32, f32), window: &mut Window) {
    let mut step = 120.0 * camera.zoom;
    while step < 60.0 {
        step *= 2.0;
    }
    let color = console::hsla(0x1b1b19);
    let mut x = camera.x.rem_euclid(step);
    while x < viewport.0 {
        window.paint_quad(gpui::fill(
            Bounds::new(point(px(x), px(TOOLBAR_H)), size(px(1.), px(viewport.1 - TOOLBAR_H))),
            color,
        ));
        x += step;
    }
    let mut y = (camera.y - TOOLBAR_H).rem_euclid(step) + TOOLBAR_H;
    while y < viewport.1 {
        window.paint_quad(gpui::fill(
            Bounds::new(point(px(0.), px(y)), size(px(viewport.0), px(1.))),
            color,
        ));
        y += step;
    }
}

impl Render for SpaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Instant::now();
        let viewport = window.viewport_size();
        self.viewport = (f32::from(viewport.width), f32::from(viewport.height));
        // La bandeja se entera de lo que pasa con cualquiera de las dos vistas.
        self.tick_mando(window, started);
        if self.view == mando::View::Mando {
            return mando::render(self, window, cx);
        }
        if std::mem::take(&mut self.fit_pending) {
            self.fit();
        }
        let now = Instant::now();
        if self.bench_step(now) || now < self.moving_until {
            window.request_animation_frame();
        }
        let built = Instant::now();
        let cards = self.snapshot(now);
        self.build_ms = built.elapsed().as_secs_f32() * 1000.0;
        let camera = self.camera;
        let cell = self.cell();
        let viewport = self.viewport;
        let paint_ms = self.paint_ms.clone();
        let empty = self.cards.is_empty();
        self.meter.frame(started);

        div()
            .id("space")
            .key_context("Space")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(mando::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, cx| view.release(cx)))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .size_full()
            .relative()
            .bg(console::hsla(BG))
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| {
                        let started = Instant::now();
                        paint_backdrop(camera, viewport, window);
                        for card in cards {
                            card.paint(cell, window, cx);
                        }
                        paint_ms.set(started.elapsed().as_secs_f32() * 1000.0);
                    },
                )
                .size_full(),
            )
            .when(empty, |el| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family("Segoe UI")
                        .text_size(px(13.))
                        .text_color(console::hsla(MUTED))
                        .child("Abre una consola desde la barra o desde Agentes en el notch."),
                )
            })
            .child(input::layer(cx.weak_entity(), self.focus.clone()))
            .child(self.toolbar(cx))
            .into_any_element()
    }
}
