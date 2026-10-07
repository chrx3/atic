//! Tasas de cambio para el conversor del launcher.
//!
//! Opt-in (`config.launcher_currency`, apagado por defecto): con el
//! interruptor apagado este módulo no toca la red. Fuentes: mindicador.cl
//! (valores oficiales publicados por el Banco Central: UF, UTM, dólar
//! observado y euro) y exchangerate-api.com (tabla de mercado, sin key, para
//! los cruces que no son CLP). La última descarga buena queda en
//! `fx-rates.json` y en RAM: la búsqueda del launcher lee RAM y nunca espera
//! a la red, y lo que el usuario escribe no se envía a ninguna parte.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atic_core::MutexExt;

use crate::calc::{Freshness, RatesLookup};

const MINDICADOR_URL: &str = "https://mindicador.cl/api";
const ERAPI_URL: &str = "https://open.er-api.com/v6/latest/USD";

/// Cada cuánto se considera vieja la tabla (las fuentes publican a diario).
const FRESH_SECS: i64 = 24 * 3600;
/// Entre intentos: no machacar las APIs si no hay red.
const RETRY_SECS: i64 = 10 * 60;
const TIMEOUT_SECS: u64 = 8;

/// Una tasa: pesos chilenos por una unidad de la moneda.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RateEntry {
    clp_per_unit: f64,
    /// Día de la tasa (`YYYY-MM-DD`), tal como lo publica la fuente.
    date: String,
    /// `bcch` (valor oficial) | `market` (tabla de mercado).
    origin: String,
}

/// Tabla completa, tal como se cachea en disco.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Rates {
    entries: HashMap<String, RateEntry>,
    /// Última descarga exitosa (unix segundos).
    fetched_at: i64,
    /// El último intento falló; se conserva la tabla anterior avisando.
    #[serde(default)]
    last_error: bool,
}

impl RatesLookup for Rates {
    fn clp_per_unit(&self, code: &str) -> Option<(f64, &str, &str)> {
        self.entries.get(code).map(|entry| {
            (
                entry.clp_per_unit,
                entry.date.as_str(),
                entry.origin.as_str(),
            )
        })
    }

    fn freshness(&self) -> Freshness {
        if self.last_error {
            return Freshness::Offline;
        }
        if now_unix() - self.fetched_at > FRESH_SECS {
            return Freshness::Stale;
        }
        Freshness::Fresh
    }
}

// ---------------------------------------------------------------------------
// Estado global: una sola tabla en RAM, leída por la búsqueda.
// ---------------------------------------------------------------------------

static CACHE: OnceLock<Mutex<Option<Arc<Rates>>>> = OnceLock::new();
static REFRESHING: AtomicBool = AtomicBool::new(false);
static LAST_ATTEMPT: AtomicI64 = AtomicI64::new(0);

fn cache() -> &'static Mutex<Option<Arc<Rates>>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Tabla lista para convertir, si ya hay alguna (disco o red).
pub fn snapshot() -> Option<Arc<Rates>> {
    cache().lock_or_recover().clone()
}

/// Arranque: carga la caché de disco (`fx-rates.json`) y, si el opt-in está
/// encendido y la tabla está vieja, refresca en background.
pub fn init(path: &Path, enabled: bool) {
    load_from_disk(path);
    if enabled {
        refresh_if_stale(path.to_path_buf());
    }
}

/// Refresca si la tabla no existe o quedó vieja. Nunca bloquea a quien llama:
/// el intento va a un hilo propio y está limitado por [`RETRY_SECS`].
pub fn refresh_if_stale(path: PathBuf) {
    let stale = match snapshot() {
        Some(rates) => rates.freshness() != Freshness::Fresh,
        None => true,
    };
    if !stale {
        return;
    }
    let now = now_unix();
    if now - LAST_ATTEMPT.load(Ordering::Relaxed) < RETRY_SECS {
        return;
    }
    LAST_ATTEMPT.store(now, Ordering::Relaxed);
    std::thread::spawn(move || {
        let _ = refresh_now(&path);
    });
}

