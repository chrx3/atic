//! Launcher tipo Spotlight: programas del menú Inicio + acciones de Atic.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

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
/// Acciones builtin indexadas en inglés.
static INDEX_EN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LauncherKind {
    App,
    Action,
}

#[derive(Debug, Clone)]
enum EntryTarget {
    Path(PathBuf),
    /// App del AppsFolder (UWP/Store): se identifica y lanza por AppUserModelID.
    Aumid(String),
    Action(&'static str),
}

#[derive(Debug, Clone)]
pub(crate) struct LauncherEntry {
    pub(crate) id: String,
    pub(crate) kind: LauncherKind,
    pub(crate) title: String,
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
    /// Hay una ventana visible de esta app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    /// Es la ventana del frente.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<bool>,
    /// Epoch ms del arranque del proceso (GetProcessTimes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_at: Option<u64>,
    /// Epoch ms de la última vez que Atic la lanzó o estuvo al frente.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

static INDEX: OnceLock<Mutex<Vec<LauncherEntry>>> = OnceLock::new();

pub(crate) fn index() -> &'static Mutex<Vec<LauncherEntry>> {
    INDEX.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn normalize(s: &str) -> String {
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

/// Puntaje de una entrada contra la query. Las apps puntúan solo por título:
/// su subtitle genérico ("Aplicación") haría que cualquier query subsecuencia
/// de esa palabra matcheara todas las apps a la vez.
fn entry_score(query: &str, entry: &LauncherEntry) -> Option<u32> {
    match entry.kind {
        LauncherKind::App => score_match(query, &entry.title),
        LauncherKind::Action => best_score(query, &[&entry.title, &entry.subtitle])
            .map(|score| score.saturating_add(15)),
    }
}

fn pick<'a>(en: bool, es: &'a str, english: &'a str) -> &'a str {
    if en {
        english
    } else {
        es
    }
}

fn builtin_actions(en: bool) -> Vec<LauncherEntry> {
    [
        (
            "action:dictation",
            pick(en, "Dictar", "Dictate"),
            pick(en, "Iniciar o detener dictado", "Start or stop dictation"),
            "dictation",
        ),
        (
            "action:capture",
            pick(en, "Capturar pantalla", "Capture screen"),
            pick(
                en,
                "Seleccionar ventana, región o monitor",
                "Pick a window, region, or monitor",
            ),
            "capture",
        ),
        (
            "action:board",
            pick(en, "Dibujar en pantalla", "Draw on screen"),
            pick(
                en,
                "Congelar la pantalla y marcarla",
                "Freeze the screen and mark it",
            ),
            "board",
        ),
        (
            "action:clipboard",
            pick(en, "Historial de clipboard", "Clipboard history"),
            pick(
                en,
                "Abrir el historial junto a la pill",
                "Open history next to the pill",
            ),
            "clipboard",
        ),
        (
            "action:snippets",
            pick(en, "Textos guardados", "Saved texts"),
            pick(en, "Abrir fragmentos y bloc", "Open snippets and the pad"),
            "snippets",
        ),
        (
            "action:agents",
            pick(en, "Agentes", "Agents"),
            pick(en, "Abrir la consola de agentes", "Open the agents console"),
            "agents",
        ),
        (
            "action:settings",
            pick(en, "Ajustes", "Settings"),
            pick(
                en,
                "Abrir la ventana principal de Atic",
                "Open the main Atic window",
            ),
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

/// Apps de `shell:AppsFolder` (UWP/Store): las que no dejan .lnk en el menú
/// Inicio —Outlook nuevo, Calculadora, Terminal…— y solo existen por AUMID.
#[cfg(windows)]
fn collect_apps_folder() -> Vec<LauncherEntry> {
    crate::launcher_icons::with_com(collect_apps_folder_inner).unwrap_or_default()
}

#[cfg(windows)]
fn collect_apps_folder_inner() -> Option<Vec<LauncherEntry>> {
    use windows::core::Interface;
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::IBindCtx;
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2,
        SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
    };

    let mut out = Vec::new();
    unsafe {
        let folder: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).ok()?;
        let items: IEnumShellItems = folder
            .BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)
            .ok()?;
        loop {
            let mut slot = [None::<IShellItem>];
            let mut fetched = 0u32;
            if items.Next(&mut slot, Some(&raw mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else {
                break;
            };
            let Some(title) = pwstr_to_string(item.GetDisplayName(SIGDN_NORMALDISPLAY)) else {
                continue;
            };
            let Ok(item2) = item.cast::<IShellItem2>() else {
                continue;
            };
            let Some(aumid) = pwstr_to_string(item2.GetString(&PKEY_AppUserModel_ID)) else {
                continue;
            };
            let title = title.trim().to_string();
            if title.is_empty() || !aumid_valido(aumid.trim()) || should_skip_app_name(&title) {
                continue;
            }
            out.push(LauncherEntry {
                id: format!("uwp:{aumid}"),
                kind: LauncherKind::App,
                title,
                subtitle: "Aplicación".into(),
                target: EntryTarget::Aumid(aumid),
            });
        }
    }
    Some(out)
}

#[cfg(not(windows))]
fn collect_apps_folder() -> Vec<LauncherEntry> {
    Vec::new()
}

/// Copia el PWSTR del shell y libera la memoria COM que lo respalda.
#[cfg(windows)]
fn pwstr_to_string(res: windows::core::Result<windows::core::PWSTR>) -> Option<String> {
    let p = res.ok()?;
    if p.is_null() {
        return None;
    }
    unsafe {
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.as_ptr() as _));
        s
    }
}

/// Suma solo las apps con título nuevo (normalizado): las del menú Inicio van
/// primero —su .lnk ya tiene icono y lanzado probados— y el AppsFolder aporta
/// las que no tienen acceso directo.
fn merge_unique_apps(base: &mut Vec<LauncherEntry>, extra: Vec<LauncherEntry>) {
    let mut seen: std::collections::HashSet<String> =
        base.iter().map(|e| normalize(&e.title)).collect();
    for entry in extra {
        if seen.insert(normalize(&entry.title)) {
            base.push(entry);
        }
    }
}

/// Época (segundos) del último kick de rebuild; 0 = nunca.
static LAST_BUILD: AtomicU64 = AtomicU64::new(0);
/// Rebuild en vuelo. Quien gana el `compare_exchange` spawnea el worker.
static REBUILDING: AtomicBool = AtomicBool::new(false);
/// Pedido de otro rebuild mientras uno corre (idioma, índice viejo).
static REBUILD_AGAIN: AtomicBool = AtomicBool::new(false);
/// Despierta a los `launcher_*` que esperan el primer índice.
static INDEX_CV: Condvar = Condvar::new();
/// Al abrir el launcher, un índice más viejo que esto se rehace en background.
const REBUILD_AFTER_SECS: u64 = 60;
/// Tope para el wait en el pool bloqueante. El hilo UI no espera nunca.
const POPULATE_WAIT: Duration = Duration::from_secs(20);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rebuild_index() -> Vec<LauncherEntry> {
    let en = INDEX_EN.load(Ordering::Relaxed);
    let mut entries = builtin_actions(en);
    let mut apps = collect_start_menu_apps();
    merge_unique_apps(&mut apps, collect_apps_folder());
    apps.sort_by_key(|entry| entry.title.to_lowercase());
    entries.extend(apps);
    LAST_BUILD.store(now_secs(), Ordering::Relaxed);
    entries
}

/// Publica el índice y despierta a quien espera el primer swap.
fn publish_index(entries: Vec<LauncherEntry>) {
    let count = entries.len();
    let mut guard = index().lock_or_recover();
    *guard = entries;
    INDEX_CV.notify_all();
    drop(guard);
    tracing::info!(count, "índice del launcher listo");
}

/// Un solo worker a la vez: dos kicks seguidos se coalescen en un segundo
/// pase (para no caminar COM dos veces al abrir durante el indexado inicial).
fn rebuild_worker() {
    loop {
        let entries = rebuild_index();
        publish_index(entries);
        if REBUILD_AGAIN.swap(false, Ordering::SeqCst) {
            continue;
        }
        REBUILDING.store(false, Ordering::SeqCst);
        // Pedido que llegó entre el swap de AGAIN y el store de REBUILDING.
        if REBUILD_AGAIN.swap(false, Ordering::SeqCst)
            && REBUILDING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            continue;
        }
        break;
    }
}

/// Reconstruye el índice en un hilo aparte y lo cambia al final. Para los
/// caminos del hilo principal, que no pueden pagar el rebuild inline (ahora
/// con COM y AppsFolder es aún más caro).
fn rebuild_in_background() {
    // Se marca ya: `maybe_refresh_index` no lanza otro kick a los 60 s
    // mientras el primero todavía camina COM.
    LAST_BUILD.store(now_secs(), Ordering::Relaxed);
    if REBUILDING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        REBUILD_AGAIN.store(true, Ordering::SeqCst);
        return;
    }
    thread::spawn(rebuild_worker);
}

/// Apps recién instaladas sin reiniciar: si el índice está viejo se rehace en
/// background y se cambia al final; la búsqueda sigue sirviendo el actual.
fn maybe_refresh_index() {
    let last = LAST_BUILD.load(Ordering::Relaxed);
    if now_secs().saturating_sub(last) < REBUILD_AFTER_SECS {
        return;
    }
    rebuild_in_background();
}

/// Indexa en background al arrancar la app.
pub fn start_indexing(en: bool) {
    INDEX_EN.store(en, Ordering::Relaxed);
    rebuild_in_background();
}

pub fn refresh_language(en: bool) {
    INDEX_EN.store(en, Ordering::Relaxed);
    // Llega desde set_config, en el hilo principal: el rebuild va aparte.
    rebuild_in_background();
}

/// Si el índice está vacío, dispara el rebuild (o se suma al que ya corre) y
/// espera el swap. Solo para el pool bloqueante de `launcher_*`: el hilo UI
/// abre el float igual y no paga COM.
pub(crate) fn ensure_index_populated() {
    if !index().lock_or_recover().is_empty() {
        maybe_refresh_index();
        return;
    }
    rebuild_in_background();
    let deadline = Instant::now() + POPULATE_WAIT;
    let mut guard = index().lock_or_recover();
    while guard.is_empty() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            tracing::warn!("el índice del launcher sigue vacío tras esperar el rebuild");
            return;
        }
        guard = match INDEX_CV.wait_timeout(guard, remaining) {
            Ok((next, timed)) => {
                if timed.timed_out() {
                    if next.is_empty() {
                        tracing::warn!(
                            "el índice del launcher sigue vacío tras esperar el rebuild"
                        );
                    }
                    return;
                }
                next
            }
            Err(poisoned) => {
                tracing::warn!(
                    "mutex envenenado por un pánico anterior; se continúa con el estado que haya"
                );
                let (next, timed) = poisoned.into_inner();
                if timed.timed_out() {
                    return;
                }
                next
            }
        };
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
    // Nada de reconstruir ni esperar aquí: esto corre en el hilo principal.
    // Los `launcher_*` (pool bloqueante) esperan el primer swap si hace falta.
    maybe_refresh_index();
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

/// Async para salir del hilo principal: reconstruir el índice recorre disco.
#[tauri::command]
pub async fn launcher_reindex() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let entries = rebuild_index();
        let count = entries.len();
        publish_index(entries);
        count
    })
    .await
    .map_err(|e| e.to_string())
}

