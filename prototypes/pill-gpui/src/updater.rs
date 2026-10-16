//! Actualizaciones sin Tauri, compatibles con las que ya publica Atic: el
//! mismo `latest.json` de GitHub Releases, el mismo instalador NSIS y la misma
//! firma minisign (la clave pública de `tauri.conf.json`). Así una
//! instalación vieja y una nueva se actualizan desde el mismo release.
//!
//! Se mira al arrancar y cada 6 horas. En Windows, instalar descarga el
//! `.exe`, comprueba su firma, lo abre en modo pasivo y cierra la pill: el
//! instalador la vuelve a abrir al terminar. En macOS baja el `.app.tar.gz`
//! (el mismo que usaba el updater de Tauri), comprueba su firma, pone la
//! `.app` nueva en el lugar de la que corre y la abre al cerrarse la pill.

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
/// La entrada de `latest.json` de esta plataforma.
const PLATFORM: &str = if cfg!(windows) {
    "windows-x86_64"
} else if cfg!(target_arch = "aarch64") {
    "darwin-aarch64"
} else {
    "darwin-x86_64"
};

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

/// La versión nueva que se puede instalar ahora, si la hay (el aviso del notch).
pub fn available() -> Option<Update> {
    match status() {
        Status::Available(update) => Some(update),
        _ => None,
    }
}

/// Mira al arrancar y cada 6 horas, en un hilo propio. `PILL_UPDATE_DEMO=0.9.0`
/// finge que esa versión está disponible, para ver el aviso del notch (instalarla
/// falla: no hay instalador).
pub fn spawn() {
    if let Ok(version) = std::env::var("PILL_UPDATE_DEMO") {
        set(Status::Available(Update { version, notes: String::new(), url: String::new(), signature: String::new() }));
        return;
    }
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
    if !supported() || matches!(status(), Status::Installing) {
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

/// En Windows siempre; en macOS solo la `.app` publicada (`com.ciat.atic`):
/// la de desarrollo no se reemplaza por la del release.
#[cfg(target_os = "macos")]
pub fn supported() -> bool {
    crate::platform::bundle_id().as_deref() == Some("com.ciat.atic")
}

#[cfg(not(target_os = "macos"))]
pub fn supported() -> bool {
    cfg!(windows)
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

/// `latest.json` → la actualización para esta plataforma, si es más nueva.
fn parse(raw: &str, current: &str) -> Result<Option<Update>, String> {
    parse_for(raw, current, PLATFORM)
}

fn parse_for(raw: &str, current: &str, platform: &str) -> Result<Option<Update>, String> {
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
        .get("platforms")
        .and_then(|platforms| platforms.get(platform))
        .ok_or_else(|| format!("latest.json sin {platform}"))?;
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
        apply(&bytes, &update.version)
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

#[cfg(windows)]
fn apply(bytes: &[u8], version: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("Atic_{version}_x64-setup.exe"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    // `/P`: pasivo, con barra de progreso y sin preguntas.
    std::process::Command::new(&path).arg("/P").spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Desarma el `.app.tar.gz`, cambia la `.app` que corre por la nueva (la vieja
/// queda en la carpeta temporal hasta que macOS la limpie) y deja lista la
/// vuelta. Si algo falla a medio camino, la de antes vuelve a su lugar.
#[cfg(target_os = "macos")]
fn apply(bytes: &[u8], version: &str) -> Result<(), String> {
    use std::process::Command;
    let app = crate::platform::bundle_path().ok_or("la pill no corre desde una .app")?;
    let work = std::env::temp_dir().join(format!("atic-update-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let unpacked = work.join("nueva");
    std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
    let archive = work.join("Atic.app.tar.gz");
    std::fs::write(&archive, bytes).map_err(|e| e.to_string())?;
    let status = Command::new("/usr/bin/tar").arg("-xzf").arg(&archive).arg("-C").arg(&unpacked).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("no se pudo desarmar la actualización".into());
    }
    let fresh = std::fs::read_dir(&unpacked)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or("la actualización no trae una .app")?;
    let previous = work.join("anterior.app");
    std::fs::rename(&app, &previous).map_err(|e| format!("no se pudo mover {}: {e}", app.display()))?;
    if let Err(error) = std::fs::rename(&fresh, &app) {
        let _ = std::fs::rename(&previous, &app);
        return Err(format!("no se pudo poner la versión nueva: {error}"));
    }
    // Para que el Finder y el Dock lean el `Info.plist` nuevo.
    let _ = Command::new("/usr/bin/touch").arg(&app).status();
    crate::platform::reopen_after_exit(&app).map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn apply(_: &[u8], _: &str) -> Result<(), String> {
    Err("sin actualizaciones en esta plataforma".into())
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
        let update = parse_for(raw, "0.4.44", "windows-x86_64").unwrap().unwrap();
        assert_eq!(update.version, "0.4.45");
        assert_eq!(update.url, "https://x/Atic_0.4.45_x64-setup.exe");
        assert!(parse_for(raw, "0.4.45", "windows-x86_64").unwrap().is_none());
        assert!(parse(r#"{"version":"9.0.0","platforms":{}}"#, "0.4.44").is_err());
    }

    #[test]
    fn lee_la_entrada_de_mac() {
        let raw = r#"{"version":"0.5.0","platforms":{
            "darwin-aarch64":{"signature":"c2ln","url":"https://x/Atic.app.tar.gz"},
            "windows-x86_64":{"signature":"c2ln","url":"https://x/Atic_0.5.0_x64-setup.exe"}}}"#;
        let update = parse_for(raw, "0.4.48", "darwin-aarch64").unwrap().unwrap();
        assert_eq!(update.url, "https://x/Atic.app.tar.gz");
        assert!(parse_for(raw, "0.4.48", "darwin-x86_64").is_err());
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
