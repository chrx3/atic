//! Ajustes en vivo para desarrollo: `tweaks.json`, al lado de `Cargo.toml`,
//! cambia colores y números mientras corre la pill de dev, sin compilar. Se
//! guarda el archivo y las ventanas se redibujan al instante.
//!
//! ```json
//! {
//!   "colors": { "f0f0ea": "ff6b6b" },
//!   "numbers": { "notch.radius": 22 }
//! }
//! ```
//!
//! - `colors`: ese color, en toda la app, se ve como el otro. Pasa por
//!   `gpui::rgb`/`rgba` (el gancho de `vendor/gpui`) y por `theme::hsla`, así
//!   que vale para la pill, el notch, las ventanas y `gpui-m3`. Con el valor
//!   oscuro de `theme` (el que está en el código), también en el tema claro.
//! - `numbers`: lo que el código lea con `num("clave", valor)`. Se envuelve el
//!   número que se está afinando, se compila una vez y desde ahí se ajusta aquí.
//!
//! Cuando el ajuste queda bien, se pasa al código y se borra del archivo. En
//! release el archivo no se lee.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

const EVERY: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Tweaks {
    colors: HashMap<u32, u32>,
    numbers: HashMap<String, f32>,
}

static TWEAKS: RwLock<Option<Tweaks>> = RwLock::new(None);
/// Hay algo en el archivo: si no, `color` ni toma el candado.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn spawn() {
    if !cfg!(debug_assertions) {
        return;
    }
    gpui::set_rgb_hook(color);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tweaks.json");
    let started = std::thread::Builder::new()
        .name("tweaks-json".into())
        .spawn(move || {
            let mut seen: Option<SystemTime> = None;
            loop {
                let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
                if modified != seen {
                    seen = modified;
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    // A medio escribir el JSON puede no valer: queda lo de antes.
                    match parse(&text) {
                        Some(tweaks) => {
                            tracing::info!(
                                colors = tweaks.colors.len(),
                                numbers = tweaks.numbers.len(),
                                "tweaks.json aplicado"
                            );
                            ACTIVE.store(!tweaks.colors.is_empty() || !tweaks.numbers.is_empty(), Ordering::Relaxed);
                            if let Ok(mut current) = TWEAKS.write() {
                                *current = Some(tweaks);
                            }
                            crate::theme::restyle();
                        }
                        None => tracing::warn!("tweaks.json no es un JSON válido; sigue el anterior"),
                    }
                }
                std::thread::sleep(EVERY);
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no arrancó el seguimiento de tweaks.json");
    }
}

/// El color que toca (`0xRRGGBB`): el del archivo, o el mismo.
pub fn color(hex: u32) -> u32 {
    if !ACTIVE.load(Ordering::Relaxed) {
        return hex;
    }
    TWEAKS
        .read()
        .ok()
        .and_then(|tweaks| tweaks.as_ref()?.colors.get(&hex).copied())
        .unwrap_or(hex)
}

/// Un número afinable en vivo: `numbers.<key>` del archivo, o `default`.
/// Puede quedar sin llamadas entre una sesión de ajuste y otra.
#[allow(dead_code)]
pub fn num(key: &str, default: f32) -> f32 {
    if !ACTIVE.load(Ordering::Relaxed) {
        return default;
    }
    TWEAKS
        .read()
        .ok()
        .and_then(|tweaks| tweaks.as_ref()?.numbers.get(key).copied())
        .unwrap_or(default)
}

/// Vacío (o sin archivo) es sin ajustes; un JSON que no vale, `None`.
fn parse(text: &str) -> Option<Tweaks> {
    if text.trim().is_empty() {
        return Some(Tweaks::default());
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut tweaks = Tweaks::default();
    if let Some(colors) = value.get("colors").and_then(|c| c.as_object()) {
        for (from, to) in colors {
            match (hex(from), to.as_str().and_then(hex)) {
                (Some(from), Some(to)) => {
                    tweaks.colors.insert(from, to);
                }
                _ => tracing::warn!(%from, %to, "tweaks.json: color que no se entiende"),
            }
        }
    }
    if let Some(numbers) = value.get("numbers").and_then(|n| n.as_object()) {
        for (key, number) in numbers {
            if let Some(number) = number.as_f64() {
                tweaks.numbers.insert(key.clone(), number as f32);
            }
        }
    }
    Some(tweaks)
}

/// `f0f0ea`, `#f0f0ea` o `0xf0f0ea`.
fn hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#').trim_start_matches("0x");
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_colores_en_cualquier_forma_y_numeros() {
        let tweaks = parse(r##"{"colors": {"#F0F0EA": "0xff6b6b", "0f0f0e": "123456"}, "numbers": {"notch.radius": 22}}"##).unwrap();
        assert_eq!(tweaks.colors.get(&0xf0f0ea), Some(&0xff6b6b));
        assert_eq!(tweaks.colors.get(&0x0f0f0e), Some(&0x123456));
        assert_eq!(tweaks.numbers.get("notch.radius"), Some(&22.0));
    }

    #[test]
    fn archivo_vacio_es_sin_ajustes_y_json_roto_no_vale() {
        assert!(parse("  ").is_some_and(|t| t.colors.is_empty()));
        assert!(parse(r##"{"colors": {"f0f0ea": "#ff"##).is_none());
    }

    #[test]
    fn ignora_colores_que_no_son_hex_de_seis() {
        let tweaks = parse(r#"{"colors": {"fff": "ff0000", "f0f0ea": "rojo"}}"#).unwrap();
        assert!(tweaks.colors.is_empty());
    }
}
