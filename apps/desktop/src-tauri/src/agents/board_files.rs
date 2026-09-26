//! Archivos en la pizarra de agentes: verlos adentro, abrirlos afuera y saber
//! qué cambió en la carpeta de cada consola.
//!
//! La pizarra pinta los archivos con el protocolo de assets, que solo sirve
//! lo que esté en su scope. Las carpetas de Atic ya van ahí desde el arranque;
//! un archivo que sueltas en la pizarra o que generó un agente se suma a mano,
//! de a uno, al pedir verlo.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use atic_core::MutexExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// Carpetas que no vale la pena recorrer: dependencias, builds y cachés.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "__pycache__",
    "venv",
    ".venv",
    ".git",
    ".svelte-kit",
    ".next",
    ".turbo",
    ".cache",
];
/// Topes del recorrido: una consola abierta en el home no debe colgar nada.
const MAX_DEPTH: usize = 8;
const MAX_VISITED: usize = 30_000;
const MAX_RESULTS: usize = 200;
/// Holgura para archivos escritos justo al abrir la consola.
const SLACK_MS: i64 = 2_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    /// `image`, `video`, `audio`, `pdf`, `text` u `other`.
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub size: u64,
    pub modified_ms: i64,
    /// Nació después de abrir la consola (no solo se modificó).
    pub created: bool,
}

pub(crate) fn kind_of(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "ico" => "image",
        "mp4" | "webm" | "mov" | "m4v" | "ogv" => "video",
        "mp3" | "wav" | "ogg" | "m4a" | "flac" | "aac" | "opus" => "audio",
        "pdf" => "pdf",
        "txt" | "md" | "markdown" | "json" | "jsonl" | "yaml" | "yml" | "toml" | "csv" | "tsv"
        | "log" | "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "svelte" | "vue" | "py"
        | "go" | "java" | "kt" | "c" | "h" | "cpp" | "hpp" | "cs" | "css" | "scss" | "html"
        | "htm" | "xml" | "sql" | "sh" | "ps1" | "bat" | "cmd" | "ini" | "env" | "php" | "rb"
        | "lua" | "dart" | "swift" => "text",
        _ => "other",
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Deja a la pizarra mostrar este archivo y dice qué es.
#[tauri::command]
pub fn board_allow_file(app: AppHandle, path: String) -> Result<BoardFile, String> {
    let target = PathBuf::from(&path);
    let meta = std::fs::metadata(&target).map_err(|e| format!("No se pudo abrir {path}: {e}"))?;
    if !meta.is_file() {
        return Err("Solo se pueden abrir archivos, no carpetas.".into());
    }
    app.asset_protocol_scope()
        .allow_file(&target)
        .map_err(|e| e.to_string())?;
    Ok(BoardFile {
        name: name_of(&target),
        kind: kind_of(&target),
        size: meta.len(),
        path,
    })
}

/// Muestra el archivo en el Explorador (o el Finder), seleccionado.
#[tauri::command]
pub fn board_reveal(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(PathBuf::from(path))
        .map_err(|e| e.to_string())
}

/// Lo abre con la app que el sistema tenga para ese tipo.
#[tauri::command]
pub fn board_open_external(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Lo que cambió en la carpeta de la consola desde que se abrió.
///
/// No sale del transcript del agente: un video que genera un comando de
/// shell no aparece como edición en ninguno. Mirar la carpeta atrapa todo,
/// lo haga quien lo haga.
#[tauri::command(async)]
pub fn console_changed_files(session: String) -> Result<Vec<ChangedFile>, String> {
    let (cwd, started_ms) = super::console::console_origin(&session)
        .ok_or_else(|| "esa consola ya no existe".to_string())?;
    if cwd.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(changed_since(Path::new(&cwd), started_ms - SLACK_MS))
}

fn skip_dir(name: &str) -> bool {
    SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(name))
}

pub(crate) fn changed_since(root: &Path, since_ms: i64) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_VISITED {
                break;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if depth < MAX_DEPTH && !skip_dir(&name) {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let modified = meta.modified().map(ms).unwrap_or(0);
            if modified < since_ms {
                continue;
            }
            let created = meta.created().map(ms).is_ok_and(|c| c >= since_ms);
            out.push(ChangedFile {
                name: name_of(&path),
                kind: kind_of(&path),
                size: meta.len(),
                modified_ms: modified,
                created,
                path: path.display().to_string(),
            });
        }
        if visited > MAX_VISITED {
            break;
        }
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    out.truncate(MAX_RESULTS);
    out
}

/// Consola que el launcher pidió abrir, esperando a que la pizarra la tome.
///
/// La ventana puede no existir todavía: el evento se perdería. La pizarra la
/// pide al montarse, y también al recibir el aviso.
static PENDING_CONSOLE: Mutex<Option<String>> = Mutex::new(None);

/// Abre la pizarra y le pide una consola nueva de `cli`.
pub(crate) fn request_new_console(app: &AppHandle, cli: &str) -> Result<(), String> {
    *PENDING_CONSOLE.lock_or_recover() = Some(cli.to_string());
    crate::agents_window::ensure_agents_window(app)?;
    let _ = app.emit_to(crate::agents_window::LABEL, "agents-new-console", cli);
    Ok(())
}

/// La consola que el launcher dejó pedida, una sola vez.
#[tauri::command]
pub fn agents_take_new_console() -> Option<String> {
    PENDING_CONSOLE.lock_or_recover().take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_tipo_sale_de_la_extension() {
        assert_eq!(kind_of(Path::new("a/video.MP4")), "video");
        assert_eq!(kind_of(Path::new("doc.pdf")), "pdf");
        assert_eq!(kind_of(Path::new("x.svelte")), "text");
        assert_eq!(kind_of(Path::new("foto.jpeg")), "image");
        assert_eq!(kind_of(Path::new("raro.xyz")), "other");
    }

    #[test]
    fn solo_lo_cambiado_despues_y_sin_dependencias() {
        let root = std::env::temp_dir().join(format!("atic-board-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("viejo.txt"), "x").unwrap();
        let since = ms(SystemTime::now()) + 1;
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("src/nuevo.rs"), "fn main(){}").unwrap();
        std::fs::write(root.join("node_modules/pkg/i.js"), "x").unwrap();
        let found: Vec<String> = changed_since(&root, since)
            .into_iter()
            .map(|f| f.name)
            .collect();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found, ["nuevo.rs"]);
    }

    #[test]
    fn el_pedido_del_launcher_se_toma_una_vez() {
        *PENDING_CONSOLE.lock_or_recover() = Some("codex".into());
        assert_eq!(agents_take_new_console().as_deref(), Some("codex"));
        assert_eq!(agents_take_new_console(), None);
    }
}
