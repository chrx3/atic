//! Letras sincronizadas del tema que suena, desde LRCLIB (lrclib.net), igual
//! que la pill de Atic (`apps/desktop/src-tauri/src/lyrics.rs`).
//!
//! Ni Windows ni las apps entregan la letra. LRCLIB es abierta, sin cuenta ni
//! clave, y la trae en LRC (una marca de tiempo por verso), que es lo que
//! permite seguir el tema. Solo viajan título, artista y duración. Se apaga
//! junto con las carátulas en línea: `PILL_MEDIA_ONLINE=0`.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

/// Un verso: desde qué segundo suena y qué dice (vacío entre estrofas).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: f32,
    pub text: String,
}

pub type Lines = Arc<Vec<Line>>;

/// Cuánto puede diferir la duración de la versión encontrada: más allá suele
/// ser otra versión (en vivo, remix, «slowed») y la letra no calzaría.
const MAX_DURATION_OFF: f32 = 3.0;

/// La letra de la pista que suena, cuando llega.
#[derive(Clone, Default)]
pub struct Slot {
    /// De qué pista es (título y artista).
    pub key: String,
    pub lines: Option<Lines>,
}

/// Lo que se le pide al hilo de letras.
pub struct Ask {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub duration: Option<f32>,
}

/// El hilo de letras: busca una por pista, guarda lo encontrado (también lo
/// que no está, para no repreguntar) y deja en `slot` la de la última pedida.
pub fn spawn(slot: Arc<Mutex<Slot>>, on_ready: impl Fn() + Send + 'static) -> mpsc::Sender<Ask> {
    let (tx, rx) = mpsc::channel::<Ask>();
    let _ = std::thread::Builder::new().name("letras".into()).spawn(move || {
        let mut cache: HashMap<String, Option<Lines>> = HashMap::new();
        while let Ok(mut ask) = rx.recv() {
            // Si se pasaron varias pistas seguidas, solo importa la última.
            while let Ok(newer) = rx.try_recv() {
                ask = newer;
            }
            if let Ok(mut s) = slot.lock() {
                *s = Slot { key: ask.key.clone(), lines: None };
            }
            let lines = match cache.get(&ask.key) {
                Some(hit) => hit.clone(),
                None => {
                    let found = fetch(&ask.title, &ask.artist, ask.duration).map(Arc::new);
                    if std::env::var_os("PILL_DEBUG").is_some() {
                        let n = found.as_ref().map_or(0, |l| l.len());
                        eprintln!("[medios] letra «{}»: {n} versos", ask.title);
                    }
                    if cache.len() > 256 {
                        cache.clear();
                    }
                    cache.insert(ask.key.clone(), found.clone());
                    found
                }
            };
            if let Ok(mut s) = slot.lock() {
                if s.key == ask.key {
                    s.lines = lines;
                }
            }
            on_ready();
        }
    });
    tx
}

/// Busca primero con el título tal cual y, si no, sin los agregados de
/// YouTube («(Official Video)», «ft. …»).
fn fetch(title: &str, artist: &str, duration: Option<f32>) -> Option<Vec<Line>> {
    let clean = super::itunes::clean_title(title);
    let mut tries = vec![(title.to_string(), artist.to_string())];
    if let Some((left, right)) = title.split_once(" - ") {
        tries.push((right.to_string(), left.to_string()));
    }
    if !clean.is_empty() && clean != title.to_lowercase() {
        tries.push((clean, artist.to_string()));
    }
    tries.into_iter().find_map(|(t, a)| {
        let url = format!(
            "https://lrclib.net/api/search?track_name={}&artist_name={}",
            super::itunes::encode(&t),
            super::itunes::encode(&a)
        );
        let body = super::itunes::fetch(&url)?;
        let hits: serde_json::Value = serde_json::from_slice(&body).ok()?;
        let lrc = pick(hits.as_array()?, duration)?;
        let lines = parse(&lrc);
        (!lines.is_empty()).then_some(lines)
    })
}

/// La versión con letra sincronizada cuya duración más se parece a la del tema.
fn pick(hits: &[serde_json::Value], duration: Option<f32>) -> Option<String> {
    hits.iter()
        .filter_map(|hit| {
            let lrc = hit.get("syncedLyrics")?.as_str()?.trim();
            if lrc.is_empty() {
                return None;
            }
            let got = hit.get("duration").and_then(|d| d.as_f64()).map(|d| d as f32);
            let off = match (duration, got) {
                (Some(want), Some(got)) => (want - got).abs(),
                _ => 0.0,
            };
            (off <= MAX_DURATION_OFF).then(|| (off, lrc.to_string()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, lrc)| lrc)
}

/// LRC: una o más marcas `[mm:ss.xx]` por verso. Las etiquetas de cabecera
/// (`[ar:…]`, `[offset:…]`) no son marcas y se ignoran.
pub fn parse(lrc: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(close) = body.find(']') else { break };
            let Some(at) = stamp(&body[..close]) else { break };
            stamps.push(at);
            rest = body[close + 1..].trim_start();
        }
        let text = rest.trim().to_string();
        lines.extend(stamps.into_iter().map(|at| Line { at, text: text.clone() }));
    }
    lines.sort_by(|a, b| a.at.total_cmp(&b.at));
    lines
}

fn stamp(tag: &str) -> Option<f32> {
    let (min, sec) = tag.split_once(':')?;
    let min: f32 = min.parse().ok()?;
    let sec: f32 = sec.replacen(':', ".", 1).parse().ok()?;
    (min.is_finite() && sec.is_finite()).then_some(min * 60.0 + sec)
}

/// El verso que suena en `position` (segundos), o `None` antes del primero.
pub fn index(lines: &[Line], position: f32) -> Option<usize> {
    lines.partition_point(|l| l.at <= position).checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_lrc_con_marcas_repetidas_y_cabeceras() {
        let lines = parse("[ar:Alguien]\n[00:12.50]Hola\n[00:05.00][01:00.00]Coro\n[00:20.00]\n");
        let at: Vec<f32> = lines.iter().map(|l| l.at).collect();
        assert_eq!(at, vec![5.0, 12.5, 20.0, 60.0]);
        assert_eq!(lines[0].text, "Coro");
        assert_eq!(lines[2].text, "");
    }

    #[test]
    fn el_verso_que_suena() {
        let lines = parse("[00:05.00]uno\n[00:10.00]dos");
        assert_eq!(index(&lines, 1.0), None);
        assert_eq!(index(&lines, 5.0), Some(0));
        assert_eq!(index(&lines, 9.9), Some(0));
        assert_eq!(index(&lines, 30.0), Some(1));
    }

    #[test]
    fn elige_la_version_de_duracion_mas_cercana() {
        let hits = serde_json::json!([
            { "duration": 200.0, "syncedLyrics": "[00:01.00]lejos" },
            { "duration": 181.0, "syncedLyrics": "[00:01.00]cerca" },
            { "duration": 180.0, "syncedLyrics": null },
            { "duration": 180.0, "syncedLyrics": "  " }
        ]);
        assert_eq!(pick(hits.as_array().unwrap(), Some(180.4)).as_deref(), Some("[00:01.00]cerca"));
        assert_eq!(pick(hits.as_array().unwrap(), Some(240.0)), None);
    }
}
