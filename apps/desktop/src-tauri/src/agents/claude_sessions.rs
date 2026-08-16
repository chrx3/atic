//! Sesiones locales de Claude Code: índice + transcript para la UI.
//!
//! Los transcripts viven en `~/.claude/projects/<cwd-encoded>/<session>.jsonl`.
//! Al reanudar:
//! 1. se listan/cargan desde disco para **mostrar** el historial en Atic,
//! 2. el proceso vivo usa `claude --resume` (contexto real del CLI),
//! 3. el adaptador mutea el replay por stdout para no duplicar.
//!
//! El JSONL es formato interno: el parser es best-effort (mensajes, tools,
//! thinking). Si Anthropic cambia el shape, el resume sigue andando; solo
//! puede degradarse la vista del historial.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use serde_json::Value;

use super::model::{Item, ItemKind, Role, ToolKind, ToolStatus, Turn, TurnStatus};
use super::skills::config_dir;

/// Una sesión del CLI, lista para `--resume`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeSession {
    pub id: String,
    /// Primer mensaje de usuario, si se pudo leer sin cargar el archivo entero.
    pub preview: String,
    /// Segundos desde epoch (mtime del `.jsonl`).
    pub updated_at: u64,
    pub cwd: String,
}

/// Codifica un path absoluto como lo hace Claude Code en `projects/`.
///
/// Docs: caracteres no alfanuméricos → `-`. En Windows, `C:\Users\…\atic`
/// queda `C--Users-…-atic`.
pub fn encode_project_key(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Normaliza el cwd a un path absoluto usable como clave.
fn absolute_cwd(cwd: &str) -> Option<PathBuf> {
    let raw = PathBuf::from(cwd.trim());
    if raw.as_os_str().is_empty() {
        return None;
    }
    let abs = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir().ok()?.join(raw)
    };
    let canon = abs.canonicalize().unwrap_or(abs);
    Some(strip_verbatim(canon))
}

/// Quita el prefijo `\\?\` de Windows, que rompería el encoding del CLI.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

fn projects_root() -> Option<PathBuf> {
    Some(config_dir()?.join("projects"))
}

