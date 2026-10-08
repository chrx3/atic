//! Actualizaciones sin Tauri, compatibles con las que ya publica Atic: el
//! mismo `latest.json` de GitHub Releases, el mismo instalador NSIS y la misma
//! firma minisign (la clave pública de `tauri.conf.json`). Así una
//! instalación vieja y una nueva se actualizan desde el mismo release.
//!
//! Se mira al arrancar y cada 6 horas. Instalar descarga el `.exe`, comprueba
//! su firma, lo abre en modo pasivo y cierra la pill: el instalador la vuelve
//! a abrir al terminar.

use std::sync::Mutex;
use std::time::Duration;

use atic_core::MutexExt;
use base64::Engine;

const ENDPOINT: &str = "https://github.com/chrx3/atic/releases/latest/download/latest.json";
/// La clave pública de `plugins.updater.pubkey` en `tauri.conf.json`.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEEwNTM5QkM0MzhGOEJCRTIKUldUaXUvZzR4SnRUb01nS21TNU9Ed004dW5CVlhCQzJBK1NhVXpXNVN2WlVXTjY0LzZQMFJ2bnAK";
/// La versión de la app (la de `tauri.conf.json`, ver `build.rs`).
pub const VERSION: &str = env!("ATIC_VERSION");
const EVERY: Duration = Duration::from_secs(6 * 3600);

/// Una versión publicada más nueva que esta.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    pub notes: String,
    url: String,
    signature: String,
}

/// Lo último que se supo.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Unknown,
    Checking,
    UpToDate,
    Available(Update),
    Installing,
    Failed(String),
}

static STATUS: Mutex<Status> = Mutex::new(Status::Unknown);

pub fn status() -> Status {
    STATUS.lock_or_recover().clone()
}

fn set(status: Status) {
    *STATUS.lock_or_recover() = status;
}

/// Mira al arrancar y cada 6 horas, en un hilo propio.
pub fn spawn() {
    let started = std::thread::Builder::new().name("actualizaciones".into()).spawn(|| loop {
        check_now();
        std::thread::sleep(EVERY);
    });
    if let Err(error) = started {
        tracing::warn!(%error, "no arrancó la revisión de actualizaciones");
    }
}

/// Pregunta ahora (bloquea: llamar fuera del hilo de la UI).
pub fn check_now() {
    if matches!(status(), Status::Installing) {
        return;
    }
    set(Status::Checking);
    let result = fetch().and_then(|raw| parse(&raw, VERSION));
    set(match result {
        Ok(Some(update)) => {
            tracing::info!(version = %update.version, "hay una actualización");
            Status::Available(update)
        }
        Ok(None) => Status::UpToDate,
        Err(error) => {
            tracing::warn!(%error, "no se pudo revisar si hay actualizaciones");
            Status::Failed(error)
        }
    });
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("atic-pill/", env!("ATIC_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

fn fetch() -> Result<String, String> {
    client()?
        .get(ENDPOINT)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| e.to_string())
}

/// `latest.json` → la actualización para Windows x64, si es más nueva.
fn parse(raw: &str, current: &str) -> Result<Option<Update>, String> {
    let json: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or("latest.json sin versión")?
        .trim_start_matches('v')
        .to_string();
    if !newer(&version, current) {
        return Ok(None);
    }
    let platform = json
        .pointer("/platforms/windows-x86_64")
        .ok_or("latest.json sin Windows")?;
    let field = |key: &str| platform.get(key).and_then(|v| v.as_str()).map(str::to_string);
    Ok(Some(Update {
        notes: json.get("notes").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        url: field("url").ok_or("latest.json sin url")?,
        signature: field("signature").ok_or("latest.json sin firma")?,
        version,
    }))
}

/// `a` es más nueva que `b` (x.y.z; lo que no es número cuenta como 0).
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect()
    };
    parts(a) > parts(b)
}

/// ¿`bytes` lleva la firma de Atic? `signature` es el `.sig` en base64, como
/// lo pone `latest.json`.
fn verify(bytes: &[u8], signature: &str) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let decode = |text: &str| -> Result<String, String> {
        let raw = b64.decode(text.trim()).map_err(|e| e.to_string())?;
        String::from_utf8(raw).map_err(|e| e.to_string())
    };
    let key = minisign_verify::PublicKey::decode(&decode(PUBKEY)?).map_err(|e| e.to_string())?;
    let sig = minisign_verify::Signature::decode(&decode(signature)?).map_err(|e| e.to_string())?;
    key.verify(bytes, &sig, true).map_err(|e| format!("firma inválida: {e}"))
}

/// Descarga, comprueba y abre el instalador. Si todo va bien, la pill debe
/// cerrarse después (`true`): el instalador la reemplaza y la vuelve a abrir.
pub fn install(update: &Update) -> bool {
    set(Status::Installing);
    let result = (|| -> Result<(), String> {
        let bytes = client()?
            .get(&update.url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
            .map_err(|e| e.to_string())?;
        verify(&bytes, &update.signature)?;
        let path = std::env::temp_dir().join(format!("Atic_{}_x64-setup.exe", update.version));
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        // `/P`: pasivo, con barra de progreso y sin preguntas.
        std::process::Command::new(&path).arg("/P").spawn().map_err(|e| e.to_string())?;
        Ok(())
    })();
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "no se pudo instalar la actualización");
            set(Status::Failed(error));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versiones() {
        assert!(newer("0.4.45", "0.4.44"));
        assert!(newer("0.5.0", "0.4.99"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("0.4.44", "0.4.44"));
        assert!(!newer("0.4.9", "0.4.44"));
    }

    #[test]
    fn lee_latest_json_como_lo_publica_atic() {
        let raw = r#"{"version":"0.4.45","notes":"Arreglos","pub_date":"2026-10-08T00:00:00Z",
            "platforms":{"windows-x86_64":{"signature":"c2ln","url":"https://x/Atic_0.4.45_x64-setup.exe"}}}"#;
        let update = parse(raw, "0.4.44").unwrap().unwrap();
        assert_eq!(update.version, "0.4.45");
        assert_eq!(update.url, "https://x/Atic_0.4.45_x64-setup.exe");
        assert!(parse(raw, "0.4.45").unwrap().is_none());
        assert!(parse(r#"{"version":"9.0.0","platforms":{}}"#, "0.4.44").is_err());
    }

    /// Contra el release publicado: baja el instalador y comprueba su firma.
    /// `cargo test updater -- --ignored`.
    #[test]
    #[ignore = "baja el instalador publicado (~25 MB)"]
    fn la_firma_del_release_publicado_es_valida() {
        let raw = fetch().unwrap();
        let update = parse(&raw, "0.0.0").unwrap().unwrap();
        let bytes = client().unwrap().get(&update.url).send().unwrap().bytes().unwrap();
        verify(&bytes, &update.signature).unwrap();
        assert!(verify(&bytes[1..], &update.signature).is_err());
    }

    #[test]
    fn la_clave_publica_de_atic_se_lee() {
        let raw = base64::engine::general_purpose::STANDARD.decode(PUBKEY).unwrap();
        assert!(minisign_verify::PublicKey::decode(&String::from_utf8(raw).unwrap()).is_ok());
        assert!(verify(b"hola", "bm8gZXMgdW5hIGZpcm1h").is_err());
    }
}
