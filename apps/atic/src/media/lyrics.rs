//! Letras sincronizadas del tema que suena, desde LRCLIB (lrclib.net) y, si
//! esa no la tiene o está caída, desde NetEase Cloud Music (music.163.com),
//! igual que Atic para Android (`Lyrics.kt`).
//!
//! Ni Windows ni las apps entregan la letra. Ambos servicios son abiertos, sin
//! cuenta ni clave, y la traen en LRC (una marca de tiempo por verso), que es
//! lo que permite seguir el tema. Solo viajan título y artista (y la duración
//! se compara aquí). Se apaga junto con las carátulas en línea:
//! `PILL_MEDIA_ONLINE=0`.

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
                None => match fetch(&ask.title, &ask.artist, ask.duration) {
                    Ok(found) => {
                        let found = found.map(Arc::new);
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
                    // Sin red o un servicio caído y el otro sin la letra: no se
                    // recuerda, para volver a preguntar la próxima vez.
                    Err(ProviderDown) => None,
                },
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

/// Un proveedor no respondió (sin red, caído o con una respuesta ilegible).
#[derive(Debug, PartialEq)]
struct ProviderDown;

/// `Ok(None)`: el proveedor respondió y no la tiene.
type Lookup = Result<Option<Vec<Line>>, ProviderDown>;

/// Primero LRCLIB y, si no la tiene o no respondió, NetEase.
fn fetch(title: &str, artist: &str, duration: Option<f32>) -> Lookup {
    let clean = super::itunes::clean_title(title);
    let mut tries = vec![(title.to_string(), artist.to_string())];
    if let Some((left, right)) = title.split_once(" - ") {
        tries.push((right.to_string(), left.to_string()));
    }
    if !clean.is_empty() && clean != title.to_lowercase() {
        tries.push((clean, artist.to_string()));
    }
    first_found([&|| from_lrclib(&tries, duration), &|| from_netease(&tries, duration)])
}

/// La primera letra que aparezca. Si alguno no respondió y ninguno la tiene,
/// es `Err`: «no la tiene» no es lo mismo que «no pude preguntar».
fn first_found<const N: usize>(providers: [&dyn Fn() -> Lookup; N]) -> Lookup {
    let mut down = false;
    for provider in providers {
        match provider() {
            Ok(Some(lines)) => return Ok(Some(lines)),
            Ok(None) => {}
            Err(ProviderDown) => down = true,
        }
    }
    if down { Err(ProviderDown) } else { Ok(None) }
}

/// LRCLIB, con el título tal cual y, si no, sin los agregados de YouTube
/// («(Official Video)», «ft. …»).
fn from_lrclib(tries: &[(String, String)], duration: Option<f32>) -> Lookup {
    let mut down = false;
    for (t, a) in tries {
        let url = format!(
            "https://lrclib.net/api/search?track_name={}&artist_name={}",
            super::itunes::encode(t),
            super::itunes::encode(a)
        );
        let Some(body) = super::itunes::fetch(&url) else {
            down = true;
            continue;
        };
        let Some(hits) = serde_json::from_slice::<serde_json::Value>(&body).ok() else {
            down = true;
            continue;
        };
        let Some(lrc) = hits.as_array().and_then(|hits| pick(hits, duration)) else { continue };
        let lines = without_translation(parse(&lrc));
        if !lines.is_empty() {
            return Ok(Some(lines));
        }
    }
    if down { Err(ProviderDown) } else { Ok(None) }
}

/// Cuántas letras de NetEase se piden como mucho por tema.
const NETEASE_TRIES: usize = 3;

/// NetEase: se busca «tema artista», se eligen los que calzan y se pide la
/// letra de cada uno.
fn from_netease(tries: &[(String, String)], duration: Option<f32>) -> Lookup {
    let mut down = false;
    for (t, a) in tries {
        // Sin «(Official Video)» y compañía, que confunden la búsqueda.
        let url = format!(
            "https://music.163.com/api/search/get?type=1&limit=10&s={}",
            super::itunes::encode(&format!("{} {a}", super::itunes::clean_title(t)))
        );
        let Some(body) = super::itunes::fetch(&url) else {
            down = true;
            continue;
        };
        let Some(found) = serde_json::from_slice::<serde_json::Value>(&body).ok() else {
            down = true;
            continue;
        };
        let Some(songs) = found.pointer("/result/songs").and_then(|s| s.as_array()) else { continue };
        for id in netease_matches(songs, t, a, duration).into_iter().take(NETEASE_TRIES) {
            let url = format!("https://music.163.com/api/song/lyric?id={id}&lv=1");
            let Some(body) = super::itunes::fetch(&url) else {
                down = true;
                continue;
            };
            let lrc = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.pointer("/lrc/lyric").and_then(|l| l.as_str().map(str::to_string)))
                .unwrap_or_default();
            let lines = without_translation(netease_clean(parse(&lrc)));
            if lines.iter().any(|l| !l.text.trim().is_empty()) {
                return Ok(Some(lines));
            }
        }
    }
    if down { Err(ProviderDown) } else { Ok(None) }
}

