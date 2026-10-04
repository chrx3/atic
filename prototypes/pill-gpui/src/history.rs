//! El historial real de Atic, solo lectura: `history.json` de
//! `clipboard_history.rs` y los PNG que apunta.
//!
//! Atic sigue siendo el dueño del archivo. El prototipo nunca lo escribe:
//! favoritos y borrados quedan en memoria (ver `ClipboardPanel`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use gpui::{hsla, rgb, Hsla, SharedString};
use serde::Deserialize;

use crate::clipboard::{Content, Entry, Picture};

/// Cuánto texto se pinta en la fila; el resto solo sirve para buscar.
const PREVIEW_CHARS: usize = 240;
/// `COLOR_MAX_LEN` de `colorMath.ts`.
const COLOR_MAX_LEN: usize = 32;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Text,
    Image,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: String,
    kind: Kind,
    preview: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    image_path: Option<String>,
    created_at_ms: u64,
    #[serde(default)]
    pinned: bool,
    /// El `.exe` que copió (`sourceApp` del watcher de Atic).
    #[serde(default)]
    source_app: Option<String>,
}

/// Versión del archivo en disco: Atic lo reescribe entero (`write_atomic_str`),
/// así que fecha y largo bastan para saber si cambió.
#[derive(Clone, Copy, PartialEq)]
pub struct Stamp {
    modified: SystemTime,
    len: u64,
}

/// `%APPDATA%\ciat\atic\data\clipboard`, o `ATIC_CLIPBOARD_DIR` para apuntar a
/// otra copia.
pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ATIC_CLIPBOARD_DIR") {
        return Some(PathBuf::from(dir));
    }
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("ciat")
            .join("atic")
            .join("data")
            .join("clipboard"),
    )
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: meta.modified().ok()?,
        len: meta.len(),
    })
}

/// Las entradas si el archivo cambió desde `seen`. `None` si sigue igual, no
/// existe o no se pudo leer; un JSON a medio escribir se reintenta en la
/// próxima vuelta.
pub fn load_if_changed(dir: &Path, seen: Option<Stamp>) -> Option<(Stamp, Vec<Entry>)> {
    let path = dir.join("history.json");
    let before = stamp(&path)?;
    if seen == Some(before) {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    let items: Vec<Item> = match serde_json::from_str(&raw) {
        Ok(items) => items,
        Err(error) => {
            eprintln!("history.json ilegible: {error}");
            return None;
        }
    };
    let entries = items
        .into_iter()
        .enumerate()
        .filter_map(|(id, item)| entry(id, item))
        .collect();
    Some((before, entries))
}

fn entry(id: usize, item: Item) -> Option<Entry> {
    // Esto corre en un hilo de fondo: extraer el ícono aquí no traba la UI.
    let source_icon = item
        .source_app
        .and_then(|path| crate::app_icon::icon_for(Path::new(&path)));
    let entry = match item.kind {
        Kind::Image => {
            let path = item.image_path?;
            // Atic borra sus capturas por antigüedad pero el historial sigue
            // apuntándolas: sin archivo no hay nada que mostrar ni pegar.
            if !Path::new(&path).exists() {
                return None;
            }
            Entry::new(
                id,
                item.id.into(),
                Content::Image(Picture::File(Path::new(&path).into())),
                &item.preview,
            )
        }
        Kind::Text => {
            let text = item.text.unwrap_or(item.preview);
            let content = match parse_color(&text) {
                Some(color) => Content::Color(text.trim().to_string().into(), color),
                None => Content::Text(text.clone().into()),
            };
            Entry::new(id, item.id.into(), content, &text)
        }
    };
    Some(Entry {
        created_ms: item.created_at_ms,
        pinned: item.pinned,
        source_icon,
        ..entry
    })
}

/// Espacios y saltos colapsados, como el `white-space: nowrap` de la fila.
pub fn one_line(text: &str) -> SharedString {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if out.chars().count() >= PREVIEW_CHARS {
            out.push('…');
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.into()
}

/// Grupo de la lista: los separadores «Hoy», «Ayer» y «Antes».
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Day {
    Today,
    Yesterday,
    Earlier,
}

impl Day {
    pub fn label(self) -> &'static str {
        match self {
            Day::Today => "Hoy",
            Day::Yesterday => "Ayer",
            Day::Earlier => "Antes",
        }
    }
}

fn local(created_ms: u64) -> Option<DateTime<Local>> {
    Local.timestamp_millis_opt(created_ms as i64).single()
}

pub fn day_of(created_ms: u64, now: DateTime<Local>) -> Day {
    match local(created_ms).map(|value| (now.date_naive() - value.date_naive()).num_days()) {
        Some(0) => Day::Today,
        Some(1) => Day::Yesterday,
        _ => Day::Earlier,
    }
}

/// La hora de la columna derecha: el grupo ya dice el día, así que hoy y ayer
/// van solo con la hora; lo anterior, con el día de la semana o la fecha.
pub fn short_when(created_ms: u64, now: DateTime<Local>) -> SharedString {
    let Some(value) = local(created_ms) else {
        return SharedString::default();
    };
    const WEEKDAYS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
    const MONTHS: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
    ];
    match (now.date_naive() - value.date_naive()).num_days() {
        0 | 1 => format!("{:02}:{:02}", value.hour(), value.minute()).into(),
        2..=6 => WEEKDAYS[value.weekday().num_days_from_monday() as usize].into(),
        _ => format!("{} {}", value.day(), MONTHS[value.month0() as usize]).into(),
    }
}

