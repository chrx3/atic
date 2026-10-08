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

mod changes;
mod explorer;
pub(crate) mod chrome;
pub(crate) mod console;
mod identity;
mod input;
mod mando;
mod panes;
mod persist;
mod picker;
mod viewer;
mod workspaces;
mod zones;

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
    ContentMask, Corners, FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla, KeyDownEvent,
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
/// En el encabezado de un archivo de la pizarra, lo que ocupa «Archivo» /
/// «Cambios», antes de la ×.
const DOC_TOGGLE: f32 = 78.0;
/// Desde este zoom, una consola con archivos cambiados muestra el botón que
/// los abre; su ancho en pantalla.
const CHIP_ZOOM: f32 = 0.45;
const CHIP_W: f32 = 96.0;
/// Zoom semántico. Desde `TEXT_ZOOM` la terminal es texto (letra de 8,5 px
/// o más); entre `LIVE_ZOOM` y `TEXT_ZOOM`, la silueta de su salida en vivo;
/// por debajo, una tarjeta con el agente y sus últimas líneas.
const TEXT_ZOOM: f32 = 0.65;
const LIVE_ZOOM: f32 = 0.3;
const MIN_ZOOM: f32 = 0.12;
const MAX_ZOOM: f32 = 2.0;
/// Sin salida por este tiempo, la consola está «lista».
const WORKING_FOR: Duration = Duration::from_millis(1500);
/// Cada cuánto se miran los archivos modificados de los agentes.
const CHANGES_EVERY: Duration = Duration::from_secs(3);
/// Es también la barra de la ventana (la nativa se esconde): mide lo mismo que
/// la del Mando.
const TOOLBAR_H: f32 = 48.0;
const FONT_FAMILY: &str = "Cascadia Mono";

const BG: u32 = crate::theme::WINDOW;
const CARD: u32 = console::BACKGROUND;
const HEADER_BG: u32 = crate::theme::SURFACE;
const TEXT: u32 = crate::theme::TEXT;
const MUTED: u32 = crate::theme::MUTED;
const FAINT: u32 = 0x5a5a54;
/// El encabezado de la tarjeta enfocada: se distingue por ser más claro, sin marco.
const HEADER_ON: u32 = crate::theme::SURFACE_ON;
/// El fondo de la zona de un espacio en la pizarra: apenas más claro que el plano.
const ZONE: u32 = 0x131312;
const WORKING: u32 = crate::theme::AMBER;
const READY: u32 = crate::theme::GREEN;

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
    /// `cwd` y las otras carpetas del espacio que recibió al abrirse.
    dirs: Vec<PathBuf>,
    /// El espacio de trabajo donde se abrió (`workspaces`).
    workspace: Option<u64>,
    /// Su nombre y su color, para distinguirla de las otras (`identity`).
    name: &'static str,
    color: u32,
    /// Los archivos que cambiaron en sus carpetas desde que se abrió.
    changes: changes::Tracked,
    /// La marca de una consola de agente con los hooks de Atic
    /// (`agent_prompts`): sus permisos se contestan desde la bandeja.
    token: Option<String>,
}

/// Un movimiento suave de la cámara de un lugar a otro.
struct Flight {
    from: Camera,
    to: Camera,
    start: Instant,
}

