//! El uso de los agentes, en un vistazo del notch: al pasar sobre Agentes en
//! la tira (como el portapapeles con su vistazo), el notch baja una tarjeta
//! por agente con su cupo (5 horas, semana…), cuándo se reinicia y qué hay
//! instalado: el cliente de terminal y la app de escritorio.
//!
//! El cupo es el de Atic (`quota/`, copiado de su backend): Claude por la API
//! de Anthropic con la sesión de `~/.claude`, Codex de sus propios archivos,
//! OpenCode, Cursor, Antigravity y Grok. Atic solo miraba el PATH; acá además
//! se detectan las apps de escritorio (Claude y Codex de la Tienda, Cursor,
//! Antigravity…).
//!
//! Dos modos, elegidos en el encabezado y guardados en `usage.txt`: «Simple»
//! (una fila de anillos; al pasar o presionar uno baja su tarjeta) y
//! «Detalle» (todas las tarjetas). La × de cada agente lo saca de la vista y
//! mantener presionado (o el lápiz) abre el personalizador (`usage/view.rs`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{point, px, Hsla};

use crate::anim::segment;

mod view;
use crate::geometry::Rect;
use crate::quota::{self, AgentQuota, QuotaOverview};
use crate::{Pill, PillShape, PANEL_W, TAB_THICK};

/// Cuánto hay que quedarse sobre Agentes para que baje, y la gracia al salir.
const HOVER_DELAY: Duration = Duration::from_millis(420);
const LEAVE_GRACE: Duration = Duration::from_millis(380);
/// Los datos se releen como mucho cada tanto (Atic además cachea 60 s).
const STALE_AFTER: Duration = Duration::from_secs(20);

const HEADER_H: f32 = 34.0;
const CARD_H: f32 = 96.0;
const GAP: f32 = 8.0;
const SIDE: f32 = 12.0;
/// Modo simple: la fila de anillos.
const SIMPLE_H: f32 = 84.0;
const BOTTOM: f32 = 12.0;
const RING: f32 = 40.0;

const OK: u32 = 0x6cc48a;
const WARN: u32 = 0xe8b04b;
const HOT: u32 = 0xff6b63;