/// Los ids de NetEase cuyo título limpio es el nuestro y que comparten un
/// artista, dentro del margen de duración. Primero el nombre idéntico al
/// nuestro, después el que no trae agregados (una «(Versión Portugués)» puede
/// durar casi lo mismo) y, a igual nombre, la duración más cercana.
fn netease_matches(songs: &[serde_json::Value], title: &str, artist: &str, duration: Option<f32>) -> Vec<u64> {
    let want = super::itunes::clean_title(title);
    let ours = super::itunes::clean_title(artist);
    if want.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u8, f32, u64)> = Vec::new();
    for song in songs {
        let name = song.get("name").and_then(|n| n.as_str()).unwrap_or_default();
        if super::itunes::clean_title(name) != want {
            continue;
        }
        let same_artist = song.get("artists").and_then(|a| a.as_array()).is_some_and(|artists| {
            artists.iter().any(|artist| {
                let theirs = super::itunes::clean_title(artist.get("name").and_then(|n| n.as_str()).unwrap_or_default());
                !theirs.is_empty() && (ours.contains(&theirs) || theirs.contains(&ours))
            })
        });
        if !same_artist {
            continue;
        }
        let got = song.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0) as f32 / 1000.0;
        let off = match duration {
            Some(want) if got > 0.0 => (want - got).abs(),
            _ => 0.0,
        };
        let rank = if name.to_lowercase() == title.trim().to_lowercase() {
            0
        } else if name.contains('(') || name.contains('[') || name.contains(" - ") {
            2
        } else {
            1
        };
        if let Some(id) = song.get("id").and_then(|i| i.as_u64()).filter(|_| off <= MAX_DURATION_OFF) {
            hits.push((rank, off, id));
        }
    }
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    hits.into_iter().map(|(_, _, id)| id).collect()
}

/// NetEase pone los créditos como versos («作词 : …», «作曲 : …») y a veces
/// rótulos sueltos («[Verso 1: …]»); «纯音乐» es instrumental, sin letra.
fn netease_clean(lines: Vec<Line>) -> Vec<Line> {
    if lines.iter().any(|l| l.text.starts_with("纯音乐")) {
        return Vec::new();
    }
    let is_han = |c: char| matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}');
    let credit = |text: &str| {
        let han = text.chars().take_while(|&c| is_han(c)).count();
        (1..=6).contains(&han)
            && text.chars().skip(han).find(|c| !c.is_whitespace()).is_some_and(|c| c == ':' || c == '：')
    };
    lines
        .into_iter()
        .filter(|l| !credit(&l.text) && !(l.text.starts_with('[') && l.text.ends_with(']')))
        .collect()
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