/// Descarga y publica. El guardia de `REFRESHING` evita dos descargas a la vez.
fn refresh_now(path: &Path) -> Result<(), String> {
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let _guard = RefreshGuard;
    match fetch_all() {
        Ok(mut rates) => {
            rates.fetched_at = now_unix();
            rates.last_error = false;
            save_to_disk(path, &rates);
            let count = rates.entries.len();
            *cache().lock_or_recover() = Some(Arc::new(rates));
            tracing::info!(monedas = count, "fx: tasas actualizadas");
            Ok(())
        }
        Err(err) => {
            // La tabla anterior se conserva: una tasa vieja con su fecha es más
            // útil que ningún número, y el subtítulo avisa que está sin conexión.
            {
                let mut guard = cache().lock_or_recover();
                if let Some(prev) = guard.as_ref() {
                    let mut next = (**prev).clone();
                    next.last_error = true;
                    *guard = Some(Arc::new(next));
                }
            }
            tracing::warn!(error = %err, "fx: no se pudieron actualizar las tasas");
            Err(err)
        }
    }
}

struct RefreshGuard;

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Descarga y mezcla de fuentes
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct MindicadorValue {
    valor: Option<f64>,
    fecha: Option<String>,
}

#[derive(serde::Deserialize)]
struct Mindicador {
    uf: Option<MindicadorValue>,
    utm: Option<MindicadorValue>,
    dolar: Option<MindicadorValue>,
    euro: Option<MindicadorValue>,
}

#[derive(serde::Deserialize)]
struct ErApi {
    rates: HashMap<String, f64>,
    time_last_update_unix: Option<i64>,
}

fn fetch_all() -> Result<Rates, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .user_agent(concat!("Atic/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;

    let official = fetch_mindicador(&client).map_err(|e| format!("mindicador: {e}"));
    let market = fetch_erapi(&client).map_err(|e| format!("mercado: {e}"));
    match (official, market) {
        (Err(official_err), Err(market_err)) => Err(format!("{official_err}; {market_err}")),
        (official, market) => Ok(merge_rates(official.ok(), market.ok(), &today_iso())),
    }
}

fn fetch_mindicador(
    client: &reqwest::blocking::Client,
) -> Result<Vec<(String, RateEntry)>, String> {
    let body = client
        .get(MINDICADOR_URL)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|resp| resp.text())
        .map_err(|e| e.to_string())?;
    parse_mindicador(&body)
}

fn fetch_erapi(client: &reqwest::blocking::Client) -> Result<Vec<(String, RateEntry)>, String> {
    let body = client
        .get(ERAPI_URL)
        .send()
        .and_then(|resp| resp.error_for_status())
        .and_then(|resp| resp.text())
        .map_err(|e| e.to_string())?;
    parse_erapi(&body)
}

/// Valores oficiales del Banco Central (vía mindicador): todos en CLP.
fn parse_mindicador(json: &str) -> Result<Vec<(String, RateEntry)>, String> {
    let raw: Mindicador = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let today = today_iso();
    let mut out = Vec::new();
    for (code, item) in [
        ("UF", raw.uf),
        ("UTM", raw.utm),
        ("USD", raw.dolar),
        ("EUR", raw.euro),
    ] {
        let Some(item) = item else {
            continue;
        };
        let Some(value) = item.valor.filter(|v| v.is_finite() && *v > 0.0) else {
            continue;
        };
        let date = item
            .fecha
            .as_deref()
            .map(iso_day)
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| today.clone());
        out.push((
            code.to_string(),
            RateEntry {
                clp_per_unit: value,
                date,
                origin: "bcch".into(),
            },
        ));
    }
    if out.is_empty() {
        return Err("sin valores oficiales".into());
    }
    Ok(out)
}

/// Tabla de mercado con base USD: se pasa todo a pesos por unidad. La fecha es
/// la de la tabla, no la del día local (puede venir de ayer en Chile).
fn parse_erapi(json: &str) -> Result<Vec<(String, RateEntry)>, String> {
    let raw: ErApi = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let usd_clp = raw
        .rates
        .get("CLP")
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or("la tabla no trae CLP")?;
    let date = raw
        .time_last_update_unix
        .and_then(unix_day)
        .unwrap_or_else(today_iso);
    let mut out = Vec::new();
    for &(code, _) in crate::calc::currencies() {
        // CLP es la base; UF y UTM solo tienen valor oficial, no de mercado.
        if matches!(code, "CLP" | "UF" | "UTM") {
            continue;
        }
        let Some(per_usd) = raw.rates.get(code).copied() else {
            continue;
        };
        if !per_usd.is_finite() || per_usd <= 0.0 {
            continue;
        }
        out.push((
            code.to_string(),
            RateEntry {
                clp_per_unit: usd_clp / per_usd,
                date: date.clone(),
                origin: "market".into(),
            },
        ));
    }
    if out.is_empty() {
        return Err("la tabla de mercado no trae monedas conocidas".into());
    }
    Ok(out)
}