/// Cómo saber si una app de escritorio está instalada, sin listar paquetes
/// (lento): la Tienda deja `%LOCALAPPDATA%\Packages\<familia>`, y los
/// instaladores por usuario, su carpeta en `%LOCALAPPDATA%`.
#[derive(Clone, Copy)]
enum Check {
    /// Prefijo del nombre de familia del paquete (`Claude_`).
    Store(&'static str),
    /// Archivo o carpeta bajo `%LOCALAPPDATA%`.
    Local(&'static str),
}

#[derive(Clone, Copy)]
struct App {
    name: &'static str,
    check: Check,
}

/// Un agente: su id de cupo (el de Atic), su cliente de terminal y sus apps.
struct Family {
    id: &'static str,
    name: &'static str,
    logo: &'static str,
    cli: &'static str,
    apps: &'static [App],
}

/// El orden de Atic (`pillQuota.ts`).
const FAMILIES: [Family; 6] = [
    Family {
        id: "claude",
        name: "Claude",
        logo: "icons/agents/claude.svg",
        cli: "claude",
        apps: &[
            App { name: "Claude", check: Check::Store("Claude_") },
            App { name: "Claude", check: Check::Local("AnthropicClaude\\claude.exe") },
        ],
    },
    Family {
        id: "codex",
        name: "Codex",
        logo: "icons/agents/openai.svg",
        cli: "codex",
        apps: &[
            App { name: "Codex", check: Check::Store("OpenAI.Codex_") },
            App { name: "ChatGPT", check: Check::Store("OpenAI.ChatGPT-Desktop_") },
        ],
    },
    Family {
        id: "opencode",
        name: "OpenCode",
        logo: "icons/agents/opencode.svg",
        cli: "opencode",
        apps: &[App { name: "OpenCode", check: Check::Local("Programs\\OpenCode") }],
    },
    Family {
        id: "agy",
        name: "Antigravity",
        logo: "icons/agents/antigravity.svg",
        cli: "agy",
        apps: &[App { name: "Antigravity", check: Check::Local("Programs\\Antigravity\\Antigravity.exe") }],
    },
    Family {
        id: "grok",
        name: "Grok",
        logo: "icons/agents/grok.svg",
        cli: "grok",
        apps: &[],
    },
    Family {
        id: "cursor-agent",
        name: "Cursor",
        logo: "icons/agents/cursor.svg",
        cli: "cursor-agent",
        apps: &[App { name: "Cursor", check: Check::Local("Programs\\cursor\\Cursor.exe") }],
    },
];

fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

fn installed(app: &App) -> bool {
    let Some(local) = local_app_data() else { return false };
    match app.check {
        Check::Local(rel) => local.join(rel).exists(),
        Check::Store(prefix) => std::fs::read_dir(local.join("Packages")).is_ok_and(|dir| {
            dir.flatten().any(|e| e.file_name().to_string_lossy().starts_with(prefix))
        }),
    }
}

/// El cliente de terminal: con la regla de Atic (PATH del proceso y del
/// registro, `.exe/.cmd/.bat`) y además los `.ps1` de npm y de los
/// instaladores de Cursor, que Atic no ve.
fn cli_installed(name: &str) -> bool {
    quota::exe::resolve(name).is_some()
        || quota::exe::search_dirs().iter().any(|dir| dir.join(format!("{name}.ps1")).is_file())
}

/// Lo instalado de un agente.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Installed {
    pub cli: bool,
    pub apps: Vec<&'static str>,
}

#[derive(Clone, Default)]
struct Snapshot {
    families: Vec<Installed>,
    quota: Option<QuotaOverview>,
    at: Option<Instant>,
}

/// Lo que elige el usuario: el modo, el orden de los agentes y los que
/// quedan fuera de la vista.
#[derive(Clone, Debug, PartialEq)]
struct Prefs {
    simple: bool,
    /// Ids en el orden elegido; los que falten van al final, en el de siempre.
    order: Vec<String>,
    hidden: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        let text = prefs_file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        Prefs::parse(&text)
    }
}

fn ids(list: &str) -> Vec<String> {
    list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

impl Prefs {
    /// `modo=simple|detalle`, `orden=claude,codex` y `ocultos=grok`, una por
    /// línea. `fijados=` (la versión anterior) se lee como «los demás fuera».
    fn parse(text: &str) -> Self {
        let mut prefs = Prefs { simple: true, order: Vec::new(), hidden: Vec::new() };
        for line in text.lines() {
            match line.trim().split_once('=') {
                Some(("modo", mode)) => prefs.simple = mode.trim() != "detalle",
                Some(("orden", list)) => prefs.order = ids(list),
                Some(("ocultos", list)) => prefs.hidden = ids(list),
                Some(("fijados", list)) if !ids(list).is_empty() => {
                    let pinned = ids(list);
                    prefs.hidden =
                        FAMILIES.iter().map(|f| f.id.to_string()).filter(|id| !pinned.contains(id)).collect();
                }
                _ => {}
            }
        }
        prefs
    }

    fn save(&self) {
        let Some(path) = prefs_file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mode = if self.simple { "simple" } else { "detalle" };
        let _ = std::fs::write(
            path,
            format!("modo={mode}\norden={}\nocultos={}\n", self.order.join(","), self.hidden.join(",")),
        );
    }

    /// Los ids en orden: los elegidos primero, el resto en el de siempre.
    fn ordered<'a>(&self, all: &[&'a str]) -> Vec<&'a str> {
        let mut out: Vec<&'a str> =
            self.order.iter().filter_map(|id| all.iter().find(|a| **a == id.as_str()).copied()).collect();
        let rest: Vec<&'a str> = all.iter().filter(|a| !out.contains(a)).copied().collect();
        out.extend(rest);
        out
    }

    /// A la vista y fuera, cada uno en su orden.
    fn split<'a>(&self, all: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
        self.ordered(all).into_iter().partition(|id| !self.hidden.iter().any(|h| h == id))
    }

    /// Mueve un agente a una fila (`hidden`) y posición. Siempre queda al
    /// menos uno a la vista: si no, no hace nada y devuelve `false`.
    fn place(&mut self, all: &[&str], id: &str, hidden: bool, index: usize) -> bool {
        let (mut shown, mut out) = self.split(all);
        let from_shown = shown.contains(&id);
        if hidden && from_shown && shown.len() == 1 {
            return false;
        }
        shown.retain(|x| *x != id);
        out.retain(|x| *x != id);
        let Some(id) = all.iter().find(|a| **a == id).copied() else { return false };
        let row = if hidden { &mut out } else { &mut shown };
        row.insert(index.min(row.len()), id);
        self.order = shown.iter().chain(out.iter()).map(|s| s.to_string()).collect();
        self.hidden = out.iter().map(|s| s.to_string()).collect();
        true
    }
}

