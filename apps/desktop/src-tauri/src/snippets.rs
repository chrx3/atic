//! Fragmentos de texto reutilizables y bloc de notas local.

use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::clipboard_history;
use crate::state::AppState;
use atic_core::MutexExt;

const SNIPPETS_FILE: &str = "snippets.json";
const SCRATCHPAD_FILE: &str = "scratchpad.json";
const NOTES_FILE: &str = "notes.json";

/// Largo máximo del título derivado de la primera línea.
const TITLE_MAX: usize = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Scratchpad {
    pub body: String,
    #[serde(default)]
    pub updated_at_ms: u64,
}

/// Una nota guardada.
///
/// El título no se pide: sale de la primera línea del cuerpo. Obligar a nombrar
/// algo antes de escribirlo es fricción justo en el momento en que querés
/// anotar rápido, que es para lo que existe el bloc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub updated_at_ms: u64,
}

static SNIPPETS: Mutex<Option<Vec<Snippet>>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn snippets_path(dir: &Path) -> std::path::PathBuf {
    dir.join(SNIPPETS_FILE)
}

fn scratchpad_path(dir: &Path) -> std::path::PathBuf {
    dir.join(SCRATCHPAD_FILE)
}

fn load_snippets_from_disk(dir: &Path) -> Vec<Snippet> {
    let path = snippets_path(dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_snippets_to_disk(dir: &Path, items: &[Snippet]) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(raw) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(snippets_path(dir), raw);
    }
}

fn ensure_loaded(state: &AppState) -> Vec<Snippet> {
    let mut guard = SNIPPETS.lock_or_recover();
    if guard.is_none() {
        let items = load_snippets_from_disk(&state.dirs.snippets_dir());
        *guard = Some(items);
    }
    guard.as_ref().unwrap().clone()
}

fn with_snippets_mut<R>(state: &AppState, f: impl FnOnce(&mut Vec<Snippet>) -> R) -> R {
    let dir = state.dirs.snippets_dir();
    let mut guard = SNIPPETS.lock_or_recover();
    if guard.is_none() {
        *guard = Some(load_snippets_from_disk(&dir));
    }
    let items = guard.as_mut().unwrap();
    let result = f(items);
    save_snippets_to_disk(&dir, items);
    result
}

fn find_snippet(state: &AppState, id: &str) -> Option<Snippet> {
    ensure_loaded(state).into_iter().find(|s| s.id == id)
}

/// Todos los fragmentos (para búsqueda).
pub(crate) fn all_snippets(state: &AppState) -> Vec<Snippet> {
    ensure_loaded(state)
}

/// Cuerpo del bloc de notas (para búsqueda).
pub(crate) fn scratchpad_body(state: &AppState) -> Result<String, String> {
    let path = scratchpad_path(&state.dirs.snippets_dir());
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(String::new());
    };
    let pad: Scratchpad = serde_json::from_str(&raw).unwrap_or_default();
    Ok(pad.body)
}

/// Lista los fragmentos guardados (más recientes primero).
#[tauri::command]
pub fn list_snippets(state: State<AppState>) -> Result<Vec<Snippet>, String> {
    let mut items = ensure_loaded(&state);
    // Más reciente primero.
    items.sort_by_key(|s| std::cmp::Reverse(s.updated_at_ms));
    Ok(items)
}

/// Crea o actualiza un fragmento. Si `id` está vacío, se genera uno nuevo.
#[tauri::command]
pub fn upsert_snippet(
    app: AppHandle,
    state: State<AppState>,
    mut snippet: Snippet,
) -> Result<Snippet, String> {
    let name = snippet.name.trim();
    if name.is_empty() {
        return Err("El nombre no puede estar vacío".into());
    }
    snippet.name = name.to_string();
    snippet.body = snippet.body.trim_end().to_string();
    snippet.aliases = snippet
        .aliases
        .into_iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    snippet.updated_at_ms = now_ms();

    if snippet.id.trim().is_empty() {
        snippet.id = uuid::Uuid::new_v4().to_string();
        with_snippets_mut(&state, |items| items.push(snippet.clone()));
    } else {
        with_snippets_mut(&state, |items| {
            if let Some(existing) = items.iter_mut().find(|s| s.id == snippet.id) {
                existing.name = snippet.name.clone();
                existing.body = snippet.body.clone();
                existing.aliases = snippet.aliases.clone();
                existing.updated_at_ms = snippet.updated_at_ms;
            } else {
                items.push(snippet.clone());
            }
        });
    }

    let _ = app.emit("snippets-changed", ());
    Ok(snippet)
}

#[tauri::command]
pub fn delete_snippet(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let removed = with_snippets_mut(&state, |items| {
        let idx = items.iter().position(|s| s.id == id)?;
        Some(items.remove(idx))
    });
    if removed.is_none() {
        return Err("Fragmento no encontrado".into());
    }
    let _ = app.emit("snippets-changed", ());
    Ok(())
}

/// Pega el cuerpo del fragmento en la app que tenía el foco.
#[tauri::command]
pub fn paste_snippet(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let snippet = find_snippet(&state, &id).ok_or_else(|| "Fragmento no encontrado".to_string())?;
    if snippet.body.is_empty() {
        return Err("El fragmento está vacío".into());
    }

    clipboard_history::focus_paste_target();
    thread::sleep(Duration::from_millis(220));

    crate::paste_queue::paste_text_or_enqueue(&app, &snippet.body)?;

    let _ = app.emit("pill-snippets-close", ());
    Ok(())
}