/// Mezcla las dos fuentes: el valor oficial gana cuando existe (UF, UTM, dólar
/// observado, euro); la tabla de mercado completa el resto de los cruces.
fn merge_rates(
    official: Option<Vec<(String, RateEntry)>>,
    market: Option<Vec<(String, RateEntry)>>,
    today: &str,
) -> Rates {
    let mut rates = Rates::default();
    // CLP es la base de la tabla: 1 peso vale 1 peso.
    rates.entries.insert(
        "CLP".into(),
        RateEntry {
            clp_per_unit: 1.0,
            date: today.to_string(),
            origin: "bcch".into(),
        },
    );
    if let Some(official) = official {
        for (code, entry) in official {
            rates.entries.insert(code, entry);
        }
    }
    if let Some(market) = market {
        // Solo rellena lo que no trajo el Banco Central.
        for (code, entry) in market {
            rates.entries.entry(code).or_insert(entry);
        }
    }
    rates
}

// ---------------------------------------------------------------------------
// Caché en disco y fechas
// ---------------------------------------------------------------------------

fn load_from_disk(path: &Path) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    match serde_json::from_str::<Rates>(&raw) {
        Ok(mut rates) => {
            // El error de red no se hereda entre sesiones: si hay tabla, se usa.
            rates.last_error = false;
            *cache().lock_or_recover() = Some(Arc::new(rates));
        }
        Err(err) => tracing::warn!(%err, "fx: caché de tasas ilegible"),
    }
}

fn save_to_disk(path: &Path, rates: &Rates) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string(rates) {
        Ok(json) => {
            if let Err(err) = std::fs::write(path, json) {
                tracing::warn!(%err, "fx: no se pudo guardar la caché de tasas");
            }
        }
        Err(err) => tracing::warn!(%err, "fx: no se pudo serializar la caché de tasas"),
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn today_iso() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn unix_day(seconds: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|date| date.date_naive().format("%Y-%m-%d").to_string())
}

