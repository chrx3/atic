//! Actualizaciones sin Tauri, compatibles con las que ya publica Atic: el
//! mismo `latest.json` de GitHub Releases, el mismo instalador NSIS y la misma
//! firma minisign (la clave pública de abajo; firma `crates/sign`). Así una
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
/// La clave pública de las actualizaciones (la privada firma con `atic-sign`).
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEEwNTM5QkM0MzhGOEJCRTIKUldUaXUvZzR4SnRUb01nS21TNU9Ed004dW5CVlhCQzJBK1NhVXpXNVN2WlVXTjY0LzZQMFJ2bnAK";
/// La versión de la app (la del `Cargo.toml`, ver `build.rs`).
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

/// Si hay una versión nueva, sin copiarla: la cara lo mira en cada sondeo.
pub fn has_update() -> bool {
    matches!(*STATUS.lock_or_recover(), Status::Available(_))
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

/// En el cliente bloqueante de reqwest, `timeout` limita la espera de la
/// respuesta (hasta los encabezados), cada lectura del cuerpo por separado y
/// los atajos que leen el cuerpo entero (`text`, `bytes`). Por eso el
/// instalador se baja leyendo de a trozos (`download`): así es un límite de
/// silencio y no un plazo para toda la descarga.
fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("atic-pill/", env!("ATIC_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

/// Lo más que puede pesar una actualización: lo publicado ronda los 100 MB.
const MAX_DOWNLOAD: u64 = 1 << 30;

/// Baja `url` entero. Sin plazo total: en una conexión lenta tarda lo que
/// tarde, y se corta solo si pasan 60 s sin que llegue nada (ver `client`).
fn download(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let response = client()?.get(url).send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response.take(MAX_DOWNLOAD + 1).read_to_end(&mut bytes).map_err(|e| format!("se cortó la descarga: {e}"))?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err("la actualización pesa demasiado".into());
    }
    Ok(bytes)
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
/// lo pone `latest.json`. Devuelve el comentario de confianza de la firma:
/// `minisign-verify` comprueba también la firma global, que lo cubre, así que
/// lo que dice lo escribió quien tiene la clave.
fn verify(bytes: &[u8], signature: &str) -> Result<String, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let decode = |text: &str| -> Result<String, String> {
        let raw = b64.decode(text.trim()).map_err(|e| e.to_string())?;
        String::from_utf8(raw).map_err(|e| e.to_string())
    };
    let key = minisign_verify::PublicKey::decode(&decode(PUBKEY)?).map_err(|e| e.to_string())?;
    let sig = minisign_verify::Signature::decode(&decode(signature)?).map_err(|e| e.to_string())?;
    key.verify(bytes, &sig, true).map_err(|e| format!("firma inválida: {e}"))?;
    Ok(sig.trusted_comment().to_string())
}

/// Lo firmado es la versión `expected` (la que anunció `latest.json`), más
/// nueva que `current` y para `platform`. `latest.json` no va firmado: sin
/// esto, quien pueda editar el release podría servir un instalador viejo, bien
/// firmado, con la etiqueta de uno nuevo.
///
/// `comment` es el comentario de confianza que escribe `atic-sign`:
/// `clave:valor` separados por tabulador, con `version` y `target`. Las firmas
/// de la 0.6.0 y anteriores no los traen y se rechazan: ninguna es más nueva
/// que una app que ya trae este control.
fn check_signed(comment: &str, expected: &str, current: &str, platform: &str) -> Result<(), String> {
    let field = |key: &str| -> Result<&str, String> {
        let mut values = comment.split('\t').filter_map(|part| part.split_once(':')).filter(|(k, _)| *k == key).map(|(_, value)| value);
        match (values.next(), values.next()) {
            (Some(value), None) => Ok(value),
            (None, _) => Err(format!("la firma no dice {key}")),
            _ => Err(format!("la firma repite {key}")),
        }
    };
    let version = field("version")?;
    if version != expected.trim_start_matches('v') {
        return Err(format!("la firma es de la versión {version}, no de la {expected}"));
    }
    if !newer(version, current) {
        return Err(format!("la versión firmada ({version}) no es más nueva que esta ({current})"));
    }
    // `darwin` a secas es el build universal: vale para las dos arquitecturas.
    let target = field("target")?;
    let fits = platform == target || platform.strip_prefix(target).is_some_and(|rest| rest.starts_with('-'));
    if target.is_empty() || !fits {
        return Err(format!("la firma es para {target}, no para {platform}"));
    }
    Ok(())
}

/// Descarga, comprueba y abre el instalador. Si todo va bien, la pill debe
/// cerrarse después (`true`): el instalador la reemplaza y la vuelve a abrir.
pub fn install(update: &Update) -> bool {
    set(Status::Installing);
    let result = (|| -> Result<(), String> {
        let bytes = download(&update.url)?;
        let signed = verify(&bytes, &update.signature)?;
        check_signed(&signed, &update.version, VERSION, PLATFORM)?;
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

/// Una carpeta nueva para el instalador, con nombre al azar. `create_dir`
/// falla si ya existe: nadie puede haberla dejado preparada. De paso borra
/// las de actualizaciones anteriores.
#[cfg(windows)]
fn fresh_dir() -> Result<std::path::PathBuf, String> {
    use std::hash::{BuildHasher, Hasher};
    const PREFIX: &str = "atic-update-";
    let temp = std::env::temp_dir();
    for entry in std::fs::read_dir(&temp).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with(PREFIX) && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    for _ in 0..8 {
        // `RandomState` trae una semilla al azar del sistema.
        let token = std::collections::hash_map::RandomState::new().build_hasher().finish();
        let dir = temp.join(format!("{PREFIX}{token:016x}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("no se pudo crear {}: {error}", dir.display())),
        }
    }
    Err("no se pudo crear la carpeta de la actualización".into())
}

/// Deja el instalador en una carpeta recién creada y lo abre. Lo que se
/// ejecuta son los bytes ya comprobados: el archivo se crea de cero (falla si
/// existe), y después de cerrarlo se vuelve a abrir sin permitir que otro
/// proceso lo escriba, borre o renombre; con eso tomado se compara con lo
/// comprobado y recién ahí se ejecuta.
#[cfg(windows)]
fn apply(bytes: &[u8], version: &str) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    // La versión va en el nombre del archivo: que no pueda salir de la carpeta.
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)) {
        return Err(format!("versión inválida: {version}"));
    }
    let path = fresh_dir()?.join(format!("Atic_{version}_x64-setup.exe"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(&path)
        .map_err(|e| format!("no se pudo crear {}: {e}", path.display()))?;
    file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|e| e.to_string())?;
    // Con el archivo abierto para escribir Windows no deja ejecutarlo.
    drop(file);
    // Abrirlo así falla si otro proceso lo tiene abierto para escribir, y
    // mientras siga abierto nadie más puede abrirlo para eso.
    let mut written = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .map_err(|e| format!("no se pudo volver a abrir {}: {e}", path.display()))?;
    let mut on_disk = Vec::with_capacity(bytes.len());
    written.read_to_end(&mut on_disk).map_err(|e| e.to_string())?;
    if on_disk != bytes {
        return Err("el instalador cambió después de comprobarlo".into());
    }
    // `/P`: pasivo, con barra de progreso y sin preguntas.
    std::process::Command::new(&path).arg("/P").spawn().map_err(|e| e.to_string())?;
    // Queda tomado hasta que la pill se cierre: el instalador sigue leyéndose
    // a sí mismo mientras instala.
    std::mem::forget(written);
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
        let bytes = download(&update.url).unwrap();
        verify(&bytes, &update.signature).unwrap();
        assert!(verify(&bytes[1..], &update.signature).is_err());
    }

    /// El comentario es el que escribe `atic-sign` (su test
    /// `el_comentario_dice_version_y_plataforma`): si cambia allá, cambia acá.
    #[test]
    fn lee_lo_que_firma_atic_sign() {
        let windows = "timestamp:1\tfile:Atic_0.6.1_x64-setup.exe\tversion:0.6.1\ttarget:windows-x86_64";
        assert!(check_signed(windows, "0.6.1", "0.6.0", "windows-x86_64").is_ok());
        assert!(check_signed(windows, "v0.6.1", "0.6.0", "windows-x86_64").is_ok());
        // El build universal de Mac sirve a las dos arquitecturas.
        let mac = "timestamp:2\tfile:Atic.app.tar.gz\tversion:0.6.1\ttarget:darwin";
        assert!(check_signed(mac, "0.6.1", "0.6.0", "darwin-aarch64").is_ok());
        assert!(check_signed(mac, "0.6.1", "0.6.0", "darwin-x86_64").is_ok());
        assert!(check_signed(mac, "0.6.1", "0.6.0", "windows-x86_64").is_err());
        assert!(check_signed(windows, "0.6.1", "0.6.0", "darwin-aarch64").is_err());
    }

    #[test]
    fn no_instala_una_version_vieja_con_etiqueta_de_nueva() {
        // `latest.json` dice 0.9.0, pero lo firmado es la 0.5.0.
        let old = "timestamp:1\tfile:Atic_0.5.0_x64-setup.exe\tversion:0.5.0\ttarget:windows-x86_64";
        assert!(check_signed(old, "0.9.0", "0.6.0", "windows-x86_64").is_err());
        // Aunque `latest.json` diga la verdad: no se baja de versión ni se reinstala la misma.
        assert!(check_signed(old, "0.5.0", "0.6.0", "windows-x86_64").is_err());
        let same = "timestamp:1\tfile:Atic_0.6.0_x64-setup.exe\tversion:0.6.0\ttarget:windows-x86_64";
        assert!(check_signed(same, "0.6.0", "0.6.0", "windows-x86_64").is_err());
        // Las firmas de antes (sin versión) no dicen qué son.
        assert!(check_signed("timestamp:1\tfile:Atic_0.9.0_x64-setup.exe", "0.9.0", "0.6.0", "windows-x86_64").is_err());
        assert!(check_signed("timestamp:1\tfile:Atic.app.tar.gz", "0.9.0", "0.6.0", "darwin-aarch64").is_err());
        // Ni versión repetida, ni plataforma a medias.
        let twice = "timestamp:1\tfile:a\tversion:0.9.0\tversion:0.5.0\ttarget:windows-x86_64";
        assert!(check_signed(twice, "0.9.0", "0.6.0", "windows-x86_64").is_err());
        let partial = "timestamp:1\tfile:a\tversion:0.9.0\ttarget:win";
        assert!(check_signed(partial, "0.9.0", "0.6.0", "windows-x86_64").is_err());
        let empty = "timestamp:1\tfile:a\tversion:0.9.0\ttarget:";
        assert!(check_signed(empty, "0.9.0", "0.6.0", "-x").is_err());
    }

    #[test]
    fn la_clave_publica_de_atic_se_lee() {
        let raw = base64::engine::general_purpose::STANDARD.decode(PUBKEY).unwrap();
        assert!(minisign_verify::PublicKey::decode(&String::from_utf8(raw).unwrap()).is_ok());
        assert!(verify(b"hola", "bm8gZXMgdW5hIGZpcm1h").is_err());
    }
}
