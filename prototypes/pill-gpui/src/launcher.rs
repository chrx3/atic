//! Apps: el lanzador, como notch. La franja es el buscador; sin texto muestra
//! favoritos y recientes, con texto los resultados.
//!
//! Lo de Atic (`launcher.rs`, `LauncherFloat.svelte`): accesos del menú Inicio,
//! apps de Store por AUMID, acciones, el mismo puntaje (prefijo, contiene, un
//! error de tipeo, letras en orden) y la calculadora (`atic-calc`, con las
//! tasas de `fx-rates.json`). Lo que
//! cambia: Atic espera 120 ms por tecla y viaja por IPC; aquí se busca en el
//! mismo cuadro. Además Ctrl+1–9 abre directo y Ctrl+M lo pasa al centro de la
//! pantalla, tipo Spotlight.
//!
//! «:» con la barra vacía entra al modo emoji (el catálogo de Atic).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    actions, div, img, prelude::*, px, rgb, svg, uniform_list, App, ClickEvent, Context, Entity,
    EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, RenderImage, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window,
};
use serde::{Deserialize, Serialize};

use crate::hover::HoverExt;
use crate::clipboard::{BAND_H, PANEL_H};
use crate::running::{self, Running};
use crate::emoji;
use crate::text_input::{self, TextInput};

const ROW_H: f32 = 44.0;
const HEADER_H: f32 = 24.0;
const FAVORITES_H: f32 = 52.0;
const FOOTER_H: f32 = 26.0;
const PAD: f32 = 8.0;
const MARK_GAP: f32 = 40.0;
const SEARCH_LIMIT: usize = 24;
const RECENTS_SHOWN: usize = 6;
const FAVORITES_MAX: usize = 8;
/// Un índice más viejo que esto se rehace en segundo plano al abrir.
const REINDEX_AFTER: Duration = Duration::from_secs(60);

// --- El índice ----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Capture,
    Board,
    Color,
    Clipboard,
    Textos,
    Flip,
    Lock,
    Sleep,
    Mute,
    Trash,
}

#[derive(Clone, Debug)]
pub enum Target {
    Path(PathBuf),
    Aumid(String),
    Action(Action),
    /// Resultado de la calculadora: se copia.
    Copy(String),
}

#[derive(Clone)]
pub struct Item {
    pub id: SharedString,
    pub title: SharedString,
    pub subtitle: SharedString,
    pub target: Target,
    pub is_app: bool,
    /// El `.exe` (en minúsculas) de un acceso directo: así se sabe si está
    /// abierto. Las apps de Store comparten proceso y no lo tienen.
    pub exe: Option<String>,
}

struct Index {
    items: Arc<Vec<Item>>,
    built: Option<Instant>,
    building: bool,
}

fn index() -> &'static Mutex<Index> {
    static INDEX: OnceLock<Mutex<Index>> = OnceLock::new();
    INDEX.get_or_init(|| {
        Mutex::new(Index {
            items: Arc::new(actions_list()),
            built: None,
            building: false,
        })
    })
}

/// Rehace el índice en otro hilo si nunca se hizo o está viejo.
pub fn refresh_index() {
    {
        let mut guard = index().lock().unwrap();
        let fresh = guard.built.is_some_and(|at| at.elapsed() < REINDEX_AFTER);
        if fresh || guard.building {
            return;
        }
        guard.building = true;
    }
    std::thread::spawn(|| {
        let started = Instant::now();
        let mut items = actions_list();
        let mut apps = start_menu_apps();
        let extra = crate::app_icon::with_com(apps_folder);
        let mut seen: HashSet<String> = apps.iter().map(|item| fold(&item.title)).collect();
        apps.extend(extra.into_iter().filter(|item| seen.insert(fold(&item.title))));
        apps.sort_by_key(|item| item.title.to_lowercase());
        items.extend(apps);
        let count = items.len();
        let mut guard = index().lock().unwrap();
        guard.items = Arc::new(items);
        guard.built = Some(Instant::now());
        guard.building = false;
        println!("lanzador: {count} entradas en {} ms", started.elapsed().as_millis());
    });
}

fn items() -> Arc<Vec<Item>> {
    index().lock().unwrap().items.clone()
}

fn actions_list() -> Vec<Item> {
    [
        ("action:capture", "Capturar pantalla", "Ventana, región o monitor", Action::Capture),
        ("action:board", "Dibujar en pantalla", "Congelar la pantalla y marcarla", Action::Board),
        ("action:color", "Elegir color", "Cuentagotas: un píxel al portapapeles", Action::Color),
        ("action:clipboard", "Historial de clipboard", "Lo último que copiaste", Action::Clipboard),
        ("action:snippets", "Textos guardados", "Fragmentos y bloc", Action::Textos),
        ("action:flip", "Flip", "Notas por app", Action::Flip),
        ("action:sys-lock", "Bloquear pantalla", "Cerrar la sesión y pedir la contraseña", Action::Lock),
        ("action:sys-sleep", "Suspender", "Suspender el equipo", Action::Sleep),
        ("action:sys-mute", "Silenciar o activar sonido", "Alternar el silencio del audio", Action::Mute),
        ("action:sys-trash", "Vaciar papelera", "Pide confirmación: no se puede deshacer", Action::Trash),
    ]
    .into_iter()
    .map(|(id, title, subtitle, action)| Item {
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        target: Target::Action(action),
        is_app: false,
        exe: None,
    })
    .collect()
}

fn should_skip(name: &str) -> bool {
    let n = fold(name);
    n.starts_with("uninstall") || n.starts_with("desinstalar") || n.contains("uninstall ")
}

