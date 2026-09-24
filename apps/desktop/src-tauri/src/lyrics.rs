//! Letras sincronizadas del tema que suena, desde LRCLIB (lrclib.net).
//!
//! Ni el SO ni las apps entregan la letra: Spotify y YT Music la sacan de APIs
//! privadas. LRCLIB es abierta, sin cuenta ni clave, y trae la letra en LRC
//! (una marca de tiempo por línea), que es lo que permite seguir el tema.
//! Solo se viajan título, artista y duración.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;

const SEARCH_URL: &str = "https://lrclib.net/api/search";
/// Cuánto puede diferir la duración de la versión encontrada: más allá suele
/// ser otra versión (en vivo, remix) y la letra no calzaría con el tiempo.
const MAX_DURATION_OFF_S: f64 = 3.0;
/// Temas recordados; al pasarse se olvidan todos (una sesión rara vez llega).
const CACHE_MAX: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    duration: Option<f64>,
    synced_lyrics: Option<String>,
}

/// Lo ya preguntado, con o sin letra: un tema sin letra no se vuelve a buscar.
fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// La letra sincronizada (LRC) del tema, o `None` si LRCLIB no la tiene.
#[tauri::command]
pub async fn media_lyrics(
    title: String,
    artist: String,
    duration_ms: Option<u64>,
) -> Result<Option<String>, String> {
    let title = title.trim().to_string();
    let artist = artist.trim().to_string();
    if title.is_empty() {
        return Ok(None);
    }
    let key = format!(
        "{title}\u{1}{artist}\u{1}{}",
        duration_ms.unwrap_or(0) / 1000
    );
    if let Some(hit) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Ok(hit.clone());
    }

    let lyrics = tauri::async_runtime::spawn_blocking(move || {
        search(&title, &artist, duration_ms.map(|ms| ms as f64 / 1000.0))
    })
    .await
    .map_err(|e| e.to_string())??;

    // Los errores de red no se guardan: el próximo intento puede andar.
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= CACHE_MAX {
        cache.clear();
    }
    cache.insert(key, lyrics.clone());
    Ok(lyrics)
}

fn search(title: &str, artist: &str, duration_s: Option<f64>) -> Result<Option<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("Atic/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}"))?;
    let mut query = vec![("track_name", title)];
    if !artist.is_empty() {
        query.push(("artist_name", artist));
    }
    let hits: Vec<Hit> = client
        .get(SEARCH_URL)
        .query(&query)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("no se pudo consultar LRCLIB: {e}"))?
        .json()
        .map_err(|e| format!("respuesta inesperada de LRCLIB: {e}"))?;
    Ok(pick(hits, duration_s))
}

/// La versión con letra sincronizada cuya duración más se parece a la del tema.
fn pick(hits: Vec<Hit>, duration_s: Option<f64>) -> Option<String> {
    hits.into_iter()
        .filter_map(|hit| {
            let lyrics = hit.synced_lyrics.filter(|s| !s.trim().is_empty())?;
            let off = match (duration_s, hit.duration) {
                (Some(want), Some(got)) => (want - got).abs(),
                _ => 0.0,
            };
            (off <= MAX_DURATION_OFF_S).then_some((off, lyrics))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, lyrics)| lyrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(duration: f64, lyrics: Option<&str>) -> Hit {
        Hit {
            duration: Some(duration),
            synced_lyrics: lyrics.map(str::to_string),
        }
    }

    #[test]
    fn elige_la_version_de_duracion_mas_cercana() {
        let hits = vec![
            hit(200.0, Some("[00:01.00]lejos")),
            hit(181.0, Some("[00:01.00]cerca")),
            hit(180.0, None),
        ];
        assert_eq!(pick(hits, Some(180.4)).as_deref(), Some("[00:01.00]cerca"));
    }

    #[test]
    fn descarta_versiones_de_otra_duracion_o_sin_letra() {
        let hits = vec![
            hit(240.0, Some("[00:01.00]en vivo")),
            hit(180.0, Some("  ")),
        ];
        assert_eq!(pick(hits, Some(180.0)), None);
    }
}