/// Hay letras subidas con la traducción en el mismo verso: «original^
/// traducción» (NUEVAYoL en LRCLIB) o con «;». Si así viene la mayoría de
/// los versos, queda el original; un «;» de puntuación en algunos versos no
/// alcanza para tocarla. Igual que `Lyrics.withoutTranslation` del celular.
pub fn without_translation(lines: Vec<Line>) -> Vec<Line> {
    let sung = lines.iter().filter(|l| !l.text.trim().is_empty()).count();
    if sung == 0 {
        return lines;
    }
    for sep in ['^', ';'] {
        let split = |text: &str| {
            let (original, translation) = text.split_once(sep)?;
            let (original, translation) = (original.trim(), translation.trim());
            (!original.is_empty() && !translation.is_empty()).then(|| original.to_string())
        };
        if lines.iter().filter(|l| split(&l.text).is_some()).count() * 2 > sung {
            return lines.into_iter().map(|l| Line { text: split(&l.text).unwrap_or(l.text), at: l.at }).collect();
        }
    }
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
    fn una_letra_con_traduccion_en_cada_verso_queda_en_el_original() {
        let text = |lines: Vec<Line>| lines.into_iter().map(|l| l.text).collect::<Vec<_>>();
        // Así viene NUEVAYoL en LRCLIB: original^traducción.
        let caret = parse("[00:00.12] ¡NUEVAYoL!^NEW YORK!\n[00:20.75] Si te quieres divertir^If you want to have fun");
        assert_eq!(text(without_translation(caret)), ["¡NUEVAYoL!", "Si te quieres divertir"]);

        let semicolon = parse("[00:01.00]Hola mi amor; Hello my love\n[00:02.00]Te extraño; I miss you\n[00:03.00]");
        assert_eq!(text(without_translation(semicolon)), ["Hola mi amor", "Te extraño", ""]);

        // Un «;» de puntuación en un verso no la toca.
        let normal = parse("[00:01.00]Por la mañana, café; por la tarde, ron\n[00:02.00]Otra\n[00:03.00]Más");
        assert_eq!(without_translation(normal.clone()), normal);
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

    fn some(text: &str) -> Lookup {
        Ok(Some(vec![Line { at: 1.0, text: text.into() }]))
    }

    #[test]
    fn si_el_primero_no_la_tiene_se_pregunta_al_segundo() {
        assert_eq!(first_found([&|| Ok(None), &|| some("b")]), some("b"));
    }

    #[test]
    fn si_el_primero_esta_caido_se_pregunta_al_segundo() {
        assert_eq!(first_found([&|| Err(ProviderDown), &|| some("b")]), some("b"));
    }

    #[test]
    fn si_el_primero_la_tiene_no_se_pregunta_al_segundo() {
        let second = || -> Lookup { panic!("no debía preguntarse") };
        assert_eq!(first_found([&|| some("a"), &second]), some("a"));
    }

    #[test]
    fn sin_letra_en_ninguno_es_un_no_hay_solo_si_todos_respondieron() {
        assert_eq!(first_found([&|| Ok(None), &|| Ok(None)]), Ok(None));
        // Uno caído y el otro sin la letra: no se sabe, así que no se recuerda.
        assert_eq!(first_found([&|| Err(ProviderDown), &|| Ok(None)]), Err(ProviderDown));
        assert_eq!(first_found([&|| Ok(None), &|| Err(ProviderDown)]), Err(ProviderDown));
    }

    #[test]
    fn netease_elige_el_nombre_y_artista_que_calzan() {
        let songs = serde_json::json!([
            { "id": 1, "name": "Despacito (Remix)", "duration": 229000, "artists": [{ "name": "Luis Fonsi" }] },
            { "id": 2, "name": "Despacito", "duration": 230413, "artists": [{ "name": "Luis Fonsi" }, { "name": "Daddy Yankee" }] },
            { "id": 3, "name": "Despacito", "duration": 230000, "artists": [{ "name": "Otro" }] },
            { "id": 4, "name": "Despacito", "duration": 300000, "artists": [{ "name": "Luis Fonsi" }] },
            { "id": 5, "name": "Despacito - Live", "duration": 230000, "artists": [{ "name": "Luis Fonsi" }] }
        ]);
        let songs = songs.as_array().unwrap();
        // El nombre idéntico primero, luego el que trae «(Remix)»; sin otro artista,
        // otra duración ni otro título («Despacito - Live» no es «Despacito»).
        assert_eq!(netease_matches(songs, "Despacito", "Luis Fonsi", Some(230.0)), vec![2, 1]);
        // Sin título limpio no hay con qué comparar.
        assert!(netease_matches(songs, "(Official Video)", "Luis Fonsi", None).is_empty());
    }

    #[test]
    fn netease_quita_creditos_y_rotulos_y_descarta_instrumentales() {
        let text = |lines: Vec<Line>| lines.into_iter().map(|l| l.text).collect::<Vec<_>>();
        let lrc = "[00:00.00]作词 : Alguien\n[00:01.00]作曲：Otro\n[00:02.00][Verso 1: X]\n[00:03.00]Hola\n[00:04.00]";
        assert_eq!(text(netease_clean(parse(lrc))), ["Hola", ""]);
        assert!(netease_clean(parse("[00:00.00]纯音乐，请欣赏\n[00:01.00]x")).is_empty());
        // Un verso que empieza con pocos caracteres chinos pero sin «:» no es un crédito.
        assert_eq!(text(netease_clean(parse("[00:01.00]我爱你 mi amor"))), ["我爱你 mi amor"]);
    }
}