/// `2026-09-26T03:00:00.000Z` → `2026-09-26`. Si no calza, cadena vacía.
fn iso_day(raw: &str) -> String {
    let day = raw.get(0..10).unwrap_or("");
    let shape_ok = day.len() == 10
        && day.as_bytes()[4] == b'-'
        && day.as_bytes()[7] == b'-'
        && day
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if shape_ok {
        day.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINDICADOR: &str = r#"{
        "uf": {"valor": 41024.46, "fecha": "2026-09-26T03:00:00.000Z"},
        "utm": {"valor": 71721, "fecha": "2026-09-01T04:00:00.000Z"},
        "dolar": {"valor": 965.71, "fecha": "2026-09-25T03:00:00.000Z"},
        "euro": {"valor": 1098.15, "fecha": "2026-09-25T03:00:00.000Z"},
        "bitcoin": {"valor": 86184.81, "fecha": "2026-09-22T03:00:00.000Z"}
    }"#;

    const ERAPI: &str = r#"{
        "result": "success",
        "time_last_update_unix": 1790380952,
        "rates": {
            "USD": 1,
            "CLP": 962.999517,
            "ARS": 1523.9662,
            "EUR": 0.877267,
            "GBP": 0.755341,
            "JPY": 157.539759,
            "XYZ": 5
        }
    }"#;

    fn entry(rates: &Rates, code: &str) -> (f64, String, String) {
        let (value, date, origin) = rates.clp_per_unit(code).expect("moneda en la tabla");
        (value, date.to_string(), origin.to_string())
    }

    #[test]
    fn parsea_los_valores_oficiales() {
        let oficial = parse_mindicador(MINDICADOR).unwrap();
        let rates = merge_rates(Some(oficial), None, "2026-09-26");
        assert_eq!(
            entry(&rates, "UF"),
            (41024.46, "2026-09-26".into(), "bcch".into())
        );
        assert_eq!(
            entry(&rates, "USD"),
            (965.71, "2026-09-25".into(), "bcch".into())
        );
        assert_eq!(
            entry(&rates, "EUR"),
            (1098.15, "2026-09-25".into(), "bcch".into())
        );
        assert_eq!(entry(&rates, "CLP").0, 1.0);
        // El bitcoin de mindicador no entra: la v1 es divisas, no cripto.
        assert!(rates.clp_per_unit("BTC").is_none());
    }

    #[test]
    fn la_tabla_de_mercado_arma_los_cruces() {
        let mercado = parse_erapi(ERAPI).unwrap();
        let rates = merge_rates(None, Some(mercado), "2026-09-26");
        // Sin mindicador, USD y EUR entran como mercado.
        let (usd, _, origin) = entry(&rates, "USD");
        assert!((usd - 962.999517).abs() < f64::EPSILON);
        assert_eq!(origin, "market");
        // Cruces que solo vienen de la tabla: ARS = CLP por USD ÷ ARS por USD.
        let (ars, ..) = entry(&rates, "ARS");
        assert!((ars - 962.999517 / 1523.9662).abs() < 1e-9);
        let (jpy, ..) = entry(&rates, "JPY");
        assert!((jpy - 962.999517 / 157.539759).abs() < 1e-9);
        // Monedas fuera del catálogo se ignoran.
        assert!(rates.clp_per_unit("XYZ").is_none());
    }

    #[test]
    fn el_valor_oficial_gana_sobre_el_de_mercado() {
        let rates = merge_rates(
            Some(parse_mindicador(MINDICADOR).unwrap()),
            Some(parse_erapi(ERAPI).unwrap()),
            "2026-09-26",
        );
        // USD y EUR existen en las dos fuentes: manda el Banco Central.
        assert_eq!(
            entry(&rates, "USD"),
            (965.71, "2026-09-25".into(), "bcch".into())
        );
        assert_eq!(
            entry(&rates, "EUR"),
            (1098.15, "2026-09-25".into(), "bcch".into())
        );
        // Y la UF sigue existiendo porque el mercado no la pisa.
        assert_eq!(entry(&rates, "UF").0, 41024.46);
        // Los cruces sin valor oficial quedan de mercado.
        assert_eq!(entry(&rates, "ARS").2, "market");
    }

    #[test]
    fn la_frescura_de_la_tabla() {
        let mut rates = Rates::default();
        // Nunca descargada: vieja, no «sin conexión» (eso es un intento fallido).
        assert_eq!(rates.freshness(), Freshness::Stale);
        rates.fetched_at = now_unix();
        assert_eq!(rates.freshness(), Freshness::Fresh);
        rates.last_error = true;
        assert_eq!(rates.freshness(), Freshness::Offline);
    }

    #[test]
    fn fechas_de_las_fuentes() {
        assert_eq!(iso_day("2026-09-26T03:00:00.000Z"), "2026-09-26");
        assert_eq!(iso_day("nope"), "");
        assert_eq!(unix_day(1790380952).as_deref(), Some("2026-09-26"));
    }

    #[test]
    fn la_cache_da_la_vuelta_por_disco() {
        let path = std::env::temp_dir().join(format!(
            "atic-fx-test-{}-{}.json",
            std::process::id(),
            now_unix()
        ));
        let rates = merge_rates(
            Some(parse_mindicador(MINDICADOR).unwrap()),
            Some(parse_erapi(ERAPI).unwrap()),
            "2026-09-26",
        );
        let mut to_save = rates.clone();
        to_save.fetched_at = now_unix();
        save_to_disk(&path, &to_save);

        // Otra sesión: sin red, la tabla guardada alcanza para convertir.
        *cache().lock_or_recover() = None;
        load_from_disk(&path);
        let loaded = snapshot().expect("caché cargada");
        assert_eq!(entry(&loaded, "UF").0, 41024.46);
        assert_eq!(loaded.freshness(), Freshness::Fresh);
        let _ = std::fs::remove_file(&path);
    }
}