fn prefs_file() -> Option<PathBuf> {
    crate::paths::file("usage.txt")
}

/// El estado de la vista mientras el vistazo está abierto.
#[derive(Clone, Default)]
struct View {
    /// En modo simple, el agente con la tarjeta abierta.
    focus: Option<&'static str>,
    /// El personalizador abierto (mantener presionado o el lápiz).
    editing: bool,
    /// Un agente presionado: si se mantiene, abre el personalizador.
    press: Option<(u64, &'static str)>,
    press_seq: u64,
    /// Una ficha arrastrándose en el personalizador.
    drag: Option<Drag>,
    /// «Tiene que quedar al menos uno a la vista».
    refused: bool,
    /// Cuándo se abrió el vistazo, cambió el detalle y cambió el modo (o se
    /// abrió el personalizador): de ahí salen las animaciones de entrada.
    opened_at: Option<Instant>,
    /// Se abrió sin datos (primera lectura): la entrada espera a que lleguen.
    opened_empty: bool,
    focus_at: Option<Instant>,
    mode_at: Option<Instant>,
    /// Dónde quedaron las fichas y las filas al pintarse, para saber dónde
    /// se suelta lo arrastrado.
    chips: Vec<(&'static str, bool, gpui::Bounds<gpui::Pixels>)>,
    lanes: [Option<gpui::Bounds<gpui::Pixels>>; 2],
}

#[derive(Clone, Copy)]
struct Drag {
    id: &'static str,
    start: (f32, f32),
    pos: (f32, f32),
    moved: bool,
    /// A qué fila (fuera = `true`) y en qué lugar iría.
    target: Option<(bool, usize)>,
}

/// El uso y lo instalado, leídos en otro hilo y compartidos con la pill.
#[derive(Clone, Default)]
pub struct Usage {
    snap: Arc<Mutex<Snapshot>>,
    loading: Arc<AtomicBool>,
    version: Arc<AtomicU64>,
    prefs: Arc<Mutex<Prefs>>,
    view: Arc<Mutex<View>>,
    /// El alto del vistazo, animado: al abrir un detalle o cambiar de modo
    /// el notch se estira suave en vez de saltar.
    height: Arc<Mutex<Option<crate::anim::Tween>>>,
}

impl Usage {
    /// Relee si los datos tienen más de `STALE_AFTER` (se llama al pasar el
    /// cursor: cuando el vistazo baja, ya están o están por llegar).
    pub fn refresh(&self) {
        let fresh = self.snap.lock().ok().and_then(|s| s.at).is_some_and(|at| at.elapsed() < STALE_AFTER);
        if fresh || self.loading.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = self.clone();
        std::thread::Builder::new()
            .name("uso-agentes".into())
            .spawn(move || {
                let started = Instant::now();
                let families = FAMILIES
                    .iter()
                    .map(|f| {
                        let mut apps: Vec<&'static str> =
                            f.apps.iter().filter(|a| installed(a)).map(|a| a.name).collect();
                        apps.dedup();
                        Installed { cli: cli_installed(f.cli), apps }
                    })
                    .collect();
                let overview = quota::fetch_overview(false);
                if std::env::var_os("PILL_DEBUG").is_some() {
                    eprintln!(
                        "[uso] {} agentes con cupo en {} ms",
                        overview.agents.len(),
                        started.elapsed().as_millis()
                    );
                }
                if let Ok(mut snap) = this.snap.lock() {
                    *snap = Snapshot { families, quota: Some(overview), at: Some(Instant::now()) };
                }
                this.loading.store(false, Ordering::Release);
                this.version.fetch_add(1, Ordering::Relaxed);
            })
            .ok();
    }

    fn snapshot(&self) -> Snapshot {
        self.snap.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn loading(&self) -> bool {
        self.loading.load(Ordering::Relaxed)
    }

    fn prefs(&self) -> Prefs {
        self.prefs.lock().map(|p| p.clone()).unwrap_or_default()
    }

    fn update_prefs(&self, change: impl FnOnce(&mut Prefs)) {
        if let Ok(mut prefs) = self.prefs.lock() {
            change(&mut prefs);
            prefs.save();
        }
    }

    fn set_simple(&self, simple: bool) {
        if self.prefs().simple != simple {
            self.update_view(|v| v.mode_at = Some(Instant::now()));
        }
        self.update_prefs(|p| p.simple = simple);
    }

    /// Mueve un agente; si dejaría la vista vacía, avisa y no lo mueve.
    fn place(&self, id: &str, hidden: bool, index: usize) {
        let mut ok = true;
        self.update_prefs(|p| ok = p.place(&all_ids(), id, hidden, index));
        if let Ok(mut view) = self.view.lock() {
            view.refused = !ok;
        }
    }

    /// Quitar de la vista (la × al pasar el cursor).
    fn hide(&self, id: &str) {
        self.place(id, true, usize::MAX);
        if let Ok(mut view) = self.view.lock() {
            if view.focus.is_some_and(|f| f == id) {
                view.focus = None;
            }
        }
    }

    fn reset_prefs(&self) {
        self.update_prefs(|p| {
            p.order.clear();
            p.hidden.clear();
        });
    }

    fn update_view(&self, change: impl FnOnce(&mut View)) {
        if let Ok(mut view) = self.view.lock() {
            change(&mut view);
        }
    }

    fn view(&self) -> View {
        self.view.lock().map(|v| v.clone()).unwrap_or_default()
    }

    fn set_focus(&self, focus: Option<&'static str>) {
        if let Ok(mut view) = self.view.lock() {
            if view.focus != focus {
                view.focus = focus;
                view.focus_at = Some(Instant::now());
            }
        }
    }

    fn set_editing(&self, editing: bool) {
        self.update_view(|view| {
            if view.editing != editing {
                view.mode_at = Some(Instant::now());
            }
            view.editing = editing;
            view.focus = None;
            view.press = None;
            view.drag = None;
            view.refused = false;
        });
    }

    /// Al cerrarse el vistazo, la próxima vez abre compacto.
    pub(crate) fn reset_view(&self) {
        if let Ok(mut view) = self.view.lock() {
            *view = View::default();
        }
        if let Ok(mut height) = self.height.lock() {
            *height = None;
        }
    }

    /// Empieza a abrirse: los anillos se llenan desde cero.
    pub(crate) fn mark_opened(&self) {
        let empty = self.snap.lock().ok().is_some_and(|s| s.at.is_none());
        self.update_view(|v| {
            v.opened_at = Some(Instant::now());
            v.opened_empty = empty;
        });
    }

    /// Desde cuándo cuenta la entrada: al abrir o, si se abrió sin datos,
    /// cuando llegaron. Una relectura con el vistazo abierto no la repite.
    fn intro_start(&self, view: &View) -> Option<Instant> {
        let data = self.snap.lock().ok().and_then(|s| s.at);
        match (view.opened_at, data) {
            (Some(open), Some(data)) if view.opened_empty => Some(open.max(data)),
            (Some(_), None) if view.opened_empty => None,
            (open, _) => open,
        }
    }

    /// Segundos desde cada cosa (muchos si no pasó).
    fn since(at: Option<Instant>) -> f32 {
        at.map_or(f32::MAX, |at| at.elapsed().as_secs_f32())
    }

    /// El alto, animado hacia `target`.
    fn animated_height(&self, target: f32) -> f32 {
        let now = Instant::now();
        let Ok(mut slot) = self.height.lock() else { return target };
        let tween = slot.get_or_insert_with(|| {
            crate::anim::Tween::new(target, Duration::from_millis(280), crate::anim::ease_smooth_out)
        });
        if (tween.target() - target).abs() > 0.5 {
            tween.set(target, now);
        }
        tween.value(now)
    }

    /// Hay una animación de la vista en curso: la pill pide cuadros.
    fn animating(&self) -> bool {
        let view = self.view();
        let height = self.height.lock().ok().is_some_and(|h| h.as_ref().is_some_and(|t| t.is_running(Instant::now())));
        let loading = self.snap.lock().ok().is_some_and(|s| s.at.is_none());
        loading || Self::since(self.intro_start(&view)) < 1.2 || Self::since(view.focus_at) < 0.4 || Self::since(view.mode_at) < 0.4 || height
    }
}

/// Una fila de la vista: el agente, lo instalado y su cupo.
struct Row {
    family: &'static Family,
    installed: Installed,
    quota: Option<AgentQuota>,
}

fn rows(snap: &Snapshot) -> Vec<Row> {
    FAMILIES
        .iter()
        .enumerate()
        .filter_map(|(i, family)| {
            let installed = snap.families.get(i).cloned().unwrap_or_default();
            let quota = snap.quota.as_ref().and_then(|q| q.agents.iter().find(|a| a.agent == family.id).cloned());
            // Sin nada instalado y sin cupo, el agente no va.
            (installed.cli || !installed.apps.is_empty() || quota.is_some()).then_some(Row {
                family,
                installed,
                quota,
            })
        })
        .collect()
}

fn all_ids() -> Vec<&'static str> {
    FAMILIES.iter().map(|f| f.id).collect()
}

/// A la vista y fuera, en el orden elegido (solo los detectados).
fn arranged(snap: &Snapshot, prefs: &Prefs) -> (Vec<Row>, Vec<Row>) {
    let mut rows = rows(snap);
    let present: Vec<&'static str> = rows.iter().map(|r| r.family.id).collect();
    let (shown, out) = prefs.split(&present);
    let mut take = |ids: Vec<&'static str>| -> Vec<Row> {
        ids.into_iter()
            .filter_map(|id| rows.iter().position(|r| r.family.id == id).map(|i| rows.remove(i)))
            .collect()
    };
    let shown = take(shown);
    let out = take(out);
    (shown, out)
}

/// Los que se ven.
fn visible(snap: &Snapshot, prefs: &Prefs) -> Vec<Row> {
    arranged(snap, prefs).0
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Las ventanas vigentes (las ya reiniciadas no dicen nada).
fn live_windows(quota: &AgentQuota) -> Vec<&quota::QuotaWindow> {
    let now = now_ms();
    quota.windows.iter().filter(|w| w.resets_at.is_none_or(|at| at > now)).collect()
}

/// El nombre corto de una ventana, como en Atic (`pillQuota.ts`).
pub(crate) fn window_label(kind: &str, minutes: Option<u64>) -> String {
    let by_kind = match kind {
        "5h" => Some("5 h"),
        "7d" | "weekly" => Some("Semana"),
        "7dOpus" => Some("Opus"),
        "7dSonnet" => Some("Sonnet"),
        "rolling" => Some("Ahora"),
        "monthly" => Some("Mes"),
        "auto" => Some("Auto"),
        "api" => Some("API"),
        _ => None,
    };
    if let Some(label) = by_kind {
        return label.into();
    }
    if let Some(model) = kind.strip_prefix("7d:") {
        let mut chars = model.chars();
        let model = chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default();
        return format!("{model} sem.");
    }
    match minutes {
        Some(300) => "5 h".into(),
        Some(10080) => "Semana".into(),
        Some(43200) => "Mes".into(),
        Some(m) => span(m as i64 * 60_000),
        None => kind.into(),
    }
}

/// Un lapso corto: «45 min», «3 h», «4 d».
pub(crate) fn span(ms: i64) -> String {
    let min = (ms.max(0) as f64 / 60_000.0).round() as i64;
    if min < 90 {
        format!("{min} min")
    } else if min < 36 * 60 {
        format!("{} h", (min as f64 / 60.0).round() as i64)
    } else {
        format!("{} d", (min as f64 / 1440.0).round() as i64)
    }
}

fn tone(percent: f64) -> Hsla {
    gpui::rgb(if percent < 60.0 {
        OK
    } else if percent < 85.0 {
        WARN
    } else {
        HOT
    })
    .into()
}

/// Un arco desde arriba, en el sentido del reloj, como polilínea.
fn arc(cx: f32, cy: f32, r: f32, fraction: f32, width: f32) -> Option<gpui::Path<gpui::Pixels>> {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return None;
    }
    let steps = (fraction * 48.0).ceil().max(2.0) as usize;
    let mut builder = gpui::PathBuilder::stroke(px(width));
    for i in 0..=steps {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * fraction * i as f32 / steps as f32;
        let p = point(px(cx + r * a.cos()), px(cy + r * a.sin()));
        if i == 0 {
            builder.move_to(p);
        } else {
            builder.line_to(p);
        }
    }
    builder.build().ok()
}

/// Lo que cuelga de la franja del tab (bajo ella arriba, al lado de la
/// columna en un costado), de arriba abajo: el contenido va siempre de pie.
/// Cada uno empieza donde termina el anterior: si coinciden (el aviso de la
/// bandeja y el uso, al pasar sobre Agentes; el dictado con un aviso) se
/// apilan, no se tapan.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Drawer {
    /// El vistazo del portapapeles (`peek` en `main.rs`).
    Clipboard,
    /// El aviso de la bandeja de agentes (`tray.rs`).
    Tray,
    /// La letra de la música (`hang.rs`).
    Lyrics,
    /// El dictado (`dictation.rs`).
    Dictation,
    /// El uso de los agentes (este archivo).
    Usage,
    /// El vistazo de Color, Capturas, Textos, Flip o Sistema (`peeks.rs`).
    Tool,
}