/// Los accesos directos del menú Inicio (usuario y todos).
fn start_menu_apps() -> Vec<Item> {
    let mut roots = Vec::new();
    for var in ["APPDATA", "ProgramData"] {
        if let Some(base) = std::env::var_os(var) {
            roots.push(PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if should_skip(stem) || !seen.insert(fold(stem)) {
                continue;
            }
            let exe = crate::app_icon::resolve_link(&path)
                .map(|target| target.to_string_lossy().to_lowercase());
            out.push(Item {
                id: format!("app:{}", path.to_string_lossy()).into(),
                title: stem.to_string().into(),
                subtitle: "Aplicación".into(),
                target: Target::Path(path),
                is_app: true,
                exe,
            });
        }
    }
    out
}

/// Las apps de `shell:AppsFolder` sin acceso directo (Store: Calculadora,
/// Terminal, Outlook nuevo…). Se lanzan por AppUserModelID.
#[cfg(windows)]
fn apps_folder() -> Vec<Item> {
    use windows::core::Interface;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx};
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2,
        SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
    };

    let take = |p: windows::core::Result<windows::core::PWSTR>| -> Option<String> {
        let p = p.ok()?;
        if p.is_null() {
            return None;
        }
        unsafe {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.as_ptr() as _));
            s
        }
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
        else {
            return out;
        };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None::<&IBindCtx>, &BHID_EnumItems)
        else {
            return out;
        };
        loop {
            let mut slot = [None::<IShellItem>];
            let mut fetched = 0u32;
            if items.Next(&mut slot, Some(&raw mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else {
                break;
            };
            let Some(title) = take(item.GetDisplayName(SIGDN_NORMALDISPLAY)) else {
                continue;
            };
            let Ok(item2) = item.cast::<IShellItem2>() else {
                continue;
            };
            let Some(aumid) = take(item2.GetString(&PKEY_AppUserModel_ID)) else {
                continue;
            };
            let (title, aumid) = (title.trim().to_string(), aumid.trim().to_string());
            if title.is_empty()
                || aumid.is_empty()
                || aumid.starts_with('\\')
                || aumid.chars().any(char::is_control)
                || should_skip(&title)
            {
                continue;
            }
            out.push(Item {
                id: format!("uwp:{aumid}").into(),
                title: title.into(),
                subtitle: "Aplicación".into(),
                target: Target::Aumid(aumid),
                is_app: true,
                exe: None,
            });
        }
    }
    out
}

#[cfg(not(windows))]
fn apps_folder() -> Vec<Item> {
    Vec::new()
}

// --- El puntaje (de `launcher.rs` de Atic) ----------------------------------------

pub fn fold(s: &str) -> String {
    crate::clipboard::fold(s)
}