/// Async para salir del hilo principal. Si el índice aún está vacío espera
/// el rebuild de background (no sirve `[]` en el primer keystroke).
#[tauri::command]
pub async fn launcher_search(query: String) -> Result<Vec<LauncherHit>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_index_populated();
        let guard = index().lock_or_recover();
        let q = query.trim();

        // Query vacía: el UI muestra solo la barra + favoritos; sin lista.
        if q.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(u32, &LauncherEntry)> = Vec::new();
        for entry in guard.iter() {
            let Some(score) = entry_score(q, entry) else {
                continue;
            };
            scored.push((score, entry));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        scored
            .into_iter()
            .take(SEARCH_LIMIT)
            .map(|(score, entry)| LauncherHit {
                id: entry.id.clone(),
                kind: entry.kind,
                title: entry.title.clone(),
                subtitle: if entry.kind == LauncherKind::App {
                    pick(
                        INDEX_EN.load(Ordering::Relaxed),
                        "Aplicación",
                        "Application",
                    )
                    .to_string()
                } else {
                    entry.subtitle.clone()
                },
                score: Some(score),
                running: None,
                foreground: None,
                opened_at: None,
                last_used_at: None,
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

pub(crate) fn hit_from_entry(entry: &LauncherEntry) -> LauncherHit {
    let en = INDEX_EN.load(Ordering::Relaxed);
    LauncherHit {
        id: entry.id.clone(),
        kind: entry.kind,
        title: entry.title.clone(),
        subtitle: if entry.kind == LauncherKind::App {
            pick(en, "Aplicación", "Application").to_string()
        } else {
            entry.subtitle.clone()
        },
        score: None,
        running: None,
        foreground: None,
        opened_at: None,
        last_used_at: None,
    }
}

/// Resuelve los favoritos persistidos contra el índice (omite ids huérfanos).
/// Async: puede reconstruir el índice y eso no va en el hilo principal.
#[tauri::command]
pub async fn launcher_list_favorites(
    state: State<'_, AppState>,
) -> Result<Vec<LauncherHit>, String> {
    let ids = state.config.lock_or_recover().launcher_favorites.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ensure_index_populated();
        let guard = index().lock_or_recover();
        let mut out = Vec::new();
        for id in ids.into_iter().take(FAVORITES_LIMIT) {
            if let Some(entry) = guard.iter().find(|e| e.id == id) {
                out.push(hit_from_entry(entry));
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

/// Alterna un id en `launcher_favorites` y persiste la config.
/// Async por lo mismo que los favoritos: el índice puede reconstruirse.
#[tauri::command]
pub async fn launcher_toggle_favorite(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<String>, String> {
    if id.trim().is_empty() {
        return Err("id vacío".into());
    }
    let exists = {
        let id = id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            ensure_index_populated();
            index().lock_or_recover().iter().any(|e| e.id == id)
        })
        .await
        .map_err(|e| e.to_string())?
    };
    if !exists {
        return Err("resultado no encontrado".into());
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
///
/// Async + hilo bloqueante: resolver el icono de un .lnk puede colgarse
/// segundos (destino en red desconectada, placeholder de OneDrive) y antes
/// congelaba el hilo principal.
#[tauri::command]
pub async fn launcher_icon(id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_index_populated();
        let target = {
            let guard = index().lock_or_recover();
            guard.iter().find(|e| e.id == id).map(|e| e.target.clone())
        };
        match target {
            Some(EntryTarget::Path(p)) => crate::launcher_icons::icon_data_url(&p),
            Some(EntryTarget::Aumid(a)) => crate::launcher_icons::uwp_icon_data_url(&a),
            _ => None,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Un AUMID se interpola en `shell:AppsFolder\…`: nada con controles, NUL o
/// separador inicial debe llegar a ShellExecute (ni entrar al índice).
pub(crate) fn aumid_valido(aumid: &str) -> bool {
    !aumid.is_empty() && !aumid.starts_with('\\') && !aumid.chars().any(char::is_control)
}

/// Lanza una app del AppsFolder por AppUserModelID: no hay .exe que abrir,
/// el shell activa el paquete.
#[cfg(windows)]
fn launch_apps_folder(aumid: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;

    if !aumid_valido(aumid) {
        return Err("identificador de app inválido".into());
    }
    let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let file: Vec<u16> = format!("shell:AppsFolder\\{aumid}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    // COM: esto ya no corre en el hilo principal. SW_SHOWNORMAL = 1;
    // ShellExecuteW devuelve > 32 si pudo lanzar.
    let h = crate::launcher_icons::with_com(|| unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    });
    if h as isize > 32 {
        Ok(())
    } else {
        Err(format!("no se pudo abrir la app ({})", h as isize))
    }
}

#[cfg(not(windows))]
fn launch_apps_folder(_aumid: &str) -> Result<(), String> {
    Err("apps de Store solo existen en Windows".into())
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

/// Async: el lookup puede reconstruir el índice; eso va al pool bloqueante.
/// Las acciones vuelven al hilo principal, que es donde siempre corrieron.
#[tauri::command]
pub async fn launcher_run(app: AppHandle, id: String) -> Result<(), String> {
    let lookup = id.clone();
    let entry = tauri::async_runtime::spawn_blocking(move || {
        ensure_index_populated();
        index()
            .lock_or_recover()
            .iter()
            .find(|e| e.id == lookup)
            .cloned()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(entry) = entry else {
        return Err("resultado no encontrado".into());
    };

    match entry.target {
        EntryTarget::Path(path) => {
            app.opener()
                .open_path(path.to_string_lossy().into_owned(), None::<&str>)
                .map_err(|e| format!("no se pudo abrir: {e}"))?;
        }
        EntryTarget::Aumid(aumid) => {
            launch_apps_folder(&aumid)?;
        }
        EntryTarget::Action(action) => {
            let app2 = app.clone();
            app.run_on_main_thread(move || {
                if let Err(err) = run_action(&app2, action) {
                    tracing::warn!(%err, "acción del launcher falló");
                }
            })
            .map_err(|e| e.to_string())?;
        }
    }
    remember_launch(&app, &id);
    hide(&app);
    Ok(())
}

fn remember_launch(app: &AppHandle, id: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let path = crate::launcher_recents::store_path(&state.dirs.data_dir());
    crate::launcher_recents::touch(&path, id);
}

/// Apps abiertas ahora + las que se lanzaron desde Atic.
#[tauri::command]
pub async fn launcher_list_recents(
    state: State<'_, AppState>,
) -> Result<Vec<LauncherHit>, String> {
    let path = crate::launcher_recents::store_path(&state.dirs.data_dir());
    tauri::async_runtime::spawn_blocking(move || crate::launcher_recents::list(&path))
        .await
        .map_err(|e| e.to_string())
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

    #[test]
    fn un_aumid_raro_no_llega_al_shell() {
        assert!(aumid_valido("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"));
        assert!(!aumid_valido(""));
        assert!(!aumid_valido("\\raro"));
        assert!(!aumid_valido("con\ncontrol"));
        assert!(!aumid_valido("nul\0"));
    }

    #[test]
    fn el_apps_folder_no_duplica_lo_que_ya_trae_el_menu_inicio() {
        let lnk = |title: &str| LauncherEntry {
            id: format!("app:{title}.lnk"),
            kind: LauncherKind::App,
            title: title.into(),
            subtitle: "Aplicación".into(),
            target: EntryTarget::Path(PathBuf::from(format!("{title}.lnk"))),
        };
        let uwp = |title: &str| LauncherEntry {
            id: format!("uwp:{title}!App"),
            kind: LauncherKind::App,
            title: title.into(),
            subtitle: "Aplicación".into(),
            target: EntryTarget::Aumid(format!("{title}!App")),
        };
        let mut apps = vec![lnk("Terminal"), lnk("Outlook (classic)")];
        // "Términal" normaliza igual que "Terminal": mismo título, se descarta.
        merge_unique_apps(
            &mut apps,
            vec![uwp("Términal"), uwp("Outlook"), uwp("WhatsApp")],
        );
        let ids: Vec<&str> = apps.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "app:Terminal.lnk",
                "app:Outlook (classic).lnk",
                "uwp:Outlook!App",
                "uwp:WhatsApp!App",
            ]
        );
    }

    #[test]
    fn apps_score_title_only() {
        let app = LauncherEntry {
            id: "app:chrome".into(),
            kind: LauncherKind::App,
            title: "Chrome".into(),
            subtitle: "Aplicación".into(),
            target: EntryTarget::Path(PathBuf::from("chrome.lnk")),
        };
        // "pli" es subsecuencia de "aplicacion"; no debe matchear vía subtitle.
        assert_eq!(entry_score("pli", &app), None);
        assert_eq!(entry_score("chro", &app), Some(100));

        let action = LauncherEntry {
            id: "action:capture".into(),
            kind: LauncherKind::Action,
            title: "Capturar pantalla".into(),
            subtitle: "Seleccionar ventana, región o monitor".into(),
            target: EntryTarget::Action("capture"),
        };
        // Las acciones siguen matcheando por subtítulo, con su boost de +15.
        assert_eq!(entry_score("ventana", &action), Some(65));
    }
}