/// Todas, en el orden en que se apilan.
const DRAWERS: [Drawer; 6] =
    [Drawer::Clipboard, Drawer::Tray, Drawer::Lyrics, Drawer::Dictation, Drawer::Usage, Drawer::Tool];

impl Pill {
    /// Cuánto ocupa cada franja ahora (su alto por cuánto bajó).
    fn drawer_height(&self, drawer: Drawer, now: Instant) -> f32 {
        match drawer {
            Drawer::Clipboard => self.peek_height() * self.peek.value(now).max(0.0),
            Drawer::Tray => {
                let (amount, height) = self.tray_stretch(now);
                height * amount.max(0.0)
            }
            Drawer::Lyrics => crate::hang::hang_height(self.side_drawers()) * self.hang_amount(now).max(0.0),
            // En un costado cabe igual, en una fila: el bloque (`SIDE_W`) es
            // más ancho que la franja de arriba (`DICT_W`).
            Drawer::Dictation => crate::dictation::DICT_H * self.dict_amount(now).max(0.0),
            Drawer::Usage => {
                let amount = self.usage_amount(now).max(0.0);
                if amount > 0.0 {
                    self.usage_height() * amount
                } else {
                    0.0
                }
            }
            Drawer::Tool => self.tool_peek_height(now) * self.tool_peek_amount(now).max(0.0),
        }
    }