/// Distancia de edición ≤ 1, con trasposición de vecinas («chorme»).
fn within_one_edit(a: &[char], b: &[char]) -> bool {
    match a.len().abs_diff(b.len()) {
        0 => {
            let mismatches: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).take(3).collect();
            match mismatches.as_slice() {
                [] | [_] => true,
                [i, j] => *j == i + 1 && a[*i] == b[*j] && a[*j] == b[*i],
                _ => false,
            }
        }
        1 => {
            let (longer, shorter) = if a.len() > b.len() { (a, b) } else { (b, a) };
            let (mut i, mut j, mut skipped) = (0, 0, false);
            while i < longer.len() && j < shorter.len() {
                if longer[i] == shorter[j] {
                    i += 1;
                    j += 1;
                } else if !skipped {
                    skipped = true;
                    i += 1;
                } else {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn is_subsequence(query: &[char], haystack: &[char]) -> bool {
    let mut qi = 0;
    for &ch in haystack {
        if qi < query.len() && ch == query[qi] {
            qi += 1;
        }
    }
    !query.is_empty() && qi == query.len()
}

fn fuzzy_window(query: &[char], haystack: &[char]) -> bool {
    let qn = query.len();
    // Con una letra el fuzzy inundaría los resultados.
    if qn < 2 {
        return false;
    }
    let min_w = (qn - 1).max(1);
    let max_w = (qn + 1).min(haystack.len());
    (min_w..=max_w).any(|w| {
        haystack.len() >= w
            && (0..=haystack.len() - w).any(|start| within_one_edit(query, &haystack[start..start + w]))
    })
}

pub fn score(query: &str, haystack: &str) -> Option<u32> {
    let q = fold(query);
    if q.is_empty() {
        return None;
    }
    let h = fold(haystack);
    if h.starts_with(&q) {
        return Some(100);
    }
    if h.contains(&q) {
        return Some(50);
    }
    let (qc, hc): (Vec<char>, Vec<char>) = (q.chars().collect(), h.chars().collect());
    if fuzzy_window(&qc, &hc) {
        return Some(40);
    }
    is_subsequence(&qc, &hc).then_some(28)
}

/// Las apps por título. Las acciones, por título con un empujón, y por
/// subtítulo solo si lo contiene tal cual: el difuso sobre frases largas
/// («Cerrar la sesión…») hacía que «chr» pusiera «Bloquear pantalla» sobre
/// Chrome.
fn item_score(query: &str, item: &Item) -> Option<u32> {
    if item.is_app {
        return score(query, &item.title);
    }
    let title = score(query, &item.title).map(|s| s + 15);
    let q = fold(query);
    let subtitle = (q.chars().count() >= 3 && fold(&item.subtitle).contains(&q)).then_some(30);
    title.max(subtitle)
}

// --- Divisas -------------------------------------------------------------------

/// Las tasas de `atic_calc::fx` (`fx-rates.json`), solo con el conversor
/// encendido en Ajustes (`launcher_currency`), como en Atic. Si la tabla quedó
/// vieja se refresca en segundo plano; la búsqueda nunca espera a la red.
fn load_rates() -> Option<Arc<atic_calc::fx::Rates>> {
    let dirs = atic_core::AppDirs::new().ok()?;
    if !atic_core::Config::load(&dirs.config_path()).launcher_currency {
        return None;
    }
    let path = dirs.fx_rates_path();
    if atic_calc::fx::snapshot().is_none() {
        atic_calc::fx::init(&path, true);
    } else {
        atic_calc::fx::refresh_if_stale(path);
    }
    atic_calc::fx::snapshot()
}


// El modo emoji (catálogo, búsqueda, secciones y tonos) vive en `emoji.rs`.

// --- Favoritos y recientes ----------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    favorites: Vec<String>,
    /// Id y cuándo se abrió (ms), del más nuevo al más viejo.
    #[serde(default)]
    recents: Vec<(String, u64)>,
    /// Ctrl+M: en el centro de la pantalla, tipo Spotlight, en vez del notch.
    #[serde(default)]
    centered: bool,
    /// Modo emoji: los últimos usados (sin tono) y el tono elegido.
    #[serde(default)]
    emoji_recents: Vec<String>,
    #[serde(default)]
    emoji_tone: u8,
}

fn store_file() -> Option<PathBuf> {
    crate::paths::file("launcher.json")
}

impl Store {
    fn load() -> Self {
        store_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let (Some(path), Ok(raw)) = (store_file(), serde_json::to_string_pretty(self)) {
            let _ = std::fs::write(path, raw);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn ago(ms: u64) -> String {
    let minutes = now_ms().saturating_sub(ms) / 60_000;
    match minutes {
        0 => "usada recién".into(),
        1..=59 => format!("usada hace {minutes} min"),
        60..=1439 => format!("usada hace {} h", minutes / 60),
        _ => format!("usada hace {} d", minutes / 1440),
    }
}

// --- El panel -----------------------------------------------------------------

actions!(
    launcher,
    [
        SelectPrev, SelectNext, Run, Dismiss, ToggleFavorite, QuitOrCopy, ToggleCentered,
        Quick1, Quick2, Quick3, Quick4, Quick5, Quick6, Quick7, Quick8, Quick9
    ]
);

const KEY_CONTEXT: &str = "Launcher";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("enter", Run, context),
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("ctrl-d", ToggleFavorite, context),
        KeyBinding::new("ctrl-enter", QuitOrCopy, context),
        KeyBinding::new("ctrl-m", ToggleCentered, context),
        KeyBinding::new("ctrl-1", Quick1, context),
        KeyBinding::new("ctrl-2", Quick2, context),
        KeyBinding::new("ctrl-3", Quick3, context),
        KeyBinding::new("ctrl-4", Quick4, context),
        KeyBinding::new("ctrl-5", Quick5, context),
        KeyBinding::new("ctrl-6", Quick6, context),
        KeyBinding::new("ctrl-7", Quick7, context),
        KeyBinding::new("ctrl-8", Quick8, context),
        KeyBinding::new("ctrl-9", Quick9, context),
    ]);
}

pub enum LauncherEvent {
    Run(Target),
    /// Un emoji (o texto) a pegar en la app de atrás.
    Paste(String),
    Close,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Search,
    Emoji,
}

const EMOJI_COLS: usize = 8;
const EMOJI_ROWS_SHOWN: usize = 6;
/// Las categorías y los tonos, bajo el buscador.
const EMOJI_BAR_H: f32 = 38.0;

#[derive(Clone)]
struct Hit {
    item: Item,
    /// La etiqueta de la derecha: «usada hace 5 min», o vacía.
    note: SharedString,
}

pub struct LauncherPanel {
    search: Entity<TextInput>,
    hits: Vec<Hit>,
    favorites: Vec<Item>,
    /// Sin texto: la lista son los recientes, con su encabezado.
    showing_recents: bool,
    selected: usize,
    scroll: UniformListScrollHandle,
    store: Store,
    icons: Arc<Mutex<HashMap<SharedString, Option<Arc<RenderImage>>>>>,
    pending: HashSet<SharedString>,
    pub mark_gap: bool,
    /// El tope de alto del panel: en un costado, casi toda la pantalla (lo
    /// pone la pill); si no, el de siempre.
    pub max_height: Option<f32>,
    pub pinned: bool,
    running: Running,
    rates: Option<Arc<atic_calc::fx::Rates>>,
    /// Apps a las que se pidió cerrar, hasta el próximo vistazo.
    closing: HashSet<SharedString>,
    mode: Mode,
    /// Los emojis en el orden de la grilla (todas las secciones seguidas),
    /// las secciones y las filas para la lista.
    emoji_hits: Vec<usize>,
    emoji_sections: Vec<emoji::Section>,
    emoji_rows: Vec<emoji::Row>,
    emoji_selected: usize,
    emoji_scroll: UniformListScrollHandle,
    _search_changed: Subscription,
}

impl EventEmitter<LauncherEvent> for LauncherPanel {}

impl Focusable for LauncherPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl LauncherPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let text: Hsla = rgb(0xf0f0ea).into();
        let search = cx.new(|cx| {
            TextInput::new(
                "Buscar apps, acciones o calcular…",
                text,
                rgb(0x9a9a90).into(),
                text,
                cx,
            )
        });
        let search_changed = cx.subscribe(&search, |panel, _, _: &text_input::Changed, cx| {
            panel.refresh(cx);
        });
        refresh_index();
        let mut panel = Self {
            search,
            hits: Vec::new(),
            favorites: Vec::new(),
            showing_recents: true,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            store: Store::load(),
            icons: Arc::new(Mutex::new(HashMap::new())),
            pending: HashSet::new(),
            mark_gap: true,
            max_height: None,
            pinned: false,
            running: running::scan(),
            rates: load_rates(),
            closing: HashSet::new(),
            mode: Mode::Search,
            emoji_hits: Vec::new(),
            emoji_sections: Vec::new(),
            emoji_rows: Vec::new(),
            emoji_selected: 0,
            emoji_scroll: UniformListScrollHandle::new(),
            _search_changed: search_changed,
        };
        panel.refresh(cx);
        panel
    }

    /// Al abrir: buscador vacío, recientes arriba, índice al día.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        refresh_index();
        self.running = running::scan();
        self.rates = load_rates();
        self.closing.clear();
        self.set_mode(Mode::Search, cx);
        self.search.update(cx, |search, cx| search.clear(cx));
        self.refresh(cx);
    }

    /// Abrir directo en el modo emoji (`PILL_OPEN=emoji`).
    pub fn open_emoji(&mut self, cx: &mut Context<Self>) {
        self.set_mode(Mode::Emoji, cx);
        self.search.update(cx, |search, cx| search.clear(cx));
        self.refresh(cx);
    }

    pub fn centered(&self) -> bool {
        self.store.centered
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        let emoji = mode == Mode::Emoji;
        self.search.update(cx, |search, cx| {
            search.pass_edges = emoji;
            search.set_placeholder(
                if emoji {
                    "Buscar emoji… (Backspace vuelve)"
                } else {
                    "Buscar apps, acciones o calcular…"
                },
                cx,
            );
        });
    }

    /// La etiqueta de una app: abierta, cerrándose o cuándo se usó.
    fn note_for(&self, item: &Item, recent: Option<u64>) -> SharedString {
        if self.closing.contains(&item.id) {
            return "Cerrando…".into();
        }
        if let Some(exe) = &item.exe {
            if self.running.is_foreground(exe) {
                return "Al frente".into();
            }
            if self.running.is_running(exe) {
                return "En uso".into();
            }
        }
        recent.map(|at| ago(at).into()).unwrap_or_default()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let raw = self.search.read(cx).text().to_string();
        if self.mode == Mode::Search && raw == ":" {
            self.set_mode(Mode::Emoji, cx);
            self.search.update(cx, |search, cx| search.clear(cx));
            return;
        }
        if self.mode == Mode::Emoji {
            let sections = emoji::sections(&raw, &self.store.emoji_recents);
            self.emoji_hits = sections.iter().flat_map(|s| s.items.iter().copied()).collect();
            self.emoji_rows = emoji::rows(&sections, EMOJI_COLS);
            self.emoji_sections = sections;
            self.emoji_selected = 0;
            self.emoji_scroll.scroll_to_item(0, ScrollStrategy::Top);
            cx.notify();
            return;
        }
        let query = raw.trim().to_string();
        let all = items();
        let by_id = |id: &str| all.iter().find(|item| item.id.as_ref() == id).cloned();
        self.favorites = self.store.favorites.iter().filter_map(|id| by_id(id)).collect();
        self.showing_recents = query.is_empty();
        self.hits = if query.is_empty() {
            self.store
                .recents
                .iter()
                .filter_map(|(id, at)| {
                    by_id(id).map(|item| Hit {
                        note: self.note_for(&item, Some(*at)),
                        item,
                    })
                })
                .take(RECENTS_SHOWN)
                .collect()
        } else {
            let mut hits = Vec::new();
            // La calculadora va primero y se lleva el Enter.
            let rates = self
                .rates
                .as_deref()
                .map(|rates| rates as &dyn crate::calc::RatesLookup);
            if let Some(result) = crate::calc::evaluate_with(&query, rates, crate::calc::Locale::Es) {
                let subtitle = match &result.source {
                    Some(source) => format!("{source} · Enter para copiar"),
                    None => "Enter para copiar".to_string(),
                };
                hits.push(Hit {
                    item: Item {
                        id: format!("calc:{query}").into(),
                        title: result.value.clone().into(),
                        subtitle: subtitle.into(),
                        target: Target::Copy(result.value),
                        is_app: false,
                        exe: None,
                    },
                    note: "= calculadora".into(),
                });
            }
            let mut scored: Vec<(u32, &Item)> = all
                .iter()
                .filter_map(|item| item_score(&query, item).map(|s| (s, item)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
            hits.extend(scored.into_iter().take(SEARCH_LIMIT).map(|(_, item)| Hit {
                note: self.note_for(item, None),
                item: item.clone(),
            }));
            hits
        };
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    pub fn desired_height(&self) -> f32 {
        if self.mode == Mode::Emoji {
            let rows = self.emoji_rows.len().clamp(1, EMOJI_ROWS_SHOWN);
            return (BAND_H + EMOJI_BAR_H + rows as f32 * ROW_H + FOOTER_H + PAD).min(self.max_height.unwrap_or(PANEL_H));
        }
        let favorites = if self.favorites.is_empty() { 0.0 } else { FAVORITES_H };
        let header = if self.showing_recents && !self.hits.is_empty() {
            HEADER_H
        } else {
            0.0
        };
        let rows = if self.hits.is_empty() {
            ROW_H
        } else {
            self.hits.len() as f32 * ROW_H
        };
        (BAND_H + favorites + header + rows + FOOTER_H + PAD).min(self.max_height.unwrap_or(PANEL_H))
    }

    fn icon(&mut self, item: &Item, cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        if let Some(hit) = self.icons.lock().unwrap().get(&item.id) {
            return hit.clone();
        }
        // Resolver un ícono puede tardar (un .lnk a una unidad de red): en otro
        // hilo, y la fila se repinta cuando llega.
        if self.pending.insert(item.id.clone()) {
            let (id, target, icons) = (item.id.clone(), item.target.clone(), self.icons.clone());
            cx.spawn(async move |this, cx| {
                let icon = cx
                    .background_spawn(async move {
                        match target {
                            Target::Path(path) => crate::app_icon::icon_for(&path),
                            Target::Aumid(aumid) => crate::app_icon::icon_for_aumid(&aumid),
                            _ => None,
                        }
                    })
                    .await;
                icons.lock().unwrap().insert(id, icon);
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        None
    }

    fn run_hit(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(hit) = self.hits.get(index).cloned() else {
            return;
        };
        self.run_item(&hit.item, cx);
    }

    fn run_item(&mut self, item: &Item, cx: &mut Context<Self>) {
        if !matches!(item.target, Target::Copy(_)) {
            let id = item.id.to_string();
            self.store.recents.retain(|(other, _)| *other != id);
            self.store.recents.insert(0, (id, now_ms()));
            self.store.recents.truncate(30);
            self.store.save();
        }
        cx.emit(LauncherEvent::Run(item.target.clone()));
    }

    /// Ctrl+Enter: en una app abierta, pedirle que cierre (como el aspa); en
    /// el modo emoji, copiar sin pegar.
    fn quit_or_copy(&mut self, _: &QuitOrCopy, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Emoji {
            self.pick_emoji(self.emoji_selected, true, cx);
            return;
        }
        let Some(hit) = self.hits.get(self.selected).cloned() else {
            return;
        };
        let Some(windows) = hit.item.exe.as_ref().and_then(|exe| self.running.windows.get(exe))
        else {
            return;
        };
        let asked = running::close(windows);
        println!("lanzador: cerrar {} ({asked} ventanas)", hit.item.title);
        self.closing.insert(hit.item.id.clone());
        let selected = self.selected;
        self.reannotate(selected, cx);
        // Un momento después se vuelve a mirar qué quedó abierto.
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(900)).await;
            let _ = this.update(cx, |panel, cx| {
                panel.running = running::scan();
                panel.closing.clear();
                panel.reannotate(selected, cx);
            });
        })
        .detach();
    }

    /// Recalcula las etiquetas sin perder la búsqueda ni la selección.
    fn reannotate(&mut self, selected: usize, cx: &mut Context<Self>) {
        let notes: Vec<SharedString> = self
            .hits
            .iter()
            .map(|hit| {
                let recent = self
                    .store
                    .recents
                    .iter()
                    .find(|(id, _)| *id == hit.item.id.as_ref())
                    .map(|(_, at)| *at)
                    .filter(|_| self.showing_recents);
                self.note_for(&hit.item, recent)
            })
            .collect();
        for (hit, note) in self.hits.iter_mut().zip(notes) {
            hit.note = note;
        }
        self.selected = selected.min(self.hits.len().saturating_sub(1));
        cx.notify();
    }

    fn toggle_centered(&mut self, _: &ToggleCentered, _: &mut Window, cx: &mut Context<Self>) {
        self.store.centered = !self.store.centered;
        self.store.save();
        cx.notify();
    }

    /// Flechas en la grilla: entre secciones conservan la columna.
    fn emoji_move(&mut self, key: emoji::Key, cx: &mut Context<Self>) {
        if self.emoji_hits.is_empty() {
            return;
        }
        let sizes: Vec<usize> = self.emoji_sections.iter().map(|s| s.items.len()).collect();
        self.emoji_selected = emoji::move_in_grid(&sizes, self.emoji_selected, key, EMOJI_COLS);
        if let Some(row) = self.emoji_row_of(self.emoji_selected) {
            self.emoji_scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// La fila de la lista donde está una posición de la grilla.
    fn emoji_row_of(&self, slot: usize) -> Option<usize> {
        self.emoji_rows.iter().position(|row| {
            matches!(row, emoji::Row::Cells { start, len } if (*start..start + len).contains(&slot))
        })
    }

    /// El emoji de una posición, con el tono elegido.
    fn emoji_char(&self, slot: usize) -> Option<SharedString> {
        let &i = self.emoji_hits.get(slot)?;
        Some(emoji::all()[i].with_skin(self.store.emoji_tone))
    }

    /// Pegar (o copiar) un emoji; queda primero en los recientes.
    fn pick_emoji(&mut self, slot: usize, copy: bool, cx: &mut Context<Self>) {
        let (Some(&i), Some(ch)) = (self.emoji_hits.get(slot), self.emoji_char(slot)) else {
            return;
        };
        emoji::push_recent(&mut self.store.emoji_recents, &emoji::all()[i].ch);
        self.store.save();
        if copy {
            cx.emit(LauncherEvent::Run(Target::Copy(ch.to_string())));
        } else {
            cx.emit(LauncherEvent::Paste(ch.to_string()));
        }
    }

    /// Un chip de categoría: la lista salta a su título.
    fn jump_to_group(&mut self, group: u8, cx: &mut Context<Self>) {
        let mut start = 0;
        for section in &self.emoji_sections {
            if section.group == Some(group) {
                self.emoji_selected = start;
                let title = self.emoji_row_of(start).map_or(0, |row| row.saturating_sub(1));
                self.emoji_scroll.scroll_to_item(title, ScrollStrategy::Top);
                cx.notify();
                return;
            }
            start += section.items.len();
        }
    }

    fn set_tone(&mut self, tone: u8, cx: &mut Context<Self>) {
        self.store.emoji_tone = tone;
        self.store.save();
        cx.notify();
    }

    /// La sección donde está la selección (para marcar su chip).
    fn emoji_group_selected(&self) -> Option<u8> {
        let mut start = 0;
        for section in &self.emoji_sections {
            if (start..start + section.items.len()).contains(&self.emoji_selected) {
                return section.group;
            }
            start += section.items.len();
        }
        None
    }

    /// Bajo el buscador: los chips de categoría (sin búsqueda) y los tonos.
    fn render_emoji_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let searching = self.emoji_sections.iter().all(|s| s.title.is_none());
        let active = self.emoji_group_selected();
        let tone = self.store.emoji_tone;
        div()
            .h(px(EMOJI_BAR_H))
            .flex_none()
            .px(px(PAD + 4.))
            .flex()
            .items_center()
            .gap(px(2.))
            .when(!searching, |el| {
                el.children(emoji::GROUPS.iter().map(|&(group, icon, _)| {
                    let on = active == Some(group);
                    div()
                        .id(("emoji-group", group as usize))
                        .size(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(9.))
                        .cursor_pointer()
                        .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.jump_to_group(group, cx)))
                        .child(icon)
                        .fx(("emoji-group-fx", group as usize), move |el, h| {
                            let (rest, over) = if on { (0.14, 0.16) } else { (0.0, 0.08) };
                            el.bg(h.mix(text.opacity(rest), text.opacity(over)))
                                .opacity(if on { 1.0 } else { 0.75 + 0.25 * h.t })
                                .text_size(px(16. + 1.5 * h.t - 1.5 * h.press))
                        })
                }))
            })
            .child(div().flex_1())
            // El tono de piel: la mano en sus seis colores.
            .children(emoji::TONES.iter().enumerate().map(|(i, hand)| {
                let on = tone as usize == i;
                div()
                    .id(("emoji-tone", i))
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(13.))
                    .when(on, |el| el.bg(text.opacity(0.16)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.set_tone(i as u8, cx)))
                    .child(*hand)
                    .fx(("emoji-tone-fx", i), move |el, h| {
                        el.opacity(if on { 1.0 } else { 0.6 + 0.4 * h.t })
                            .text_size(px(14. + 1.5 * h.t - 1.5 * h.press))
                    })
            }))
    }

    fn render_emoji_row(&mut self, row: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let muted: Hsla = rgb(0x8a8a82).into();
        let (start, len) = match self.emoji_rows.get(row) {
            Some(emoji::Row::Cells { start, len }) => (*start, *len),
            Some(emoji::Row::Title(title)) => {
                return div()
                    .w_full()
                    .h(px(ROW_H))
                    .px(px(PAD + 8.))
                    .pb(px(4.))
                    .flex()
                    .items_end()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(*title)
                    .into_any_element();
            }
            None => return div().into_any_element(),
        };
        let cells: Vec<_> = (start..start + len)
            .map(|slot| {
                let selected = slot == self.emoji_selected;
                let ch = self.emoji_char(slot).unwrap_or_default();
                div()
                    .id(("emoji", slot))
                    .flex_1()
                    .h(px(ROW_H - 4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .cursor_pointer()
                    .child(ch)
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.pick_emoji(slot, false, cx)))
                    .fx(("emoji-fx", slot), move |el, h| {
                        // El emoji crece un poco con el cursor y se achica al
                        // apretarlo (`.lf-emoji:active` en la web).
                        let rest = if selected { 0.14 } else { 0.0 };
                        el.bg(text.opacity(rest + (0.08_f32.max(rest) - rest) * h.over))
                            .text_size(px(22. + 2. * h.over - 3. * h.press))
                    })
                    .into_any_element()
            })
            .collect();
        // Fila incompleta: celdas vacías para que no se estiren.
        let fillers = (len..EMOJI_COLS).map(|_| div().flex_1().into_any_element());
        div()
            .w_full()
            .h(px(ROW_H))
            .px(px(PAD))
            .flex()
            .gap(px(2.))
            .children(cells)
            .children(fillers)
            .into_any_element()
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.hits.is_empty() {
            return;
        }
        self.selected = index % self.hits.len();
        self.scroll.scroll_to_item(self.selected, ScrollStrategy::Center);
        cx.notify();
    }

    fn toggle_favorite(&mut self, _: &ToggleFavorite, _: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.hits.get(self.selected) else {
            return;
        };
        if matches!(hit.item.target, Target::Copy(_)) {
            return;
        }
        let id = hit.item.id.to_string();
        if let Some(pos) = self.store.favorites.iter().position(|f| *f == id) {
            self.store.favorites.remove(pos);
        } else {
            self.store.favorites.push(id);
            if self.store.favorites.len() > FAVORITES_MAX {
                self.store.favorites.remove(0);
            }
        }
        self.store.save();
        let selected = self.selected;
        self.refresh(cx);
        self.select(selected, cx);
    }

    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let hit = self.hits[index].clone();
        let icon = self.icon(&hit.item, cx);
        let text: Hsla = rgb(0xf0f0ea).into();
        let muted: Hsla = rgb(0x9a9a90).into();
        let faint: Hsla = rgb(0x6e6e66).into();
        let selected = index == self.selected;
        let favorite = self.store.favorites.iter().any(|f| *f == hit.item.id.as_ref());
        let glyph = match hit.item.target {
            Target::Copy(_) => "icons/type.svg",
            Target::Action(Action::Capture) => "icons/crop.svg",
            Target::Action(Action::Board) => "icons/pencil.svg",
            Target::Action(Action::Color) => "icons/pipette.svg",
            Target::Action(Action::Clipboard) => "icons/clipboard.svg",
            Target::Action(Action::Textos) => "icons/text-align-start.svg",
            Target::Action(Action::Trash) => "icons/trash.svg",
            Target::Action(Action::Lock) => "icons/lock.svg",
            _ => "icons/circle-dot.svg",
        };
        let leading = match icon {
            Some(icon) => img(icon).size(px(24.)).into_any_element(),
            None => svg()
                .path(glyph)
                .size(px(16.))
                .text_color(muted)
                .into_any_element(),
        };
        div()
            .w_full()
            .px(px(PAD))
            .child(
                div()
                    .id(("launcher-row", index))
                    .w_full()
                    .h(px(ROW_H))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.run_hit(index, cx)))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .when(!hit.item.is_app, |el| el.bg(text.opacity(0.07)))
                            .child(leading),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(13.)).truncate().child(hit.item.title.clone()))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(muted)
                                    .truncate()
                                    .child(hit.item.subtitle.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(10.))
                            .text_color(faint)
                            .when(favorite, |el| {
                                el.child(svg().path("icons/star.svg").size(px(11.)).text_color(muted))
                            })
                            .child(hit.note.clone())
                            .when(index < 9 && !selected, |el| el.child(format!("Ctrl+{}", index + 1)))
                            .when(selected, |el| el.child(div().text_color(text).child("↵"))),
                    )
                    .fx(("launcher-row-fx", index), move |el, h| {
                        el.bg(h.mix(text.opacity(0.0), text.opacity(0.09)))
                    })
                    .lit(selected),
            )
    }
}

