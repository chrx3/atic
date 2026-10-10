//! Importar un `.code-workspace` de VS Code como espacio nuevo
//! (`from_vscode` de la referencia, `src-tauri/src/workspace.rs:138`).
//!
//! El archivo es JSONC: admite comentarios y comas finales. Las carpetas
//! pueden ser relativas al archivo. Atic solo guarda las rutas: el nombre de
//! cada carpeta (`name`) y los ajustes (`settings`) no se llevan.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

/// Lo que se toma de un `.code-workspace`: el nombre sugerido (el del archivo)
/// y las carpetas, ya resueltas contra la del archivo.
#[derive(Debug, PartialEq)]
pub struct Imported {
    pub name: String,
    pub folders: Vec<PathBuf>,
}

/// Salta un comentario que empieza en `i` (`//` o `/* */`); devuelve dónde sigue,
/// o `None` si en `i` no hay uno.
fn skip_comment(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i) != Some(&'/') {
        return None;
    }
    match chars.get(i + 1) {
        Some('/') => Some(chars[i..].iter().position(|c| *c == '\n').map_or(chars.len(), |n| i + n)),
        Some('*') => {
            let mut j = i + 2;
            while j < chars.len() && !(chars[j] == '*' && chars.get(j + 1) == Some(&'/')) {
                j += 1;
            }
            Some((j + 2).min(chars.len()))
        }
        _ => None,
    }
}

/// El JSONC sin comentarios ni comas antes de `}` o `]`, listo para `serde_json`.
/// Respeta las cadenas (un `//` dentro de una URL no es un comentario).
fn strip_jsonc(raw: &str) -> String {
    let chars: Vec<char> = raw.trim_start_matches('\u{feff}').chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '\\' && i + 1 < chars.len() {
                    out.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if chars[i - 1] == '"' {
                    break;
                }
            }
        } else if let Some(next) = skip_comment(&chars, i) {
            i = next;
        } else if c == ',' {
            // Una coma seguida (salvo espacios y comentarios) de un cierre es una coma final.
            let mut j = i + 1;
            loop {
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                match skip_comment(&chars, j) {
                    Some(next) => j = next,
                    None => break,
                }
            }
            if !matches!(chars.get(j), Some('}') | Some(']')) {
                out.push(c);
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// La ruta sin `.` ni `..` (sin tocar el disco: `canonicalize` en Windows deja
/// el prefijo `\\?\`).
fn clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Lee el texto de un `.code-workspace`; `base` es la carpeta que lo contiene y
/// `stem` su nombre sin extensión (el nombre del espacio).
pub fn parse(raw: &str, base: &Path, stem: &str) -> Result<Imported, String> {
    let value: Value = serde_json::from_str(&strip_jsonc(raw)).map_err(|e| format!("No es un workspace válido: {e}"))?;
    let mut folders: Vec<PathBuf> = Vec::new();
    for entry in value.get("folders").and_then(Value::as_array).into_iter().flatten() {
        // Las carpetas remotas (`uri`) no existen en este equipo.
        let Some(path) = entry.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) else {
            continue;
        };
        let path = Path::new(path);
        let full = clean(&if path.is_absolute() { path.to_path_buf() } else { base.join(path) });
        if !folders.iter().any(|f| crate::space::workspaces::same(f, &full)) {
            folders.push(full);
        }
    }
    if folders.is_empty() {
        return Err("El workspace no tiene carpetas".into());
    }
    Ok(Imported { name: stem.to_string(), folders })
}

/// Lee un `.code-workspace` del disco.
pub fn read(path: &Path) -> Result<Imported, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Workspace");
    parse(&raw, path.parent().unwrap_or(Path::new(".")), stem)
}

/// El aviso de las carpetas que ya no existen («No se encontró: a, b»), si falta
/// alguna (`store.ts:102` de la referencia).
pub fn missing_note(folders: &[PathBuf]) -> Option<String> {
    let names: Vec<String> = folders.iter().filter(|f| !f.is_dir()).map(|f| crate::space::workspaces::short_name(f)).collect();
    (!names.is_empty()).then(|| format!("No se encontró: {}", names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_jsonc_con_comentarios_y_comas_finales() {
        let raw = r#"{
            // el proyecto
            "folders": [
                { "path": "app", "name": "Aplicación" }, /* relativa */
                { "path": "../libs/core", },
                { "path": "app" },
                { "uri": "vscode-remote://ssh/x" },
                { "path": "https://no//es-comentario" },
            ],
            "settings": { "editor.tabSize": 2, },
        }"#;
        let base = Path::new("work").join("proyecto");
        let imported = parse(raw, &base, "proyecto").unwrap();
        assert_eq!(imported.name, "proyecto");
        assert_eq!(
            imported.folders,
            vec![
                Path::new("work").join("proyecto").join("app"),
                Path::new("work").join("libs").join("core"),
                Path::new("work").join("proyecto").join("https:").join("no").join("es-comentario"),
            ]
        );
    }

    #[test]
    fn la_carpeta_punto_es_la_del_archivo_y_las_absolutas_se_respetan() {
        let abs = std::env::temp_dir();
        let raw = format!(r#"{{ "folders": [ {{ "path": "." }}, {{ "path": {} }} ] }}"#, serde_json::to_string(&abs.to_string_lossy()).unwrap());
        let imported = parse(&raw, Path::new("base"), "x").unwrap();
        assert_eq!(imported.folders, vec![PathBuf::from("base"), clean(&abs)]);
    }

    #[test]
    fn sin_carpetas_o_roto_es_un_error() {
        assert!(parse(r#"{ "folders": [] }"#, Path::new("."), "x").is_err());
        assert!(parse("no json", Path::new("."), "x").is_err());
        // El BOM del principio no estorba.
        assert!(parse("\u{feff}{ \"folders\": [{ \"path\": \"a\" }] }", Path::new("."), "x").is_ok());
    }

    #[test]
    fn avisa_de_las_carpetas_que_faltan() {
        let here = std::env::temp_dir();
        let gone = here.join("atic-no-existe-123456");
        assert_eq!(missing_note(&[here.clone()]), None);
        assert_eq!(missing_note(&[here, gone]).as_deref(), Some("No se encontró: atic-no-existe-123456"));
    }
}
