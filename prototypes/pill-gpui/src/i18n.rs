//! Los textos de la UI en español o inglés, con las mismas claves que la app
//! de Tauri.
//!
//! - `assets/i18n/atic.{es,en}.json`: generados desde
//!   `apps/desktop/src/lib/core/i18n` con `scripts/i18n-export.mjs`. No se
//!   editan a mano.
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