impl Render for LauncherPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text: Hsla = rgb(0xf0f0ea).into();
        let muted: Hsla = rgb(0x9a9a90).into();
        let faint: Hsla = rgb(0x6e6e66).into();

        let band = div()
            .h(px(BAND_H))
            .flex_none()
            .flex()
            .items_center()
            .pr(px(14.))
            .when(self.mark_gap, |el| {
                el.child(
                    div()
                        .id("launcher-mark")
                        .w(px(MARK_GAP))
                        .h_full()
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(LauncherEvent::Close))),
                )
            })
            .when(!self.mark_gap, |el| {
                el.pl(px(14.)).child(
                    svg()
                        .path("icons/search.svg")
                        .size(px(13.))
                        .mr(px(6.))
                        .text_color(faint),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .child(self.search.clone()),
            );

        let favorites: Vec<_> = self
            .favorites
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                let icon = self.icon(&item, cx);
                div()
                    .id(("launcher-fav", i))
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .on_click({
                        let item = item.clone();
                        cx.listener(move |panel, _: &ClickEvent, _, cx| panel.run_item(&item, cx))
                    })
                    .fx(("launcher-fav-fx", i), move |el, h| {
                        // Como los favoritos de la web: el ícono crece un
                        // poco con el cursor y se hunde al apretar.
                        let size = 26. + 2. * h.t - 2.5 * h.press;
                        el.bg(h.mix(text.opacity(0.06), text.opacity(0.12))).child(match icon {
                            Some(icon) => img(icon).size(px(size)).into_any_element(),
                            None => div()
                                .text_size(px(13. + h.t))
                                .child(item.title.chars().next().unwrap_or('·').to_string())
                                .into_any_element(),
                        })
                    })
                    .into_any_element()
            })
            .collect();

        let emoji_mode = self.mode == Mode::Emoji;
        let body = if emoji_mode {
            if self.emoji_hits.is_empty() {
                div()
                    .h(px(ROW_H))
                    .px(px(PAD + 10.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("Ningún emoji con ese nombre")
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(self.render_emoji_bar(cx))
                    .child(
                        uniform_list(
                            "launcher-emoji",
                            self.emoji_rows.len(),
                            cx.processor(|panel, range: std::ops::Range<usize>, _, cx| {
                                range.map(|row| panel.render_emoji_row(row, cx)).collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(self.emoji_scroll.clone())
                        .w_full()
                        .flex_1(),
                    )
                    .into_any_element()
            }
        } else if self.hits.is_empty() {
            div()
                .h(px(ROW_H))
                .px(px(PAD + 10.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .text_color(muted)
                .child(if self.showing_recents {
                    "Escribe para buscar · Ctrl+D marca favoritos"
                } else {
                    "Sin resultados"
                })
                .into_any_element()
        } else {
            uniform_list(
                "launcher-hits",
                self.hits.len(),
                cx.processor(|panel, range: std::ops::Range<usize>, _, cx| {
                    range.map(|i| panel.render_row(i, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .w_full()
            .flex_1()
            .into_any_element()
        };

        let footer = div()
            .h(px(FOOTER_H))
            .flex_none()
            .flex()
            .items_center()
            .px(px(PAD + 10.))
            .text_size(px(10.))
            .text_color(faint)
            .child(if emoji_mode {
                let name = self
                    .emoji_hits
                    .get(self.emoji_selected)
                    .map(|&i| {
                        let ch = self.emoji_char(self.emoji_selected).unwrap_or_default();
                        format!("{ch} {} · ", emoji::all()[i].name)
                    })
                    .unwrap_or_default();
                format!("{name}Enter pegar · Ctrl+Enter copiar · Esc volver")
            } else {
                let quit = self
                    .hits
                    .get(self.selected)
                    .and_then(|hit| hit.item.exe.as_ref())
                    .is_some_and(|exe| self.running.is_running(exe));
                format!(
                    "Enter abrir · Ctrl+1–9 directo{} · Ctrl+D favorito · : emoji · Ctrl+M {}",
                    if quit { " · Ctrl+Enter cerrar app" } else { "" },
                    if self.store.centered { "al notch" } else { "al centro" }
                )
            });

        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|p, _: &SelectPrev, _, cx| {
                if p.mode == Mode::Emoji {
                    return p.emoji_move(emoji::Key::Up, cx);
                }
                let n = p.hits.len().max(1);
                p.select(p.selected + n - 1, cx)
            }))
            .on_action(cx.listener(|p, _: &SelectNext, _, cx| {
                if p.mode == Mode::Emoji {
                    return p.emoji_move(emoji::Key::Down, cx);
                }
                p.select(p.selected + 1, cx)
            }))
            .on_action(cx.listener(|p, _: &Run, _, cx| {
                if p.mode == Mode::Emoji {
                    return p.pick_emoji(p.emoji_selected, false, cx);
                }
                p.run_hit(p.selected, cx)
            }))
            .on_action(cx.listener(|p, _: &Dismiss, _, cx| {
                // En el modo emoji, Esc vuelve a la búsqueda.
                if p.mode == Mode::Emoji {
                    p.set_mode(Mode::Search, cx);
                    p.search.update(cx, |search, cx| search.clear(cx));
                    return;
                }
                cx.emit(LauncherEvent::Close)
            }))
            .on_action(cx.listener(Self::quit_or_copy))
            .on_action(cx.listener(Self::toggle_centered))
            // Con `pass_edges`, el campo deja pasar ←→ y Backspace vacío.
            .on_action(cx.listener(|p, _: &text_input::Left, _, cx| p.emoji_move(emoji::Key::Left, cx)))
            .on_action(cx.listener(|p, _: &text_input::Right, _, cx| p.emoji_move(emoji::Key::Right, cx)))
            .on_action(cx.listener(|p, _: &text_input::Backspace, _, cx| {
                p.set_mode(Mode::Search, cx);
                p.refresh(cx);
            }))
            .on_action(cx.listener(Self::toggle_favorite))
            .on_action(cx.listener(|p, _: &Quick1, _, cx| p.run_hit(0, cx)))
            .on_action(cx.listener(|p, _: &Quick2, _, cx| p.run_hit(1, cx)))
            .on_action(cx.listener(|p, _: &Quick3, _, cx| p.run_hit(2, cx)))
            .on_action(cx.listener(|p, _: &Quick4, _, cx| p.run_hit(3, cx)))
            .on_action(cx.listener(|p, _: &Quick5, _, cx| p.run_hit(4, cx)))
            .on_action(cx.listener(|p, _: &Quick6, _, cx| p.run_hit(5, cx)))
            .on_action(cx.listener(|p, _: &Quick7, _, cx| p.run_hit(6, cx)))
            .on_action(cx.listener(|p, _: &Quick8, _, cx| p.run_hit(7, cx)))
            .on_action(cx.listener(|p, _: &Quick9, _, cx| p.run_hit(8, cx)))
            .size_full()
            .flex()
            .flex_col()
            .pb(px(PAD))
            .font_family("Segoe UI")
            .text_color(text)
            .child(band)
            .when(!favorites.is_empty() && !emoji_mode, |el| {
                el.child(
                    div()
                        .h(px(FAVORITES_H))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(PAD + 6.))
                        .children(favorites),
                )
            })
            .when(self.showing_recents && !self.hits.is_empty() && !emoji_mode, |el| {
                el.child(
                    div()
                        .h(px(HEADER_H))
                        .flex_none()
                        .flex()
                        .items_end()
                        .pb(px(4.))
                        .px(px(PAD + 10.))
                        .text_size(px(10.))
                        .text_color(faint)
                        .child("Recientes"),
                )
            })
            .child(body)
            .child(footer)
    }
}

// --- Abrir y acciones del sistema -------------------------------------------------

/// Abre una app: el acceso directo con el shell, o la de Store por AUMID.
pub fn launch(target: &Target) -> Result<(), String> {
    let file = match target {
        Target::Path(path) => path.to_string_lossy().into_owned(),
        Target::Aumid(aumid) => format!("shell:AppsFolder\\{aumid}"),
        _ => return Ok(()),
    };
    shell_open(&file)
}

#[cfg(windows)]
pub(crate) fn shell_open(file: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let result = crate::app_icon::with_com(|| unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(file), None, None, SW_SHOWNORMAL)
    });
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("no se pudo abrir ({})", result.0 as isize))
    }
}

#[cfg(not(windows))]
pub(crate) fn shell_open(_: &str) -> Result<(), String> {
    Err("solo Windows".into())
}

#[cfg(windows)]
pub fn system_action(action: Action) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, VK_VOLUME_MUTE,
    };
    match action {
        Action::Lock => unsafe {
            windows::Win32::System::Shutdown::LockWorkStation().map_err(|e| e.to_string())
        },
        Action::Sleep => unsafe {
            // false: suspender, no hibernar.
            windows::Win32::System::Power::SetSuspendState(false, false, false)
                .then_some(())
                .ok_or_else(|| "no se pudo suspender".to_string())
        },
        Action::Mute => unsafe {
            keybd_event(VK_VOLUME_MUTE.0 as u8, 0, Default::default(), 0);
            keybd_event(VK_VOLUME_MUTE.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            Ok(())
        },
        // Sin banderas: Windows pide confirmación antes de vaciar.
        Action::Trash => unsafe {
            windows::Win32::UI::Shell::SHEmptyRecycleBinW(None, None, 0).map_err(|e| e.to_string())
        },
        _ => Ok(()),
    }
}

#[cfg(not(windows))]
pub fn system_action(_: Action) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puntaje_como_atic() {
        assert_eq!(score("chr", "Google Chrome"), Some(50));
        assert_eq!(score("goo", "Google Chrome"), Some(100));
        assert_eq!(score("chorme", "Google Chrome"), Some(40));
        // «gc» queda a un error de «go»: 40, igual que en Atic.
        assert_eq!(score("gc", "Google Chrome"), Some(40));
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert!(is_subsequence(&chars("gchr"), &chars("google chrome")));
        assert!(!is_subsequence(&chars("zx"), &chars("google chrome")));
        assert_eq!(score("x", "Google Chrome"), None);
        // Sin tildes en ningún lado.
        assert_eq!(score("musica", "Música"), Some(100));
    }

    #[test]
    fn las_acciones_no_tapan_a_las_apps() {
        let app = Item {
            id: "app:chrome".into(),
            title: "Google Chrome".into(),
            subtitle: "Aplicación".into(),
            target: Target::Path("chrome.lnk".into()),
            is_app: true,
            exe: None,
        };
        let lock = actions_list()
            .into_iter()
            .find(|item| item.id.as_ref() == "action:sys-lock")
            .unwrap();
        assert!(item_score("chr", &app) > item_score("chr", &lock));
        // Por subtítulo, solo si lo contiene: «sesion» encuentra Bloquear.
        assert_eq!(item_score("sesion", &lock), Some(30));
    }

    #[test]
    fn sin_texto_parte_por_las_caras() {
        let sections = emoji::sections("", &[]);
        assert_eq!(emoji::all()[sections[0].items[0]].ch.as_ref(), "😀");
    }

    #[test]
    fn emoji_en_espanol() {
        let hits = emoji::search(emoji::all(), "corazon rojo", 160);
        assert!(!hits.is_empty());
        assert!(hits.iter().take(5).any(|&i| emoji::all()[i].ch.as_ref() == "❤️"));
        assert!(emoji::search(emoji::all(), "zzzz qqq", 160).is_empty());
    }

    #[test]
    fn calculadora_de_atic() {
        let result = crate::calc::evaluate_with("2+2*3", None, crate::calc::Locale::Es);
        assert_eq!(result.map(|r| r.value), Some("8".into()));
        assert!(crate::calc::evaluate_with("chrome", None, crate::calc::Locale::Es).is_none());
    }
}

// --- En la pill ----------------------------------------------------------------

impl crate::Pill {
    /// El atajo abre el lanzador en el notch; con él abierto, lo cierra.
    pub(crate) fn toggle_launcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.capture.is_some() || self.board.is_some() || self.color.is_some() {
            return;
        }
        if self.panel_open && self.notch_tool == crate::NotchTool::Apps {
            self.close_panel(true, cx);
            return;
        }
        self.open_notch(crate::NotchTool::Apps, window, cx);
    }

    pub(crate) fn launcher_event(
        &mut self,
        event: &LauncherEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = match event {
            LauncherEvent::Close => return self.close_panel(true, cx),
            LauncherEvent::Paste(text) => return self.paste_text(text.clone(), cx),
            LauncherEvent::Run(target) => target.clone(),
        };
        println!("lanzador → {target:?}");
        match target {
            Target::Copy(value) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(value));
                self.close_panel(true, cx);
            }
            // La app nueva se lleva el foco: no se devuelve a la anterior.
            Target::Path(_) | Target::Aumid(_) => {
                self.close_panel(false, cx);
                cx.background_spawn(async move {
                    if let Err(error) = launch(&target) {
                        eprintln!("lanzador: {error}");
                    }
                })
                .detach();
            }
            Target::Action(action) => {
                self.close_panel(false, cx);
                match action {
                    Action::Capture => self.start_capture(false, window, cx),
                    Action::Board => self.start_board(window, cx),
                    Action::Color => self.start_color(window, cx),
                    Action::Clipboard => self.open_notch(crate::NotchTool::Clipboard, window, cx),
                    Action::Textos => self.open_notch(crate::NotchTool::Textos, window, cx),
                    Action::Flip => self.start_flip(window, cx),
                    system => {
                        if let Some(target) = self.paste_target.take() {
                            crate::paste::force_foreground(target);
                        }
                        if let Err(error) = system_action(system) {
                            eprintln!("lanzador: {error}");
                        }
                    }
                }
            }
        }
        cx.notify();
    }
}