    /// Dónde empieza una franja dentro de `drawers_area`: tras las de arriba.
    pub(crate) fn drawer_top(&self, drawer: Drawer, now: Instant) -> f32 {
        DRAWERS
            .into_iter()
            .take_while(|d| *d < drawer)
            .map(|d| self.drawer_height(d, now))
            .sum()
    }

    /// Lo que ocupan todas las franjas, una sobre otra.
    pub(crate) fn drawers_stack(&self, now: Instant) -> f32 {
        DRAWERS.into_iter().map(|d| self.drawer_height(d, now)).sum()
    }

    /// Donde cuelgan las franjas: arriba, bajo la franja del tab estirado; en
    /// un costado, el bloque al lado de la columna (`geometry::side_block`).
    pub(crate) fn drawers_area(&self, now: Instant) -> Option<Rect> {
        match self.shape(now) {
            PillShape::Tab { block: Some(block), .. } => Some(block),
            PillShape::Tab { edge, rect, .. } if !edge.is_vertical() => Some(edge.beyond_band(&rect, TAB_THICK)),
            _ => None,
        }
    }

    /// El rectángulo de una franja: tras la franja del tab y las de arriba.
    pub(crate) fn drawer_rect(&self, drawer: Drawer, now: Instant) -> Option<Rect> {
        let area = self.drawers_area(now)?;
        let h = self.drawer_height(drawer, now);
        (h > 4.0).then(|| Rect {
            x: area.x,
            y: area.y + self.drawer_top(drawer, now),
            w: area.w,
            h,
        })
    }

