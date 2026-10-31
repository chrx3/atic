//! Los textos de la UI en español o inglés.
//!
//! - `assets/i18n/atic.{es,en}.json`: los textos que venían de la app de
//!   Tauri (ya retirada), con sus mismas claves. Ahora se editan a mano.
//! - `assets/i18n/pill.{es,en}.json`: las claves propias de la pill, a mano.
//!   Ganan sobre las de Atic si se repite una.
//!
//! Igual que en Tauri: si falta la clave en el idioma elegido se usa la de
//! español, y si tampoco está, la clave misma. `{nombre}` se reemplaza con
//! [`tf`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

type Table = HashMap<String, String>;

struct Tables {
    es: Table,
    en: Table,
}

static TABLES: OnceLock<Tables> = OnceLock::new();
static ENGLISH: AtomicBool = AtomicBool::new(false);

fn parse(sources: &[&str]) -> Table {
    let mut table = Table::new();
    for source in sources {
        if let Ok(map) = serde_json::from_str::<Table>(source) {
            table.extend(map);
        }
    }
    table
}

fn tables() -> &'static Tables {
    TABLES.get_or_init(|| Tables {
        es: parse(&[
            include_str!("../assets/i18n/atic.es.json"),
            include_str!("../assets/i18n/pill.es.json"),
        ]),
        en: parse(&[
            include_str!("../assets/i18n/atic.en.json"),
            include_str!("../assets/i18n/pill.en.json"),
        ]),
    })
}

/// Toma el idioma de `config.json` (`ui_language`: `system`, `es` o `en`).
pub fn init() {
    let stored = atic_core::AppDirs::new()
        .map(|dirs| atic_core::Config::load(&dirs.config_path()).ui_language)
        .unwrap_or_else(|_| "system".into());
    set_language(&atic_core::locale::resolve_ui_language(&stored));
}

/// `en` o `es`; cualquier otra cosa es español.
pub fn set_language(code: &str) {
    ENGLISH.store(code == "en", Ordering::Relaxed);
}

/// Si la UI está en inglés: quien arma textos una sola vez (los menús de
/// macOS) lo mira para rehacerlos al cambiar el idioma.
pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

fn lookup(key: &'static str) -> &'static str {
    let tables = tables();
    let found = if ENGLISH.load(Ordering::Relaxed) {
        tables.en.get(key).or_else(|| tables.es.get(key))
    } else {
        tables.es.get(key)
    };
    match found {
        Some(text) => text.as_str(),
        None => {
            tracing::debug!(key, "texto sin traducción");
            key
        }
    }
}

/// El texto de `key` en el idioma actual.
pub fn t(key: &'static str) -> &'static str {
    lookup(key)
}

/// El texto de `key` con `{nombre}` reemplazado por su valor.
pub fn tf(key: &'static str, vars: &[(&str, &dyn std::fmt::Display)]) -> String {
    interpolate(lookup(key), vars)
}

fn interpolate(template: &str, vars: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = template.to_owned();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_tablas_cargan_y_tienen_las_mismas_claves_de_la_pill() {
        let tables = tables();
        assert!(tables.es.len() > 1000, "faltan las claves generadas");
        let pill_es = parse(&[include_str!("../assets/i18n/pill.es.json")]);
        let pill_en = parse(&[include_str!("../assets/i18n/pill.en.json")]);
        let mut missing: Vec<_> = pill_es.keys().filter(|key| !pill_en.contains_key(*key)).collect();
        missing.sort();
        assert!(missing.is_empty(), "sin inglés: {missing:?}");
    }

    /// Cada `t("…")`/`tf("…")` del código tiene su texto en los dos idiomas:
    /// una clave mal escrita se vería tal cual en la UI.
    #[test]
    fn las_claves_del_codigo_existen_en_los_dos_idiomas() {
        fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }
        let tables = tables();
        let mut files = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let mut missing = Vec::new();
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            for start in ["t(\"", "tf(\""] {
                for (at, _) in source.match_indices(start) {
                    // Que sea la función y no el final de otro nombre (`list(`, `shift(`).
                    let before = source[..at].chars().next_back();
                    if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = &source[at + start.len()..];
                    let Some(end) = rest.find('"') else { continue };
                    let key = &rest[..end];
                    if key == "no.existe.esta.clave" || key.contains(char::is_whitespace) || !key.contains('.') {
                        continue;
                    }
                    if !tables.es.contains_key(key) || !tables.en.contains_key(key) {
                        missing.push(format!("{}: {key}", file.file_name().unwrap().to_string_lossy()));
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "claves sin texto: {missing:?}");
    }

    #[test]
    fn interpola_y_deja_lo_que_no_conoce() {
        let text = interpolate("Descargar {version} de {who}", &[("version", &"0.5")]);
        assert_eq!(text, "Descargar 0.5 de {who}");
    }

    #[test]
    fn sin_clave_devuelve_la_clave() {
        assert_eq!(t("no.existe.esta.clave"), "no.existe.esta.clave");
    }
}
