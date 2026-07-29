//! Launcher tipo Spotlight: programas del menú Inicio + acciones de Atic.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;

use atic_core::MutexExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

const WINDOW_LABEL: &str = "launcher";
const EMPTY_RESULT_LIMIT: usize = 12;
const SEARCH_LIMIT: usize = 24;

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

/// Centra el launcher en el monitor donde está el cursor (pantalla activa).
///
/// Antes usaba `current_monitor()` de la ventana, que era la última donde se
/// abrió —en multi-monitor quedaba en la pantalla equivocada.
fn position_on_active_monitor(window: &WebviewWindow) {
    let win_size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(560, 420));
    let w = win_size.width as i32;
    let h = win_size.height as i32;

    if let Some((x, y)) = active_monitor_origin(window, w, h) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

#[cfg(windows)]
fn active_monitor_origin(window: &WebviewWindow, w: i32, h: i32) -> Option<(i32, i32)> {
    let _ = window;
    let monitors = atic_capture::monitors::enumerate();
    if monitors.is_empty() {
        return None;
    }
    let cursor = crate::floating::cursor_position();
    let work = cursor
        .and_then(|(cx, cy)| {
            monitors
                .iter()
                .find(|m| m.bounds.contains(cx, cy))
                .map(|m| m.work_area)
        })
        .or_else(|| monitors.iter().find(|m| m.is_primary).map(|m| m.work_area))
        .or_else(|| monitors.first().map(|m| m.work_area))?;

    let x = work.x + (work.width as i32 - w) / 2;
    // Un poco por encima del centro vertical, estilo Spotlight.
    let y = work.y + (work.height as i32 - h) / 3;
    Some(crate::floating::clamp(x, y, w, h))
}

#[cfg(not(windows))]
fn active_monitor_origin(window: &WebviewWindow, w: i32, h: i32) -> Option<(i32, i32)> {
    let monitor = window.current_monitor().ok().flatten()?;
    let size = monitor.size();
    let pos = monitor.position();
    let x = pos.x + (size.width as i32 - w) / 2;
    let y = pos.y + (size.height as i32 - h) / 3;
    Some((x, y))
}

pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        tracing::warn!("ventana launcher inexistente");
        return;
    };
    ensure_index_populated();
    let _ = window.set_size(tauri::LogicalSize::new(560.0, 420.0));
    // Posicionar ANTES de show: si no, parpadea un frame en el monitor viejo.
    position_on_active_monitor(&window);
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("launcher-opened", ());
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.hide();
    }
}

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show(app);
    }
}

#[tauri::command]
pub fn toggle_launcher(app: AppHandle) {
    toggle(&app);
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

    if q.is_empty() {
        let mut hits: Vec<LauncherHit> = Vec::new();
        for entry in guard.iter().filter(|e| e.kind == LauncherKind::Action) {
            hits.push(LauncherHit {
                id: entry.id.clone(),
                kind: entry.kind,
                title: entry.title.clone(),
                subtitle: entry.subtitle.clone(),
                score: None,
            });
        }
        for entry in guard
            .iter()
            .filter(|e| e.kind == LauncherKind::App)
            .take(EMPTY_RESULT_LIMIT.saturating_sub(hits.len()))
        {
            hits.push(LauncherHit {
                id: entry.id.clone(),
                kind: entry.kind,
                title: entry.title.clone(),
                subtitle: entry.subtitle.clone(),
                score: None,
            });
        }
        return Ok(hits);
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

fn run_action(app: &AppHandle, action: &str) -> Result<(), String> {
    match action {
        "dictation" => {
            crate::dictation::toggle_dictation(app);
            Ok(())
        }
        "capture" => crate::capture_session::trigger(app),
        "clipboard" => {
            crate::clipboard_history::summon_clipboard_panel(app);
            Ok(())
        }
        "snippets" => {
            crate::snippets::summon_snippets_panel(app);
            Ok(())
        }
        "agents" => {
            crate::agents::bridge::show_agents_window(app.clone());
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