#[tauri::command]
pub fn get_scratchpad(state: State<AppState>) -> Result<Scratchpad, String> {
    let path = scratchpad_path(&state.dirs.snippets_dir());
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(Scratchpad::default());
    };
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn notes_path(dir: &Path) -> std::path::PathBuf {
    dir.join(NOTES_FILE)
}

/// Título a partir de la primera línea con contenido.
fn derive_title(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty());
    match line {
        None => "Nota sin título".to_string(),
        Some(l) if l.chars().count() <= TITLE_MAX => l.to_string(),
        Some(l) => {
            let cut: String = l.chars().take(TITLE_MAX - 1).collect();
            format!("{cut}…")
        }
    }
}

fn load_notes(dir: &Path) -> Vec<Note> {
    let Ok(raw) = std::fs::read_to_string(notes_path(dir)) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_notes(dir: &Path, notes: &[Note]) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir);
    let raw = serde_json::to_string_pretty(notes).map_err(|e| e.to_string())?;
    std::fs::write(notes_path(dir), raw).map_err(|e| e.to_string())
}

/// Notas guardadas, de la más reciente a la más vieja.
#[tauri::command]
pub fn list_notes(state: State<AppState>) -> Result<Vec<Note>, String> {
    let mut notes = load_notes(&state.dirs.snippets_dir());
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated_at_ms));
    Ok(notes)
}

/// Crea o actualiza una nota. Sin `id`, crea una nueva y devuelve la creada.
///
/// El cuerpo vacío no se guarda: sería una nota fantasma en la lista, y el
/// llamador natural (guardar al cambiar de nota) pasa por acá siempre, tenga o
/// no algo escrito.
#[tauri::command]
pub fn save_note(
    state: State<AppState>,
    id: Option<String>,
    body: String,
) -> Result<Option<Note>, String> {
    if body.trim().is_empty() {
        return Ok(None);
    }
    let dir = state.dirs.snippets_dir();
    let mut notes = load_notes(&dir);
    let title = derive_title(&body);
    let now = now_ms();

    let note = match id.and_then(|id| notes.iter().position(|n| n.id == id).zip(Some(id))) {
        Some((idx, id)) => {
            let note = Note {
                id,
                title,
                body,
                updated_at_ms: now,
            };
            notes[idx] = note.clone();
            note
        }
        None => {
            let note = Note {
                id: format!("note-{now}"),
                title,
                body,
                updated_at_ms: now,
            };
            notes.push(note.clone());
            note
        }
    };

    save_notes(&dir, &notes)?;
    Ok(Some(note))
}

#[tauri::command]
pub fn delete_note(state: State<AppState>, id: String) -> Result<(), String> {
    let dir = state.dirs.snippets_dir();
    let mut notes = load_notes(&dir);
    notes.retain(|n| n.id != id);
    save_notes(&dir, &notes)
}

#[tauri::command]
pub fn set_scratchpad(state: State<AppState>, body: String) -> Result<Scratchpad, String> {
    let dir = state.dirs.snippets_dir();
    let _ = std::fs::create_dir_all(&dir);
    let pad = Scratchpad {
        body,
        updated_at_ms: now_ms(),
    };
    let raw = serde_json::to_string_pretty(&pad).map_err(|e| e.to_string())?;
    std::fs::write(scratchpad_path(&dir), raw).map_err(|e| e.to_string())?;
    Ok(pad)
}

/// Atajo de fragmentos: el frontend hace toggle (cerrar si ya está abierto).
pub fn summon_snippets_panel(app: &AppHandle) {
    clipboard_history::remember_paste_target();
    let _ = app.emit("pill-snippets-toggle", ());
}

/// Compacta la pill y la anima hasta el cursor antes de expandir fragmentos.
#[tauri::command]
pub fn prepare_snippets_pill(app: AppHandle, fly: bool) -> Result<u64, String> {
    clipboard_history::prepare_clipboard_pill(app, fly)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulo_usa_la_primera_linea_con_contenido() {
        assert_eq!(
            derive_title("  \n\n  Comprar pan\nmás cosas"),
            "Comprar pan"
        );
    }

    #[test]
    fn titulo_de_cuerpo_vacio_no_queda_en_blanco() {
        // Una nota sin título en la lista sería una fila sin nada que leer.
        assert_eq!(derive_title("   \n \n"), "Nota sin título");
    }

    #[test]
    fn titulo_largo_se_corta_con_puntos_suspensivos() {
        let largo = "a".repeat(TITLE_MAX + 20);
        let titulo = derive_title(&largo);
        assert_eq!(titulo.chars().count(), TITLE_MAX);
        assert!(titulo.ends_with('…'));
    }

    #[test]
    fn titulo_corta_por_caracteres_no_por_bytes() {
        // Con acentos, cortar por bytes parte un carácter y entra en pánico.
        let largo = "á".repeat(TITLE_MAX + 5);
        let titulo = derive_title(&largo);
        assert_eq!(titulo.chars().count(), TITLE_MAX);
    }
}