    /// Baja tras quedarse sobre Agentes; se queda mientras el cursor esté en
    /// la herramienta o en el vistazo. Otra herramienta o abrir un panel lo
    /// recogen.
    pub(crate) fn update_usage_peek(
        &mut self,
        now: Instant,
        cursor: Option<(f32, f32)>,
        strip_hovered: Option<usize>,
        docked: bool,
    ) {
        let on_tool = strip_hovered == Some(crate::AGENTES_TOOL) && !self.panel_visible() && self.docked_still();
        let relay = self.other_peek_open(crate::peeks::PeekKind::Usage);
        // El aviso de la bandeja se apila arriba del uso (los dos salen al pasar
        // sobre Agentes): estar en él también es estar en el vistazo. Si no,
        // ir a «Ver» cerraba el uso.
        let on_panel = cursor.is_some_and(|c| self.usage_over(c, now) || self.tray_over(c, now));
        if on_tool {
            // Apenas llega el cursor se piden los datos: al bajar ya están.
            self.usage.refresh();
            let since = *self.usage_hover_since.get_or_insert(now);
            // Con otro vistazo abierto, el relevo es rápido.
            let wait = if relay { crate::peeks::SWITCH_DELAY } else { HOVER_DELAY };
            if now.duration_since(since) >= wait {
                if self.usage_peek.target() != 1.0 {
                    self.usage.mark_opened();
                    self.hand_off_peeks(crate::peeks::PeekKind::Usage, now);
                }
                self.usage_peek.set(1.0, now);
            }
        } else {
            self.usage_hover_since = None;
        }
        // `PILL_OPEN=uso`: el vistazo queda fijo para revisarlo (sin esto se
        // cerraba con la tira y volvía a abrirse, reiniciando la animación).
        let open = std::env::var("PILL_OPEN").unwrap_or_default();
        let demo = open == "uso" || open == "uso-editar";
        if self.usage_peek.target() == 1.0 && !demo {
            // Sobre otra herramienta con vistazo se espera el relevo; sobre
            // una sin vistazo, el mismo margen que al salir.
            let relay_to = strip_hovered.is_some_and(|t| t != crate::AGENTES_TOOL && crate::peeks::any_peek(t));
            let closed = self.strip.target() == 0.0 || self.dictation_shown();
            if closed || self.panel_visible() || !docked || !self.docked_still() {
                self.close_usage_peek(now);
            } else if on_tool || on_panel || relay_to {
                self.usage_leave_at = None;
            } else {
                let since = *self.usage_leave_at.get_or_insert(now);
                if now.duration_since(since) >= LEAVE_GRACE {
                    self.usage_peek.set(0.0, now);
                    self.usage_leave_at = None;
                    self.usage.reset_view();
                }
            }
        }
        // `PILL_OPEN=uso` (o `uso-editar`, con el personalizador): abierto al
        // arrancar, para revisar el diseño.
        if demo && docked && self.docked_still() && !self.panel_visible() {
            self.usage.refresh();
            self.strip.set(1.0, now);
            if self.usage_peek.target() != 1.0 {
                self.usage.mark_opened();
                if open == "uso-editar" {
                    self.usage.set_editing(true);
                }
            }
            self.usage_peek.set(1.0, now);
        }
    }

