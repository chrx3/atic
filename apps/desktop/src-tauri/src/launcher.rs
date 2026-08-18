//! Launcher tipo Spotlight: programas del menú Inicio + acciones de Atic.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use atic_core::MutexExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::floating::BubbleShape;
use crate::state::AppState;

const SEARCH_LIMIT: usize = 24;
const FAVORITES_LIMIT: usize = 8;

/// Float Spotlight compacto (una línea); el frontend crece al buscar.
const LAUNCHER_SHAPE: BubbleShape = BubbleShape {
    w: 292,
    h: 40,
    gap: 10,
    corner: 18,
};

const LAUNCHER_ANCHOR: &str = "launcher-bubble-anchor";
const LAUNCHER_DISMISS: &str = "launcher-bubble-dismiss";

/// Float del launcher abierto (en el overlay, no ventana aparte).
static LAUNCHER_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LauncherKind {
    App,
    Action,
}

#[derive(Debug, Clone)]
enum EntryTarget {
    Path(PathBuf),
    Action(&'static str),
}

#[derive(Debug, Clone)]
struct LauncherEntry {
    id: String,
    kind: LauncherKind,
    title: String,
    subtitle: String,
    target: EntryTarget,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherHit {
    pub id: String,
    pub kind: LauncherKind,
    pub title: String,
    pub subtitle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
}

static INDEX: OnceLock<Mutex<Vec<LauncherEntry>>> = OnceLock::new();

fn index() -> &'static Mutex<Vec<LauncherEntry>> {
    INDEX.get_or_init(|| Mutex::new(Vec::new()))
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Distancia de edición ≤ 1 (inserción, borrado, sustitución o trasposición).
fn within_one_edit(a: &[char], b: &[char]) -> bool {
    let (na, nb) = (a.len(), b.len());
    match na.abs_diff(nb) {
        0 => {
            let mut mismatches = Vec::new();
            for (i, (ca, cb)) in a.iter().zip(b.iter()).enumerate() {
                if ca != cb {
                    mismatches.push(i);
                    if mismatches.len() > 2 {
                        return false;
                    }
                }
            }
            match mismatches.as_slice() {
                [] | [_] => true,
                // Trasposición de dos letras contiguas: "chorme" ↔ "chrome".
                [i, j] if *j == i + 1 && a[*i] == b[*j] && a[*j] == b[*i] => true,
                _ => false,
            }
        }
        1 => {
            let (longer, shorter) = if na > nb { (a, b) } else { (b, a) };
            let mut i = 0;
            let mut j = 0;
            let mut skipped = false;
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

/// ¿La query aparece como subsecuencia (chars en orden, con huecos)?
fn is_subsequence(query: &[char], haystack: &[char]) -> bool {
    if query.is_empty() {
        return false;
    }
    let mut qi = 0;
    for &ch in haystack {
        if ch == query[qi] {
            qi += 1;
            if qi == query.len() {
                return true;
            }
        }
    }
    false
}

/// Mejor ventana del haystack a distancia ≤ 1 de la query.
fn fuzzy_window_match(query: &[char], haystack: &[char]) -> bool {
    let qn = query.len();
    if qn < 2 {
        // Una sola letra: solo exacto / subsecuencia; el fuzzy inundaría resultados.
        return false;
    }
    let min_w = qn.saturating_sub(1).max(1);
    let max_w = (qn + 1).min(haystack.len());
    if haystack.len() < min_w {
        return false;
    }
    for w in min_w..=max_w {
        for start in 0..=(haystack.len() - w) {
            if within_one_edit(query, &haystack[start..start + w]) {
                return true;
            }
        }
    }
    false
}

fn score_match(query: &str, haystack: &str) -> Option<u32> {
    let q = normalize(query);
    if q.is_empty() {
        return None;
    }
    let h = normalize(haystack);
    let qc: Vec<char> = q.chars().collect();
    let hc: Vec<char> = h.chars().collect();

    if h.starts_with(&q) {
        return Some(100);
    }
    if h.contains(&q) {
        return Some(50);
    }
    // Typo de una letra / trasposición frente a un trozo del título.
    if fuzzy_window_match(&qc, &hc) {
        return Some(40);
    }
    // "chr" → "Chrome": letras en orden, aunque no contiguas.
    if is_subsequence(&qc, &hc) {
        return Some(28);
    }
    None
}

fn best_score(query: &str, parts: &[&str]) -> Option<u32> {
    parts
        .iter()
        .filter_map(|part| score_match(query, part))
        .max()
}

fn builtin_actions() -> Vec<LauncherEntry> {
    [
        (
            "action:dictation",
            "Dictar",
            "Iniciar o detener dictado",
            "dictation",
        ),
        (
            "action:capture",
            "Capturar pantalla",
            "Seleccionar ventana, región o monitor",
            "capture",
        ),
        (
            "action:board",
            "Dibujar en pantalla",
            "Congelar la pantalla y marcarla",
            "board",
        ),
        (
            "action:clipboard",
            "Historial de clipboard",
            "Abrir el historial junto a la pill",
            "clipboard",
        ),
        (
            "action:snippets",
            "Textos guardados",
            "Abrir fragmentos y bloc",
            "snippets",
        ),
        (
            "action:agents",
            "Agentes",
            "Abrir la consola de agentes",
            "agents",
        ),
        (
            "action:settings",
            "Ajustes",
            "Abrir la ventana principal de Atic",
            "settings",
        ),
    ]
    .into_iter()
    .filter(|(_, _, _, action)| crate::agents::UI_ENABLED || *action != "agents")
    .map(|(id, title, subtitle, action)| LauncherEntry {
        id: id.into(),
        kind: LauncherKind::Action,
        title: title.into(),
        subtitle: subtitle.into(),
        target: EntryTarget::Action(action),
    })
    .collect()
}

fn should_skip_app_name(name: &str) -> bool {
    let n = normalize(name);
    n.starts_with("uninstall")
        || n.starts_with("desinstalar")
        || n.contains("uninstall ")
        || n == "desktop.ini"
}

#[cfg(windows)]
fn collect_start_menu_apps() -> Vec<LauncherEntry> {
    let mut roots = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Ok(program_data) = std::env::var("ProgramData") {
        roots.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        walk_lnks(&root, &mut out, &mut seen);
    }
    out.sort_by_key(|entry| entry.title.to_lowercase());
    out
}

#[cfg(windows)]
fn walk_lnks(
    dir: &Path,
    out: &mut Vec<LauncherEntry>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_lnks(&path, out, seen);
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("lnk") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if should_skip_app_name(stem) {
            continue;
        }
        let key = normalize(stem);
        if !seen.insert(key) {
            continue;
        }
        out.push(LauncherEntry {
            id: format!("app:{}", path.to_string_lossy()),
            kind: LauncherKind::App,
            title: stem.to_string(),
            subtitle: "Aplicación".into(),
            target: EntryTarget::Path(path),
        });
    }
}

#[cfg(target_os = "macos")]
fn collect_start_menu_apps() -> Vec<LauncherEntry> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = dirs_home() {
        roots.push(home.join("Applications"));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        walk_apps(&root, &mut out, &mut seen);
    }
    out.sort_by_key(|entry| entry.title.to_lowercase());
    out
}

#[cfg(target_os = "macos")]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn walk_apps(
    dir: &Path,
    out: &mut Vec<LauncherEntry>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".app") {
            continue;
        }
        let title = name.trim_end_matches(".app");
        if should_skip_app_name(title) {
            continue;
        }
        let key = normalize(title);
        if !seen.insert(key) {
            continue;
        }
        out.push(LauncherEntry {
            id: format!("app:{}", path.to_string_lossy()),
            kind: LauncherKind::App,
            title: title.to_string(),
            subtitle: "Aplicación".into(),
            target: EntryTarget::Path(path),
        });
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn collect_start_menu_apps() -> Vec<LauncherEntry> {
    Vec::new()
}

fn rebuild_index() -> Vec<LauncherEntry> {
    let mut entries = builtin_actions();
    entries.extend(collect_start_menu_apps());
    entries
}

/// Indexa en background al arrancar la app.
pub fn start_indexing() {
    thread::spawn(|| {
        let entries = rebuild_index();
        let count = entries.len();
        *index().lock_or_recover() = entries;
        tracing::info!(count, "índice del launcher listo");
    });
}

fn ensure_index_populated() {
    let mut guard = index().lock_or_recover();
    if guard.is_empty() {
        *guard = rebuild_index();
    }
}

/// ¿El float del launcher está abierto?
///
/// Sin usar hoy, igual que sus gemelas en `clipboard_history` y `snippets`.
/// Se conserva por simetría: los tres módulos de float exponen la misma API, y
/// borrar solo esta —la única que el compilador ve muerta, porque su módulo no
/// es `pub`— dejaría el trío desparejo.
#[allow(dead_code)]
pub fn float_open() -> bool {
    LAUNCHER_OPEN.load(Ordering::Relaxed)
}

/// Abre el float del launcher (sale de la pill vía `panel_float`).
///
/// La ventana Tauri `launcher` queda sin uso en el path primario: el UI vive
/// en el overlay con `.float-emerge`, como clipboard/snippets/agentes.
pub fn show(app: &AppHandle) {
    if LAUNCHER_OPEN.load(Ordering::Relaxed) {
        return;
    }
    ensure_index_populated();
    let opened = crate::panel_float::show(app, &LAUNCHER_OPEN, LAUNCHER_SHAPE, LAUNCHER_ANCHOR);
    if opened {
        crate::overlay::set_topmost(app, crate::agents::bridge::overlay_should_be_topmost());
        let _ = app.emit("launcher-opened", ());
    }
}

pub fn hide(app: &AppHandle) {
    if !LAUNCHER_OPEN.load(Ordering::Relaxed) {
        return;
    }
    crate::panel_float::hide(app, &LAUNCHER_OPEN, LAUNCHER_DISMISS);
    crate::overlay::set_topmost(app, crate::agents::bridge::overlay_should_be_topmost());
    let _ = app.emit("launcher-closed", ());
}

pub fn toggle(app: &AppHandle) {
    if LAUNCHER_OPEN.load(Ordering::Relaxed) {
        hide(app);
    } else {
        show(app);
    }
}

/// Atajo Ctrl+Space: al abrir, la pill vuela al centro y recién ahí se muestra.
pub fn toggle_via_slot(app: &AppHandle) {
    if LAUNCHER_OPEN.load(Ordering::Relaxed) {
        hide(app);
        return;
    }
    if let Some(pill) = app.get_webview_window(crate::overlay::LABEL) {
        let visible = app
            .try_state::<crate::state::AppState>()
            .map(|s| s.config.lock_or_recover().show_pill)
            .unwrap_or(true);
        if visible {
            let _ = pill.set_always_on_top(true);
            let _ = pill.show();
        }
    }
    let _ = app.emit("activate-tool-slot", "launcher");
}

#[tauri::command]
pub fn toggle_launcher(app: AppHandle) {
    toggle(&app);
}

#[tauri::command]
pub fn show_launcher(app: AppHandle) {
    show(&app);
}

#[tauri::command]
pub fn hide_launcher(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
pub fn launcher_reindex() -> Result<usize, String> {
    let entries = rebuild_index();
    let count = entries.len();
    *index().lock_or_recover() = entries;
    Ok(count)
}

#[tauri::command]
pub fn launcher_search(query: String) -> Result<Vec<LauncherHit>, String> {
    ensure_index_populated();
    let guard = index().lock_or_recover();
    let q = query.trim();

    // Query vacía: el UI muestra solo la barra + favoritos; sin lista.
    if q.is_empty() {
        return Ok(Vec::new());
    }

    let mut scored: Vec<(u32, &LauncherEntry)> = Vec::new();
    for entry in guard.iter() {
        let Some(mut score) = best_score(q, &[&entry.title, &entry.subtitle]) else {
            continue;
        };
        if entry.kind == LauncherKind::Action {
            score = score.saturating_add(15);
        }
        scored.push((score, entry));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    Ok(scored
        .into_iter()
        .take(SEARCH_LIMIT)
        .map(|(score, entry)| LauncherHit {
            id: entry.id.clone(),
            kind: entry.kind,
            title: entry.title.clone(),
            subtitle: entry.subtitle.clone(),
            score: Some(score),
        })
        .collect())
}

fn hit_from_entry(entry: &LauncherEntry) -> LauncherHit {
    LauncherHit {
        id: entry.id.clone(),
        kind: entry.kind,
        title: entry.title.clone(),
        subtitle: entry.subtitle.clone(),
        score: None,
    }
}

/// Resuelve los favoritos persistidos contra el índice (omite ids huérfanos).
#[tauri::command]
pub fn launcher_list_favorites(state: State<AppState>) -> Result<Vec<LauncherHit>, String> {
    ensure_index_populated();
    let ids = state.config.lock_or_recover().launcher_favorites.clone();
    let guard = index().lock_or_recover();
    let mut out = Vec::new();
    for id in ids.into_iter().take(FAVORITES_LIMIT) {
        if let Some(entry) = guard.iter().find(|e| e.id == id) {
            out.push(hit_from_entry(entry));
        }
    }
    Ok(out)
}

/// Alterna un id en `launcher_favorites` y persiste la config.
#[tauri::command]
pub fn launcher_toggle_favorite(state: State<AppState>, id: String) -> Result<Vec<String>, String> {
    if id.trim().is_empty() {
        return Err("id vacío".into());
    }
    ensure_index_populated();
    {
        let guard = index().lock_or_recover();
        if !guard.iter().any(|e| e.id == id) {
            return Err("resultado no encontrado".into());
        }
    }

    let mut cfg = state.config.lock_or_recover();
    if let Some(pos) = cfg.launcher_favorites.iter().position(|f| f == &id) {
        cfg.launcher_favorites.remove(pos);
    } else {
        cfg.launcher_favorites.retain(|f| f != &id);
        cfg.launcher_favorites.push(id);
        if cfg.launcher_favorites.len() > FAVORITES_LIMIT {
            let drop_n = cfg.launcher_favorites.len() - FAVORITES_LIMIT;
            cfg.launcher_favorites.drain(0..drop_n);
        }
    }
    let next = cfg.launcher_favorites.clone();
    cfg.save(&state.dirs.config_path())
        .map_err(|e| e.to_string())?;
    Ok(next)
}

/// Icono de una entrada `app:…` como data URL PNG (cacheado en RAM).
#[tauri::command]
pub fn launcher_icon(id: String) -> Result<Option<String>, String> {
    ensure_index_populated();
    let path = {
        let guard = index().lock_or_recover();
        guard
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| match &e.target {
                EntryTarget::Path(p) => Some(p.clone()),
                EntryTarget::Action(_) => None,
            })
    };
    Ok(path.and_then(|p| crate::launcher_icons::icon_data_url(&p)))
}

fn run_action(app: &AppHandle, action: &str) -> Result<(), String> {
    match action {
        "dictation" => {
            crate::dictation::toggle_dictation(app);
            Ok(())
        }
        "capture" => crate::capture_session::trigger(app),
        "board" => crate::annotate::toggle_board(app),
        "clipboard" => {
            crate::clipboard_history::remember_paste_target();
            crate::shortcuts::emit_tool_slot(app, "activate-tool-slot", "clipboard");
            Ok(())
        }
        "snippets" => {
            crate::clipboard_history::remember_paste_target();
            crate::shortcuts::emit_tool_slot(app, "activate-tool-slot", "snippets");
            Ok(())
        }
        "agents" => {
            if crate::agents::UI_ENABLED {
                crate::agents::bridge::show_agents_window(app.clone());
            }
            Ok(())
        }
        "settings" => {
            crate::state::show_main(app);
            Ok(())
        }
        other => Err(format!("acción desconocida: {other}")),
    }
}

#[tauri::command]
pub fn launcher_run(app: AppHandle, id: String) -> Result<(), String> {
    ensure_index_populated();
    let entry = {
        let guard = index().lock_or_recover();
        guard.iter().find(|e| e.id == id).cloned()
    };
    let Some(entry) = entry else {
        return Err("resultado no encontrado".into());
    };

    match entry.target {
        EntryTarget::Path(path) => {
            app.opener()
                .open_path(path.to_string_lossy().into_owned(), None::<&str>)
                .map_err(|e| format!("no se pudo abrir: {e}"))?;
        }
        EntryTarget::Action(action) => {
            run_action(&app, action)?;
        }
    }
    hide(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_prefix_outrank_typos() {
        assert_eq!(score_match("chrome", "Google Chrome"), Some(50));
        assert_eq!(score_match("goo", "Google Chrome"), Some(100));
    }

    #[test]
    fn tolerates_one_letter_typo() {
        // Sustitución
        assert_eq!(score_match("chreme", "Chrome"), Some(40));
        // Inserción / borrado
        assert_eq!(score_match("chromee", "Chrome"), Some(40));
        assert_eq!(score_match("chrom", "Chrome"), Some(100)); // prefix
        assert_eq!(score_match("dicatr", "Dictar"), Some(40));
        // Trasposición
        assert_eq!(score_match("chorme", "Chrome"), Some(40));
    }

    #[test]
    fn subsequence_helper_matches_gapped_chars() {
        let q: Vec<char> = "cme".chars().collect();
        let h: Vec<char> = "chrome".chars().collect();
        assert!(is_subsequence(&q, &h));
        assert!(!is_subsequence(&"cmx".chars().collect::<Vec<_>>(), &h));
    }

    #[test]
    fn rejects_unrelated() {
        assert_eq!(score_match("zzzz", "Chrome"), None);
    }
}