/// `parseCssColor` de `colorMath.ts`: hex de 3, 4, 6 u 8 dígitos (sin `#`
/// solo 6) y `rgb()`/`hsl()` con coma o espacio. El alfa se ignora.
pub fn parse_color(raw: &str) -> Option<Hsla> {
    let value = raw.trim();
    if value.is_empty() || value.len() > COLOR_MAX_LEN {
        return None;
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    if !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        if !value.starts_with('#') && hex.len() != 6 {
            return None;
        }
        let full = match hex.len() {
            3 | 4 => hex[..3].chars().flat_map(|ch| [ch, ch]).collect::<String>(),
            6 | 8 => hex[..6].to_string(),
            _ => return None,
        };
        return u32::from_str_radix(&full, 16).ok().map(|n| rgb(n).into());
    }

    let open = value.find('(')?;
    let body = value[open + 1..].strip_suffix(')')?;
    let name = value[..open].trim().to_ascii_lowercase();
    let cut = body.split('/').next().unwrap_or_default();
    let parts: Vec<&str> = if cut.contains(',') {
        cut.split(',').map(str::trim).filter(|p| !p.is_empty()).collect()
    } else {
        cut.split_whitespace().collect()
    };
    if !(parts.len() == 3 || parts.len() == 4) || body.contains('(') {
        return None;
    }
    match name.as_str() {
        "rgb" | "rgba" => {
            let r = channel(parts[0], 255.)?;
            let g = channel(parts[1], 255.)?;
            let b = channel(parts[2], 255.)?;
            let n = ((r.round() as u32) << 16) | ((g.round() as u32) << 8) | b.round() as u32;
            Some(rgb(n).into())
        }
        "hsl" | "hsla" => {
            let h: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
            if !h.is_finite() {
                return None;
            }
            let s = channel(parts[1], 1.)?;
            let l = channel(parts[2], 1.)?;
            Some(hsla(h.rem_euclid(360.) / 360., s, l, 1.))
        }
        _ => None,
    }
}

/// Un canal: número suelto o porcentaje; `max` es lo que vale el 100 %.
fn channel(raw: &str, max: f32) -> Option<f32> {
    let text = raw.trim();
    let (number, percent) = match text.strip_suffix('%') {
        Some(number) => (number, true),
        None => (text, false),
    };
    let n: f32 = number.parse().ok()?;
    let value = if percent { n / 100. * max } else { n };
    (n.is_finite() && (0.0..=max).contains(&value)).then_some(value)
}

/// Para las entradas de prueba: imagen embebida en el binario.
pub fn embedded(bytes: &'static [u8]) -> Picture {
    Picture::Embedded(Arc::new(gpui::Image::from_bytes(
        gpui::ImageFormat::Png,
        bytes.to_vec(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colores_como_atic() {
        assert!(parse_color("#e85a52").is_some());
        assert!(parse_color("#abc").is_some());
        assert!(parse_color("e85a52").is_some());
        assert!(parse_color("rgb(212, 168, 75)").is_some());
        assert!(parse_color("rgb(212 168 75 / 50%)").is_some());
        assert!(parse_color("hsl(120, 50%, 40%)").is_some());
        // Sin almohadilla, solo seis dígitos: "123" es un número.
        assert!(parse_color("123").is_none());
        assert!(parse_color("rgb(300, 0, 0)").is_none());
        assert!(parse_color("Reunión el jueves").is_none());
    }

    #[test]
    fn grupos_y_horas() {
        let now = Local.with_ymd_and_hms(2026, 10, 2, 19, 0, 0).unwrap();
        let ms = |d: DateTime<Local>| d.timestamp_millis() as u64;
        let today = ms(Local.with_ymd_and_hms(2026, 10, 2, 9, 5, 0).unwrap());
        let yesterday = ms(Local.with_ymd_and_hms(2026, 10, 1, 23, 59, 0).unwrap());
        let monday = ms(Local.with_ymd_and_hms(2026, 9, 28, 8, 0, 0).unwrap());
        let old = ms(Local.with_ymd_and_hms(2026, 9, 3, 14, 30, 0).unwrap());
        assert_eq!(day_of(today, now), Day::Today);
        assert_eq!(day_of(yesterday, now), Day::Yesterday);
        assert_eq!(day_of(monday, now), Day::Earlier);
        assert_eq!(short_when(today, now), "09:05");
        assert_eq!(short_when(yesterday, now), "23:59");
        assert_eq!(short_when(monday, now), "lun");
        assert_eq!(short_when(old, now), "3 sept");
    }

    #[test]
    fn una_linea() {
        assert_eq!(one_line("  hola\n\n  mundo\t!"), "hola mundo !");
    }
}