    pub(crate) fn close_usage_peek(&mut self, now: Instant) {
        self.usage_peek.set(0.0, now);
        self.usage_leave_at = None;
        self.usage.reset_view();
    }

    /// Las animaciones del vistazo (anillos que se llenan, el detalle que
    /// baja, el alto) piden cuadros mientras duran.
    pub(crate) fn usage_animating(&self, now: Instant) -> bool {
        self.usage_peek.value(now) > 0.01 && self.usage.animating()
    }

    /// Cuánto bajó el vistazo de uso (0 a 1), en cualquier borde.
    pub(crate) fn usage_amount(&self, now: Instant) -> f32 {
        self.usage_peek.value(now).clamp(0.0, 1.2)
    }

    /// El ancho del tab con el vistazo: el de un panel.
    pub(crate) fn usage_length(length: f32, amount: f32) -> f32 {
        length + (PANEL_W.max(length) - length) * amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombra_las_ventanas_como_atic() {
        assert_eq!(window_label("5h", Some(300)), "5 h");
        assert_eq!(window_label("primary", Some(300)), "5 h");
        assert_eq!(window_label("secondary", Some(10080)), "Semana");
        assert_eq!(window_label("7d:gemini", None), "Gemini sem.");
        assert_eq!(window_label("primary", Some(120)), "2 h");
    }

    #[test]
    fn lee_las_preferencias() {
        assert_eq!(Prefs::parse(""), Prefs { simple: true, order: vec![], hidden: vec![] });
        let prefs = Prefs::parse("modo=detalle\norden=codex, claude,\nocultos=grok\n");
        assert!(!prefs.simple);
        assert_eq!(prefs.order, vec!["codex", "claude"]);
        assert_eq!(prefs.hidden, vec!["grok"]);
        // La versión anterior: lo no fijado queda fuera.
        let old = Prefs::parse("fijados=claude");
        assert!(old.hidden.contains(&"codex".to_string()) && !old.hidden.contains(&"claude".to_string()));
    }

    #[test]
    fn ordena_y_mueve_entre_filas() {
        let all = ["claude", "codex", "grok"];
        let mut prefs = Prefs { simple: true, order: vec![], hidden: vec![] };
        assert_eq!(prefs.split(&all), (vec!["claude", "codex", "grok"], vec![]));
        assert!(prefs.place(&all, "grok", false, 0));
        assert_eq!(prefs.split(&all).0, vec!["grok", "claude", "codex"]);
        assert!(prefs.place(&all, "claude", true, 0));
        assert_eq!(prefs.split(&all), (vec!["grok", "codex"], vec!["claude"]));
        assert!(prefs.place(&all, "codex", true, 9));
        // El último a la vista no se puede sacar.
        assert!(!prefs.place(&all, "grok", true, 0));
        assert_eq!(prefs.split(&all), (vec!["grok"], vec!["claude", "codex"]));
    }

    #[test]
    fn lapsos_cortos() {
        assert_eq!(span(45 * 60_000), "45 min");
        assert_eq!(span(3 * 3_600_000), "3 h");
        assert_eq!(span(4 * 86_400_000), "4 d");
    }
}