/// Resuelve el directorio `projects/<key>` para este cwd (con fallback case-insensitive).
fn project_dir_for(cwd: &Path) -> Option<PathBuf> {
    let root = projects_root()?;
    let key = encode_project_key(&cwd.to_string_lossy());
    let direct = root.join(&key);
    if direct.is_dir() {
        return Some(direct);
    }
    // Windows a veces mezcla `C--` y `c--`.
    let lower = key.to_ascii_lowercase();
    let entries = fs::read_dir(&root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.eq_ignore_ascii_case(&key) || name.to_ascii_lowercase() == lower {
            let p = entry.path();
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

fn mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Lee como mucho ~64 KiB y busca el primer mensaje de usuario.
///
/// Solo para la etiqueta de la lista. Si el formato cambia, devolvemos el id
/// acortado y listo: reanudar no depende del preview.
fn peek_preview(path: &Path, fallback_id: &str) -> String {
    let Ok(file) = fs::File::open(path) else {
        return short_id(fallback_id);
    };
    let mut limited = file.take(64 * 1024);
    let mut buf = String::new();
    if limited.read_to_string(&mut buf).is_err() {
        return short_id(fallback_id);
    }
    for line in buf.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        if let Some(text) = extract_user_text(&v) {
            let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if t.is_empty() {
                continue;
            }
            if t.chars().count() > 120 {
                let cut: String = t.chars().take(117).collect();
                return format!("{cut}…");
            }
            return t;
        }
    }
    short_id(fallback_id)
}

fn extract_user_text(v: &serde_json::Value) -> Option<String> {
    let message = v.get("message")?;
    let content = message.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let arr = content.as_array()?;
    let mut parts = Vec::new();
    for part in arr {
        if part.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                parts.push(t);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn short_id(id: &str) -> String {
    let take = id.chars().take(8).collect::<String>();
    format!("sesión {take}")
}

fn session_jsonl(cwd: &str, session_id: &str) -> Option<PathBuf> {
    let abs = absolute_cwd(cwd)?;
    let dir = project_dir_for(&abs)?;
    let path = dir.join(format!("{session_id}.jsonl"));
    path.is_file().then_some(path)
}

/// Carga el transcript del CLI como turnos canónicos (solo para pintar).
///
/// Best-effort: ignora attachments/meta y sidechains. Tope de turnos para no
/// congelar la UI con sesiones enormes.
pub fn load_transcript(cwd: &str, session_id: &str) -> Result<Vec<Turn>, String> {
    let path = session_jsonl(cwd, session_id)
        .ok_or_else(|| format!("no hay transcript local para {session_id} en esa carpeta"))?;
    let file = fs::File::open(&path).map_err(|e| format!("no se pudo leer {path:?}: {e}"))?;
    let reader = BufReader::new(file);

    let mut turns: Vec<Turn> = Vec::new();
    let mut tool_at: HashMap<String, (usize, usize)> = HashMap::new();
    let mut seq = 0u64;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        match v.get("type").and_then(Value::as_str) {
            Some("user") if v.get("isCompactSummary").and_then(Value::as_bool) == Some(true) => {
                // Resumen sintético post-/compact: no es diálogo del usuario.
                apply_compact_summary(&v, &mut turns, &mut seq);
            }
            Some("user") => apply_user_line(&v, &mut turns, &mut tool_at, &mut seq),
            Some("assistant") => apply_assistant_line(&v, &mut turns, &mut tool_at, &mut seq),
            Some("system")
                if v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") =>
            {
                apply_compact_boundary(&v, &mut turns, &mut seq);
            }
            _ => {}
        }
        // Mantener solo los últimos N turnos en memoria mientras parseamos.
        const MAX_TURNS: usize = 60;
        if turns.len() > MAX_TURNS {
            let drop = turns.len() - MAX_TURNS;
            turns.drain(0..drop);
            tool_at.clear();
            // Reindexar tools del buffer restante.
            for (ti, turn) in turns.iter().enumerate() {
                for (ii, item) in turn.items.iter().enumerate() {
                    if matches!(item.kind, ItemKind::Tool { .. } | ItemKind::Collab { .. }) {
                        tool_at.insert(item.id.clone(), (ti, ii));
                    }
                }
            }
        }
    }

    // Cerrar el último turno abierto.
    if let Some(t) = turns.last_mut() {
        if t.status == TurnStatus::Running {
            t.status = TurnStatus::Done;
        }
    }
    Ok(turns)
}

fn new_turn(seq: &mut u64) -> Turn {
    *seq += 1;
    Turn {
        id: format!("cli-t{seq}"),
        items: Vec::new(),
        status: TurnStatus::Running,
        cost_usd: None,
    }
}

fn push_notice(turns: &mut Vec<Turn>, seq: &mut u64, text: String) {
    if turns.is_empty() || turns.last().map(|t| t.status) == Some(TurnStatus::Done) {
        turns.push(new_turn(seq));
    }
    let turn = turns.last_mut().expect("turn");
    *seq += 1;
    turn.items
        .push(Item::new(format!("cli-n{seq}"), ItemKind::Notice { text }));
}

fn apply_compact_boundary(v: &Value, turns: &mut Vec<Turn>, seq: &mut u64) {
    let pre = v
        .get("compactMetadata")
        .and_then(|m| m.get("preTokens"))
        .and_then(Value::as_u64);
    let text = match pre {
        Some(n) => format!("Contexto compactado (antes ~{n} tokens)."),
        None => "Contexto compactado.".to_string(),
    };
    if let Some(t) = turns.last_mut() {
        if t.status == TurnStatus::Running {
            t.status = TurnStatus::Done;
        }
    }
    push_notice(turns, seq, text);
    if let Some(t) = turns.last_mut() {
        t.status = TurnStatus::Done;
    }
}

fn apply_compact_summary(v: &Value, turns: &mut Vec<Turn>, seq: &mut u64) {
    let Some(text) = extract_user_text(v) else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    push_notice(turns, seq, format!("Resumen del contexto\n\n{text}"));
}

fn apply_user_line(
    v: &Value,
    turns: &mut Vec<Turn>,
    tool_at: &mut HashMap<String, (usize, usize)>,
    seq: &mut u64,
) {
    let content = v.get("message").and_then(|m| m.get("content"));
    // Resultados de tool: parchean items ya mostrados (pueden venir solos o
    // junto a texto; el texto se maneja abajo).
    if let Some(Value::Array(blocks)) = content {
        for b in blocks {
            if b.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let id = b
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if id.is_empty() {
                continue;
            }
            let is_error = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let output = render_tool_content(b.get("content"));
            if let Some(&(ti, ii)) = tool_at.get(&id) {
                if let Some(item) = turns.get_mut(ti).and_then(|t| t.items.get_mut(ii)) {
                    match &mut item.kind {
                        ItemKind::Tool {
                            status,
                            output: out,
                            ..
                        } => {
                            *status = if is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Completed
                            };
                            *out = output;
                        }
                        ItemKind::Collab {
                            status, summary, ..
                        } => {
                            *status = if is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Completed
                            };
                            *summary = output.chars().take(400).collect();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let Some(text) = extract_user_text(v) else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }

    if let Some(t) = turns.last_mut() {
        if t.status == TurnStatus::Running {
            t.status = TurnStatus::Done;
        }
    }
    let mut turn = new_turn(seq);
    *seq += 1;
    turn.items.push(Item::new(
        format!("cli-u{seq}"),
        ItemKind::Message {
            role: Role::User,
            text: text.to_string(),
            streaming: false,
        },
    ));
    turns.push(turn);
}

fn apply_assistant_line(
    v: &Value,
    turns: &mut Vec<Turn>,
    tool_at: &mut HashMap<String, (usize, usize)>,
    seq: &mut u64,
) {
    if turns.is_empty() || turns.last().map(|t| t.status) == Some(TurnStatus::Done) {
        turns.push(new_turn(seq));
    }
    let turn_i = turns.len() - 1;
    let turn = &mut turns[turn_i];

    let blocks = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for b in blocks {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = b.get("text").and_then(Value::as_str).unwrap_or("").trim();
                if text.is_empty() {
                    continue;
                }
                *seq += 1;
                turn.items.push(Item::new(
                    format!("cli-m{seq}"),
                    ItemKind::Message {
                        role: Role::Assistant,
                        text: text.to_string(),
                        streaming: false,
                    },
                ));
            }
            Some("thinking") => {
                let text = b
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if text.is_empty() {
                    continue;
                }
                *seq += 1;
                turn.items.push(Item::new(
                    format!("cli-r{seq}"),
                    ItemKind::Reasoning {
                        text: text.to_string(),
                        streaming: false,
                    },
                ));
            }
            Some("tool_use") => {
                let name = b
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let id = b
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let id = if id.is_empty() {
                    *seq += 1;
                    format!("cli-tool{seq}")
                } else {
                    id
                };
                let input = b.get("input").cloned().unwrap_or(Value::Null);
                let title = tool_title_from_input(&input, &name);
                let kind = if matches!(name.as_str(), "Task" | "Agent") {
                    let sub = input
                        .get("subagent_type")
                        .or_else(|| input.get("agent"))
                        .and_then(Value::as_str)
                        .unwrap_or("task")
                        .to_string();
                    ItemKind::Collab {
                        name: name.clone(),
                        title: title.clone(),
                        subagent_type: sub,
                        status: ToolStatus::InProgress,
                        summary: String::new(),
                        parent_turn_id: None,
                        creation_source: "provider_native".to_string(),
                    }
                } else {
                    let locations = tool_locs_from_input(&input);
                    ItemKind::Tool {
                        name: name.clone(),
                        title,
                        tool_kind: ToolKind::guess(&name),
                        status: ToolStatus::InProgress,
                        input,
                        output: String::new(),
                        locations,
                    }
                };
                let item_i = turn.items.len();
                turn.items.push(Item::new(id.clone(), kind));
                tool_at.insert(id, (turn_i, item_i));
            }
            _ => {}
        }
    }
}

fn tool_title_from_input(input: &Value, fallback: &str) -> String {
    let Some(o) = input.as_object() else {
        return fallback.to_string();
    };
    for key in [
        "file_path",
        "command",
        "pattern",
        "path",
        "url",
        "query",
        "description",
        "question",
    ] {
        if let Some(s) = o.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return s.chars().take(120).collect();
            }
        }
    }
    // AskUserQuestion: primer encabezado.
    if let Some(q) = o
        .get("questions")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    {
        if let Some(h) = q.get("header").and_then(Value::as_str) {
            return h.to_string();
        }
        if let Some(h) = q.get("question").and_then(Value::as_str) {
            return h.chars().take(120).collect();
        }
    }
    fallback.to_string()
}

fn tool_locs_from_input(input: &Value) -> Vec<String> {
    let Some(o) = input.as_object() else {
        return Vec::new();
    };
    ["file_path", "path", "notebook_path"]
        .iter()
        .filter_map(|k| o.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn render_tool_content(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Lista sesiones del CLI para un `cwd`, más recientes primero.
pub fn list_for_cwd(cwd: &str) -> Vec<ClaudeCodeSession> {
    let Some(abs) = absolute_cwd(cwd) else {
        return Vec::new();
    };
    let cwd_str = abs.to_string_lossy().to_string();
    let Some(dir) = project_dir_for(&abs) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // Los ids de sesión son UUID; ignorar basura / exports raros.
        if stem.len() < 32 || !stem.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            continue;
        }
        out.push(ClaudeCodeSession {
            id: stem.to_string(),
            preview: peek_preview(&path, stem),
            updated_at: mtime_secs(&path),
            cwd: cwd_str.clone(),
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    // Tope razonable para el picker; el CLI también muestra un recorte.
    out.truncate(40);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_windows_atic_path() {
        let key = encode_project_key(r"C:\Users\alice\Documents\atic");
        assert_eq!(key, "C--Users-alice-Documents-atic");
    }

    #[test]
    fn encode_unix_path() {
        assert_eq!(
            encode_project_key("/Users/alice/code/myapp"),
            "-Users-alice-code-myapp"
        );
    }
}