/// Lo que dura un vuelo de la cámara: el ritmo de las animaciones de Atic.
const FLIGHT: Duration = Duration::from_millis(620);

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
    /// Los espacios de trabajo y el activo (`workspaces.rs`).
    spaces: workspaces::Workspaces,
    /// El espacio donde se abre la próxima consola, si se pidió uno.
    opening_in: Option<u64>,
    /// Los agentes de `agents::AGENTS` instalados en este equipo.
    available: Vec<usize>,
    /// El selector de carpetas, si está abierto (`picker.rs`).
    picker: Option<picker::Picker>,
    /// Los archivos abiertos en paneles del Mando (`viewer.rs`).
    docs: Vec<viewer::Doc>,
    /// Se está arrastrando una zona de la pizarra: desde dónde y dónde
    /// estaban sus consolas.
    zone_drag: Option<((f32, f32), Vec<(u64, Area)>)>,
    /// La cámara va camino a otro lugar (`fly`).
    flight: Option<Flight>,
    /// Un clic que empezó en una tarjeta de lejos: si no se arrastra, la
    /// cámara vuela a ella al soltar.
    tap: Option<(u64, (f32, f32))>,
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
        // Los archivos que tocó cada agente: `git status` en segundo plano.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(CHANGES_EVERY).await;
            let Ok(jobs) = this.update(cx, |view, _| {
                view.cards.iter().filter(|c| !c.dirs.is_empty()).map(|c| (c.id, c.dirs.clone())).collect::<Vec<_>>()
            }) else {
                break;
            };
            let scans = cx
                .background_spawn(async move {
                    jobs.into_iter().map(|(id, dirs)| (id, changes::scan(&dirs))).collect::<Vec<_>>()
                })
                .await;
            let updated = this.update(cx, |view, cx| {
                for (id, scan) in scans {
                    if let Some(card) = view.card_mut(id) {
                        let cwd = card.cwd.clone().unwrap_or_default();
                        card.changes.update(scan, &cwd);
                    }
                }
                cx.notify();
            });
            if updated.is_err() {
                break;
            }
        })
        .detach();
        // Qué agentes hay instalados, para ofrecer solo esos.
        cx.spawn(async move |this, cx| {
            let available = cx
                .background_spawn(async {
                    (0..crate::agents::AGENTS.len())
                        .filter(|&index| crate::agents::on_path(crate::agents::AGENTS[index].cli))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.available = available;
                cx.notify();
            });
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
            spaces: workspaces::Workspaces::load(),
            opening_in: None,
            available: Vec::new(),
            picker: None,
            docs: Vec::new(),
            zone_drag: None,
            flight: None,
            tap: None,
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
        // En la pizarra va dentro de la zona de su espacio (`zones`); con el
        // plano vacío, al centro de lo que se ve.
        let placed = self.board_items();
        let workspace = self.workspace_for(&open);
        if let Some((x, y)) = zones::place(workspace, &placed) {
            area.x = x;
            area.y = y;
        } else {
            let center = self.camera.to_board((self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0));
            area.x = center.0 - area.w / 2.0;
            area.y = center.1 - area.h / 2.0;
        }
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

    /// El espacio donde se abre: el que se pidió con `opening_in`, el que
    /// tiene la carpeta pedida o, sin carpeta, el activo.
    fn workspace_for(&self, open: &Open) -> Option<u64> {
        match (self.opening_in, &open.cwd) {
            (Some(id), _) => Some(id),
            (None, Some(cwd)) => self.spaces.find_for(cwd),
            (None, None) => self.spaces.active_id(),
        }
    }

    fn open_at(&mut self, open: Open, area: Area, cx: &mut Context<Self>) {
        let cell = self.cell();
        let workspace = self.workspace_for(&open).and_then(|id| self.spaces.get(id)).cloned();
        self.opening_in = None;
        // Sin carpeta pedida: la principal del espacio, la de quien lo abrió o
        // la del proceso, donde el PTY ya arrancaba.
        let cwd = open
            .cwd
            .or_else(|| workspace.as_ref().and_then(|w| w.main().cloned()))
            .or_else(|| self.default_cwd.clone())
            .or_else(|| std::env::current_dir().ok());
        let mut args = open.args;
        let mut env = Vec::new();
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
        // Un agente en una carpeta del espacio trabaja también en las otras.
        // Va después de los hooks: con comillas en la línea, `prepare` la
        // tomaría por sintaxis de shell y no los pondría.
        let extras = match (&workspace, &cwd) {
            (Some(space), Some(dir)) => space.extras(dir),
            _ => Vec::new(),
        };
        if let (Some(agent), [flag, line]) = (open.agent, args.as_mut_slice()) {
            if flag.eq_ignore_ascii_case("/K") {
                *line = workspaces::with_add_dirs(agent, line, &extras);
            }
        }
        let dirs: Vec<PathBuf> = cwd.iter().cloned().chain(extras).collect();
        let names: Vec<&str> = self.cards.iter().map(|c| c.name).collect();
        let colors: Vec<u32> = self.cards.iter().map(|c| c.color).collect();
        let (name, color) = identity::pick(&names, &colors);
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
                    dirs,
                    workspace: workspace.map(|w| w.id),
                    name,
                    color,
                    changes: changes::Tracked::default(),
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
        if let Some(area) = self.card(id).map(|c| c.area) {
            self.reveal_area(area);
        }
    }

    /// Si el área no se ve entera, la cámara vuela hasta dejarla al centro
    /// (alejándose si no cabe).
    fn reveal_area(&mut self, area: Area) {
        let bounds = self.camera.area(&area);
        let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let (vw, vh) = self.viewport;
        if left < 0.0 || top < TOOLBAR_H || left + w > vw || top + h > vh {
            let zoom = self.camera.zoom.min(self.fit_zoom(area));
            self.fly(self.centered(area, zoom));
        }
    }

    /// «Auto foco»: la cámara vuela a la tarjeta y la deja al centro, a un
    /// tamaño en que se lee.
    fn focus_on(&mut self, id: u64) {
        if let Some(area) = self.card(id).map(|c| c.area) {
            let zoom = self.fit_zoom(area).min(1.0);
            self.fly(self.centered(area, zoom));
        }
    }

    /// El zoom con que el área cabe en lo que se ve, con margen.
    fn fit_zoom(&self, area: Area) -> f32 {
        let (vw, vh) = (self.viewport.0 - 60.0, self.viewport.1 - TOOLBAR_H - 60.0);
        (vw / area.w).min(vh / area.h).clamp(MIN_ZOOM, MAX_ZOOM)
    }

    /// La cámara con el centro del área al centro de lo que se ve.
    fn centered(&self, area: Area, zoom: f32) -> Camera {
        let center = (self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0);
        Camera {
            zoom,
            x: center.0 - (area.x + area.w / 2.0) * zoom,
            y: center.1 - (area.y + area.h / 2.0) * zoom,
        }
    }

    /// Lleva la cámara a `to` en un movimiento suave (ver `step_flight`).
    fn fly(&mut self, to: Camera) {
        self.flight = Some(Flight { from: self.camera, to, start: Instant::now() });
        self.moved();
    }

    /// Avanza el vuelo de la cámara. Dice si sigue en curso.
    ///
    /// El centro de lo que se ve va en línea recta y el zoom cambia en escala
    /// logarítmica: así acercarse y alejarse se sienten parejos. La curva
    /// acelera y frena por igual, sin rebote.
    fn step_flight(&mut self, now: Instant) -> bool {
        let Some(flight) = &self.flight else {
            return false;
        };
        let t = (now.duration_since(flight.start).as_secs_f32() / FLIGHT.as_secs_f32()).min(1.0);
        let eased = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
        let view = (self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0);
        let (a, b) = (flight.from.to_board(view), flight.to.to_board(view));
        let zoom = (flight.from.zoom.ln() + (flight.to.zoom.ln() - flight.from.zoom.ln()) * eased).exp();
        let center = (a.0 + (b.0 - a.0) * eased, a.1 + (b.1 - a.1) * eased);
        self.camera = Camera { zoom, x: view.0 - center.0 * zoom, y: view.1 - center.1 * zoom };
        if t >= 1.0 {
            self.camera = flight.to;
            self.flight = None;
        }
        self.moved();
        true
    }

    /// Todas las tarjetas a la vista, de una vez (al abrir la ventana).
    fn fit(&mut self) {
        if let Some(camera) = self.fit_camera() {
            self.camera = camera;
            self.moved();
        }
    }

    /// La cámara que deja todas las tarjetas a la vista.
    fn fit_camera(&self) -> Option<Camera> {
        if self.cards.is_empty() {
            return None;
        }
        // Con las zonas y sus encabezados: también son parte de lo que se ve.
        let mut areas: Vec<Area> = self.board_items().into_iter().map(|(_, a)| a).collect();
        areas.extend(self.zone_list().into_iter().map(|z| z.area));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for area in &areas {
            x0 = x0.min(area.x);
            y0 = y0.min(area.y);
            x1 = x1.max(area.x + area.w);
            y1 = y1.max(area.y + area.h);
        }
        let all = Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        Some(self.centered(all, self.fit_zoom(all).min(1.0)))
    }

    /// «Ajustar» y Ctrl+0: todo a la vista, con vuelo.
    fn fly_fit(&mut self) {
        if let Some(camera) = self.fit_camera() {
            self.fly(camera);
        }
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
        // Tocar el plano detiene cualquier vuelo de la cámara.
        self.flight = None;
        let board = self.camera.to_board(p);
        // Los archivos van encima de las consolas: se miran primero.
        if let Some((id, area)) = self.doc_at(p) {
            if let Some(index) = self.docs.iter().position(|d| d.id == id) {
                let doc = self.docs.remove(index);
                self.docs.push(doc);
            }
            let local = (board.0 - area.x, board.1 - area.y);
            if local.1 <= HEADER && local.0 >= area.w - HEADER {
                self.docs.retain(|d| d.id != id);
            } else if local.1 <= HEADER && local.0 >= area.w - HEADER - DOC_TOGGLE {
                // «Archivo» / «Cambios».
                if let Some(doc) = self.docs.last_mut().filter(|d| d.diff.is_some()) {
                    doc.show_diff = !doc.show_diff;
                    doc.scroll = 0;
                }
            } else if local.0 >= area.w - GRIP && local.1 >= area.h - GRIP {
                self.drag = Some(Drag::Resize { id, from: p, area });
            } else if local.1 <= HEADER || self.camera.zoom < LIVE_ZOOM {
                self.drag = Some(Drag::Move { id, from: p, area });
            }
            cx.notify();
            return;
        }
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
                // De lejos la tarjeta entera se arrastra; doble clic acerca, y
                // también un clic sin arrastrar si no se alcanza a leer.
                if event.click_count == 2 {
                    self.zoom_to(id);
                } else {
                    self.drag = Some(Drag::Move { id, from: p, area });
                    if self.camera.zoom < TEXT_ZOOM {
                        self.tap = Some((id, p));
                    }
                }
            } else {
                // Clic en la terminal: queda para escribir y, si está a medias
                // fuera de la vista, la cámara la trae.
                self.reveal(id);
            }
        } else if event.click_count == 2 {
            self.fly_fit();
        } else {
            self.drag = Some(Drag::Pan {
                from: p,
                camera: self.camera,
            });
        }
        cx.notify();
    }

    /// Acerca la cámara a una tarjeta, hasta que se lee.
    fn zoom_to(&mut self, id: u64) {
        self.focus_on(id);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((from, start)) = &self.zone_drag {
            if event.pressed_button != Some(MouseButton::Left) {
                return self.release(cx);
            }
            let zoom = self.camera.zoom;
            let dx = (f32::from(event.position.x) - from.0) / zoom;
            let dy = (f32::from(event.position.y) - from.1) / zoom;
            let moves = start.clone();
            for (id, area) in moves {
                if let Some(target) = self.area_mut(id) {
                    target.x = area.x + dx;
                    target.y = area.y + dy;
                }
            }
            cx.notify();
            return;
        }
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            return self.release(cx);
        }
        let p = (f32::from(event.position.x), f32::from(event.position.y));
        // Moverse más que un temblor ya no es un clic: es arrastrar.
        if self.tap.is_some_and(|(_, from)| (p.0 - from.0).abs() + (p.1 - from.1).abs() > 4.0) {
            self.tap = None;
        }
        let zoom = self.camera.zoom;
        match drag {
            Drag::Pan { from, camera } => {
                self.camera.x = camera.x + p.0 - from.0;
                self.camera.y = camera.y + p.1 - from.1;
                self.moved();
            }
            Drag::Move { id, from, area } => {
                if let Some(target) = self.area_mut(id) {
                    target.x = area.x + (p.0 - from.0) / zoom;
                    target.y = area.y + (p.1 - from.1) / zoom;
                }
            }
            Drag::Resize { id, from, area } => {
                let min = self.area_for(40, 8);
                if let Some(target) = self.area_mut(id) {
                    target.w = (area.w + (p.0 - from.0) / zoom).max(min.w);
                    target.h = (area.h + (p.1 - from.1) / zoom).max(min.h);
                }
            }
        }
        cx.notify();
    }

    /// Empieza a mover la zona de un espacio: todas sus consolas juntas.
    fn start_zone_drag(&mut self, workspace: u64, from: (f32, f32)) {
        let mut moving: Vec<(u64, Area)> =
            self.cards.iter().filter(|c| c.workspace == Some(workspace)).map(|c| (c.id, c.area)).collect();
        // Los archivos abiertos junto a esas consolas se mueven con ellas.
        let anchors: Vec<u64> = moving.iter().map(|(id, _)| *id).collect();
        for doc in &self.docs {
            if let (Some(area), true) = (doc.area, doc.anchor.is_some_and(|a| anchors.contains(&a))) {
                moving.push((doc.id, area));
            }
        }
        self.zone_drag = Some((from, moving));
    }

    /// Las zonas de los espacios con consolas.
    fn zone_list(&self) -> Vec<zones::Zone> {
        let ids: Vec<u64> = self.spaces.list().iter().map(|s| s.id).collect();
        zones::zones(&ids, &self.board_items())
    }

    /// Lo que ocupa lugar en la pizarra, con su espacio: las consolas y los
    /// archivos abiertos junto a ellas (del espacio de su consola).
    fn board_items(&self) -> Vec<(Option<u64>, Area)> {
        let docs = self.docs.iter().filter_map(|doc| {
            let workspace = doc.anchor.and_then(|id| self.card(id)).and_then(|c| c.workspace);
            doc.area.map(|area| (workspace, area))
        });
        self.cards.iter().map(|c| (c.workspace, c.area)).chain(docs).collect()
    }

    /// El área de una consola o de un archivo de la pizarra.
    fn area_mut(&mut self, id: u64) -> Option<&mut Area> {
        if let Some(card) = self.cards.iter_mut().find(|c| c.id == id) {
            return Some(&mut card.area);
        }
        self.docs.iter_mut().find(|d| d.id == id).and_then(|d| d.area.as_mut())
    }

    /// El archivo de la pizarra de más arriba bajo el punto de pantalla.
    fn doc_at(&self, p: (f32, f32)) -> Option<(u64, Area)> {
        let board = self.camera.to_board(p);
        self.docs.iter().rev().find_map(|d| d.area.filter(|a| a.contains(board)).map(|a| (d.id, a)))
    }

    /// Abre un archivo como tarjeta de la pizarra, en columna a la derecha de
    /// la consola `anchor` (con una línea que los une). Si ya estaba, lo trae.
    fn board_doc(&mut self, path: &std::path::Path, show_diff: bool, anchor: Option<u64>, cx: &mut Context<Self>) {
        if let Some(index) = self.docs.iter().position(|d| d.area.is_some() && workspaces::same(&d.path, path)) {
            let mut doc = self.docs.remove(index);
            doc.reload();
            doc.show_diff = show_diff && doc.diff.is_some();
            let area = doc.area;
            self.docs.push(doc);
            if let Some(area) = area {
                self.reveal_area(area);
            }
            cx.notify();
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut doc = viewer::Doc::load(id, path, show_diff);
        let size = self.area_for(84, 24);
        let anchor = anchor.and_then(|id| self.card(id)).map(|c| (c.id, c.area));
        let (x, y) = match anchor {
            Some((anchor_id, a)) => {
                let stacked = self.docs.iter().filter(|d| d.anchor == Some(anchor_id) && d.area.is_some()).count();
                (a.x + a.w + 60.0, a.y + stacked as f32 * (size.h + 24.0))
            }
            None => {
                let center = self.camera.to_board((self.viewport.0 / 2.0, (self.viewport.1 + TOOLBAR_H) / 2.0));
                (center.0 - size.w / 2.0, center.1 - size.h / 2.0)
            }
        };
        let area = Area { x, y, ..size };
        doc.area = Some(area);
        doc.anchor = anchor.map(|(id, _)| id);
        self.docs.push(doc);
        self.reveal_area(area);
        cx.notify();
    }

    /// Los archivos que cambió una consola, como tarjetas a su lado (los
    /// primeros cuatro: más no se alcanzan a leer).
    fn open_changed(&mut self, id: u64, cx: &mut Context<Self>) {
        let paths: Vec<std::path::PathBuf> = self
            .card(id)
            .map(|c| c.changes.list.iter().filter(|f| f.kind != changes::Kind::Deleted).take(4).map(|f| f.path.clone()).collect())
            .unwrap_or_default();
        for path in paths {
            self.board_doc(&path, true, Some(id), cx);
        }
    }

    /// «Ordenar»: las consolas del espacio en una grilla, desde donde empieza
    /// la zona.
    fn arrange_zone(&mut self, workspace: u64, cx: &mut Context<Self>) {
        let mut mine: Vec<(u64, Area)> =
            self.cards.iter().filter(|c| c.workspace == Some(workspace)).map(|c| (c.id, c.area)).collect();
        mine.sort_by_key(|(id, _)| *id);
        let areas: Vec<Area> = mine.iter().map(|(_, a)| *a).collect();
        let origin = (
            areas.iter().map(|a| a.x).fold(f32::MAX, f32::min),
            areas.iter().map(|a| a.y).fold(f32::MAX, f32::min),
        );
        for ((id, _), (x, y)) in mine.iter().zip(zones::arrange(&areas, origin)) {
            if let Some(card) = self.card_mut(*id) {
                card.area.x = x;
                card.area.y = y;
            }
        }
        self.moved();
        cx.notify();
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        self.zone_drag = None;
        // Un clic sin arrastrar en una tarjeta lejana: la cámara va a ella.
        if let Some((id, _)) = self.tap.take() {
            self.focus_on(id);
        }
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
        } else if let Some((id, _)) = self.doc_at(p).filter(|_| self.camera.zoom >= LIVE_ZOOM) {
            // La rueda sobre un archivo lo recorre.
            let lines = (dy / (self.cell().h * self.camera.zoom)).round() as i64;
            if let Some(doc) = self.docs.iter_mut().find(|d| d.id == id) {
                let last = doc.rows().saturating_sub(1) as i64;
                doc.scroll = (doc.scroll as i64 - lines).clamp(0, last) as usize;
            }
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
        self.fly_fit();
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
        self.focus_on(next);
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
            .child(mando::board_toggles(self, cx))
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
            .child(mando::new_agent_button(cx))
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
                    label: format!("{} · {}", card.name, card.label).into(),
                    title: title.into(),
                    agent: card.agent,
                    color: card.color,
                    changes: card.changes.list.len(),
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
        color: console::term(fg),
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
    /// El color de la consola (`identity`): pinta el fondo de su logo.
    color: u32,
    changes: usize,
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
        // El logo del agente sobre su color, como en el Mando.
        let logo = 18. * z;
        let logo_bounds = Bounds::new(point(px(x0 + 8. * z), px(cy - logo / 2.)), size(px(logo), px(logo)));
        window.paint_quad(quad(
            logo_bounds,
            Corners::all(px(logo * 0.28)),
            console::hsla(self.color),
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
        if logo >= 8.0 {
            let glyph = logo * 0.62;
            let _ = window.paint_svg(
                Bounds::new(
                    point(px(x0 + 8. * z + (logo - glyph) / 2.), px(cy - glyph / 2.)),
                    size(px(glyph), px(glyph)),
                ),
                mando::agent_icon(self.agent).into(),
                gpui::TransformationMatrix::unit(),
                console::hsla(0x141413),
                cx,
            );
        }
        // El estado, junto a la × de cerrar.
        let dot_r = 3.5 * z;
        window.paint_quad(quad(
            Bounds::new(
                point(px(f32::from(b.right()) - 40. * z - dot_r), px(cy - dot_r)),
                size(px(dot_r * 2.), px(dot_r * 2.)),
            ),
            Corners::all(px(dot_r)),
            console::hsla(dot),
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
        let label_size = (12.0 * z).max(1.0);
        // De cerca, los archivos cambiados son un botón aparte (`change_chips`):
        // el texto le deja su lugar.
        let chip = self.changes > 0 && z >= CHIP_ZOOM;
        if label_size >= 4.0 {
            let mut text = self.label.to_string();
            if self.changes > 0 && !chip {
                text = format!("{text}  ·  {}", mando::plural(self.changes, "archivo", "archivos"));
            }
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
                (x0 + 34. * z, cy - label_size * 0.65),
                Some(f32::from(b.size.width) - 90. * z - if chip { CHIP_W + 8.0 } else { 0.0 }),
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
                console::term(color),
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
                                console::term(c).opacity(0.55),
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
                console::term(color),
            ));
        }
    }
    if let Some((row, col)) = grid.cursor {
        let bounds = Bounds::new(
            point(px(origin.0 + col.min(grid.cols) as f32 * cw), px(origin.1 + row as f32 * lh)),
            size(px(cw), px(lh)),
        );
        if focused {
            window.paint_quad(gpui::fill(bounds, console::term(console::FOREGROUND).opacity(0.75)));
        } else {
            window.paint_quad(gpui::outline(bounds, console::term(MUTED), gpui::BorderStyle::Solid));
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
        paint_text_in(name, big, console::term(TEXT), (x, y), None, window, cx);
        y += big * 1.6;
    }
    let small = 22. * z;
    if small >= 3.0 {
        for line in lines {
            let line: String = line.trim().chars().take(70).collect();
            paint_text_in(&line, small, console::term(MUTED), (x, y), Some(f32::from(content.size.width) - 32. * z), window, cx);
            y += small * 1.45;
        }
    }
}

fn paint_text(
    text: &str,
    font_size: f32,
    color: u32,
    at: (f32, f32),
    max_width: Option<f32>,
    window: &mut Window,
    cx: &mut App,
) {
    paint_text_in(text, font_size, console::hsla(color), at, max_width, window, cx);
}

/// `paint_text` con el color ya resuelto: sobre una terminal va sin tema.
fn paint_text_in(
    text: &str,
    font_size: f32,
    color: Hsla,
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
        color,
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

/// Una fila de un archivo de la pizarra.
struct DocRow {
    number: Option<usize>,
    text: String,
    background: Option<u32>,
    color: u32,
}

/// Lo que se dibuja de un archivo abierto en la pizarra.
struct DocDraw {
    bounds: Bounds<Pixels>,
    zoom: f32,
    title: String,
    /// `Some(viendo los cambios)` si hay cambios que mirar.
    toggle: Option<bool>,
    note: Option<String>,
    rows: Vec<DocRow>,
    digits: usize,
}

impl SpaceView {
    fn doc_snapshot(&self) -> Vec<DocDraw> {
        let viewport = Bounds::new(
            point(px(0.), px(TOOLBAR_H)),
            size(px(self.viewport.0), px(self.viewport.1 - TOOLBAR_H)),
        );
        let cell = self.cell();
        self.docs
            .iter()
            .filter_map(|doc| {
                let area = doc.area?;
                let bounds = self.camera.area(&area);
                if !bounds.intersects(&viewport) {
                    return None;
                }
                let fits = ((area.h - HEADER - PAD) / cell.h).floor().max(1.0) as usize;
                let (rows, digits): (Vec<DocRow>, usize) = match (&doc.diff, doc.show_diff) {
                    (Some(diff), true) => (
                        diff.iter()
                            .skip(doc.scroll)
                            .take(fits)
                            .map(|line| {
                                let (background, color) = match line.kind {
                                    viewer::LineKind::Added => (Some(0x1d3324), 0xc8f0d2),
                                    viewer::LineKind::Removed => (Some(0x3a2020), 0xf2c4bd),
                                    viewer::LineKind::Hunk => (Some(0x1f2430), 0x8fa6d6),
                                    viewer::LineKind::Context => (None, console::FOREGROUND),
                                };
                                DocRow { number: line.number, text: line.text.clone(), background, color }
                            })
                            .collect(),
                        diff.iter().filter_map(|l| l.number).max().unwrap_or(1).to_string().len(),
                    ),
                    _ => (
                        doc.lines
                            .iter()
                            .enumerate()
                            .skip(doc.scroll)
                            .take(fits)
                            .map(|(index, text)| DocRow {
                                number: Some(index + 1),
                                text: text.clone(),
                                background: None,
                                color: console::FOREGROUND,
                            })
                            .collect(),
                        doc.lines.len().max(1).to_string().len(),
                    ),
                };
                let path = doc.path.display().to_string();
                let (dir, name) = path.rsplit_once(['\\', '/']).unwrap_or(("", &path));
                let dir = dir.rsplit(['\\', '/']).next().unwrap_or("");
                Some(DocDraw {
                    bounds,
                    zoom: self.camera.zoom,
                    title: if dir.is_empty() { name.to_string() } else { format!("{name}  ·  {dir}") },
                    toggle: doc.diff.as_ref().map(|_| doc.show_diff),
                    note: doc.note.clone(),
                    rows,
                    digits,
                })
            })
            .collect()
    }

    /// De la consola al archivo: del borde derecho de una al encabezado del otro.
    fn doc_links(&self) -> Vec<((f32, f32), (f32, f32))> {
        self.docs
            .iter()
            .filter_map(|doc| {
                let area = doc.area?;
                let anchor = self.card(doc.anchor?)?.area;
                let from = self.camera.to_screen((anchor.x + anchor.w, anchor.y + HEADER / 2.0));
                let to = self.camera.to_screen((area.x, area.y + HEADER / 2.0));
                Some((from, to))
            })
            .collect()
    }

    /// «N archivos» en el encabezado de cada consola que cambió algo: abre
    /// esos archivos a su lado.
    fn change_chips(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let z = self.camera.zoom;
        if z < CHIP_ZOOM {
            return Vec::new();
        }
        self.cards
            .iter()
            .filter(|c| !c.changes.list.is_empty())
            .filter_map(|card| {
                let b = self.camera.area(&card.area);
                let (right, top) = (f32::from(b.right()), f32::from(b.origin.y));
                if right < 0.0 || top > self.viewport.1 || top + HEADER * z < TOOLBAR_H {
                    return None;
                }
                let id = card.id;
                Some(
                    div()
                        .id(("change-chip", id as usize))
                        .absolute()
                        .left(px(right - 52. * z - CHIP_W))
                        .top(px(top + (HEADER * z - 22.) / 2.))
                        .w(px(CHIP_W))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(11.))
                        .bg(console::hsla(0x2a2a28))
                        .hover(|el| el.bg(console::hsla(0x353532)))
                        .cursor_pointer()
                        .font_family("Segoe UI")
                        .text_size(px(11.5))
                        .text_color(console::hsla(WORKING))
                        .tooltip(crate::hover::tip("Abrir a su lado los archivos que cambió"))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.open_changed(id, cx)))
                        .child(mando::plural(card.changes.list.len(), "archivo", "archivos"))
                        .into_any_element(),
                )
            })
            .collect()
    }
}

/// Una línea de código, con la letra de las consolas.
fn paint_mono(text: &str, font_size: f32, color: u32, (x, y): (f32, f32), window: &mut Window, cx: &mut App) {
    if text.is_empty() {
        return;
    }
    let run = TextRun {
        len: text.len(),
        font: font(),
        color: console::term(color),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(text.to_string().into(), px(font_size), &[run], None);
    let _ = line.paint(point(px(x), px(y)), px(font_size * LINE_HEIGHT), window, cx);
}

impl DocDraw {
    fn paint(self, cell: Cell, window: &mut Window, cx: &mut App) {
        let z = self.zoom;
        let b = self.bounds;
        let radius = px(RADIUS * z);
        window.paint_shadows(b, Corners::all(radius), &[shadow(0.35, z)]);
        window.paint_quad(quad(b, Corners::all(radius), console::hsla(CARD), px(0.), gpui::transparent_black(), gpui::BorderStyle::Solid));
        let header_h = HEADER * z;
        window.paint_quad(quad(
            Bounds::new(b.origin, size(b.size.width, px(header_h))),
            Corners { top_left: radius, top_right: radius, bottom_left: px(0.), bottom_right: px(0.) },
            console::hsla(HEADER_BG),
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::Solid,
        ));
        let x0 = f32::from(b.origin.x);
        let right = f32::from(b.right());
        let cy = f32::from(b.origin.y) + header_h / 2.0;
        let icon = 14. * z;
        if icon >= 5.0 {
            let _ = window.paint_svg(
                Bounds::new(point(px(x0 + 10. * z), px(cy - icon / 2.)), size(px(icon), px(icon))),
                "icons/text-align-start.svg".into(),
                gpui::TransformationMatrix::unit(),
                console::hsla(MUTED),
                cx,
            );
        }
        let label = (12.0 * z).max(1.0);
        if label >= 4.0 {
            let toggle_w = if self.toggle.is_some() { DOC_TOGGLE * z } else { 0.0 };
            paint_text(&self.title, label, TEXT, (x0 + 32. * z, cy - label * 0.65), Some(f32::from(b.size.width) - 70. * z - toggle_w), window, cx);
            if let Some(diff) = self.toggle {
                // Lo que se está viendo; un clic cambia al otro.
                let text = if diff { "Cambios ⇄" } else { "Archivo ⇄" };
                paint_text(text, label * 0.95, if diff { WORKING } else { MUTED }, (right - HEADER * z - DOC_TOGGLE * z + 6. * z, cy - label * 0.62), None, window, cx);
            }
            paint_text("×", label * 1.3, FAINT, (right - 22. * z, cy - label * 0.85), None, window, cx);
        }
        let content = Bounds::new(
            point(b.origin.x, b.origin.y + px(header_h)),
            size(b.size.width, b.size.height - px(header_h)),
        );
        let font_size = FONT_SIZE * z;
        window.with_content_mask(Some(ContentMask { bounds: content }), |window| {
            if z < LIVE_ZOOM || font_size < 4.0 {
                return;
            }
            let (left, top) = (x0 + PAD * z, f32::from(content.origin.y) + 4. * z);
            if let Some(note) = &self.note {
                paint_text_in(note, label, console::term(MUTED), (left, top + 8. * z), None, window, cx);
                return;
            }
            let lh = cell.h * z;
            let gutter = (self.digits as f32 + 2.0) * cell.w * z;
            for (index, row) in self.rows.iter().enumerate() {
                let y = top + index as f32 * lh;
                if let Some(color) = row.background {
                    window.paint_quad(gpui::fill(
                        Bounds::new(point(b.origin.x, px(y)), size(b.size.width, px(lh))),
                        console::hsla(color),
                    ));
                }
                if let Some(number) = row.number {
                    let text = format!("{number:>width$}", width = self.digits);
                    paint_mono(&text, font_size, FAINT, (left, y), window, cx);
                }
                paint_mono(&row.text, font_size, row.color, (left + gutter, y), window, cx);
            }
        });
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
        let flying = self.step_flight(now);
        if flying || self.bench_step(now) || now < self.moving_until {
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
        let zone_list = self.zone_list();
        let frames: Vec<Bounds<Pixels>> = zone_list.iter().map(|z| self.camera.area(&z.area)).collect();
        let zone_radius = 22. * camera.zoom;
        let headers = mando::zone_headers(self, &zone_list, cx);
        let chips = self.change_chips(cx);
        let docs = self.doc_snapshot();
        let links = self.doc_links();
        let overlays = mando::board_overlays(self, cx);
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
                        // Las zonas de los espacios, debajo de sus consolas.
                        for frame in frames {
                            window.paint_quad(quad(
                                frame,
                                Corners::all(px(zone_radius)),
                                console::hsla(ZONE),
                                px(0.),
                                gpui::transparent_black(),
                                gpui::BorderStyle::Solid,
                            ));
                        }
                        for card in cards {
                            card.paint(cell, window, cx);
                        }
                        // Cada archivo, unido con una línea a la consola que lo abrió.
                        for (from, to) in links {
                            let mut path = gpui::PathBuilder::stroke(px(1.5));
                            path.move_to(point(px(from.0), px(from.1)));
                            path.line_to(point(px(to.0), px(to.1)));
                            if let Ok(path) = path.build() {
                                window.paint_path(path, console::hsla(FAINT));
                            }
                        }
                        for doc in docs {
                            doc.paint(cell, window, cx);
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
                        .child("Abre un agente con «Nuevo agente» o desde Agentes en el notch."),
                )
            })
            .children(headers)
            .children(chips)
            .child(input::layer(cx.weak_entity(), self.focus.clone()))
            .child(self.toolbar(cx))
            .children(overlays)
            .into_any_element()
    }
}
