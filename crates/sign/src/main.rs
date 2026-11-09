//! Firma un archivo para el updater de Atic, sin el CLI de Tauri.
//!
//! El formato es el mismo que dejaba `tauri signer sign`, que es el que
//! comprueba la app (`updater.rs`, con `minisign-verify`):
//! - La clave privada (`~/.tauri/atic-updater.key`) es el texto de una clave
//!   secreta de minisign, cifrada con contraseña y en base64.
//! - El `.sig` es el texto de la firma de minisign, en base64.
//!
//! El comentario de confianza de la firma (lo cubre la firma global de
//! minisign, así que no se puede cambiar sin la clave) dice qué se firmó:
//! `timestamp:…<TAB>file:…<TAB>version:…<TAB>target:…`. La app exige que
//! `version` sea la que anuncia `latest.json` y más nueva que la instalada:
//! `latest.json` no va firmado, y sin esto se podría servir un instalador
//! viejo, bien firmado, con la etiqueta de uno nuevo.
//!
//! Uso: `atic-sign <archivo> [--version <x.y.z>] [--target <plataforma>]
//! [--key <ruta>] [--password-file <ruta>]`.
//! - `--version`: por omisión, la de `apps/atic/Cargo.toml` de este repo.
//! - `--target`: por omisión sale del nombre: `…-setup.exe` es
//!   `windows-x86_64` y `….app.tar.gz` es `darwin` (el build universal, que
//!   sirve a `darwin-aarch64` y `darwin-x86_64`).
//! - Por omisión lee `~/.tauri/atic-updater.key` y
//!   `~/.tauri/atic-updater.password` (en Windows, bajo `%USERPROFILE%`).
//!
//! Deja `<archivo>.sig` al lado.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;

fn main() {
    if let Err(error) = run() {
        eprintln!("atic-sign: {error}");
        std::process::exit(1);
    }
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "no encontré la carpeta del usuario".to_string())
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    let mut key_path = None;
    let mut password_path = None;
    let mut version = None;
    let mut target = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--key" => key_path = args.next().map(PathBuf::from),
            "--password-file" => password_path = args.next().map(PathBuf::from),
            "--version" => version = Some(args.next().ok_or("--version sin valor")?),
            "--target" => target = Some(args.next().ok_or("--target sin valor")?),
            _ => file = Some(PathBuf::from(arg)),
        }
    }
    let file = file.ok_or("falta el archivo a firmar: atic-sign <archivo>")?;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let version = match version {
        Some(version) => version,
        None => app_version()?,
    };
    let target = match target {
        Some(target) => target,
        None => target_of(&name)?.to_string(),
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let trusted = trusted_comment(&name, &version, &target, stamp)?;

    let keys = home()?.join(".tauri");
    let key_path = key_path.unwrap_or_else(|| keys.join("atic-updater.key"));
    let password_path = password_path.unwrap_or_else(|| keys.join("atic-updater.password"));
    let key = std::fs::read_to_string(&key_path).map_err(|e| format!("{}: {e}", key_path.display()))?;
    let password = std::fs::read_to_string(&password_path).map_err(|e| format!("{}: {e}", password_path.display()))?;
    let secret = open_key(&key, password.trim())?;
    let data = std::fs::File::open(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let signature = sign(&secret, data, &trusted)?;
    let out = PathBuf::from(format!("{}.sig", file.display()));
    std::fs::write(&out, signature).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{}", out.display());
    Ok(())
}

/// La versión de la app: la primera `version = "…"` de `apps/atic/Cargo.toml`,
/// como la leen los scripts de release. La ruta es la del repo donde se
/// compiló el firmador, que es desde donde lo corren los scripts (`cargo run`).
fn app_version() -> Result<String, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/atic/Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("no pude leer la versión de {} ({e}); pásala con --version", manifest.display()))?;
    version_in_manifest(&text)
        .map(str::to_string)
        .ok_or_else(|| format!("{} no dice la versión; pásala con --version", manifest.display()))
}

fn version_in_manifest(text: &str) -> Option<&str> {
    text.lines().find_map(|line| line.trim_end().strip_prefix("version = \"")?.strip_suffix('"'))
}

/// La plataforma según el nombre del artefacto, con los nombres de
/// `latest.json`. El `.app.tar.gz` es universal: `darwin` a secas vale para
/// las dos arquitecturas de Mac.
fn target_of(name: &str) -> Result<&'static str, String> {
    if name.ends_with("-setup.exe") {
        Ok("windows-x86_64")
    } else if name.ends_with(".app.tar.gz") {
        Ok("darwin")
    } else {
        Err(format!("no sé para qué plataforma es {name}; pásala con --target"))
    }
}

/// El comentario de confianza: `clave:valor` separados por tabulador. Las dos
/// primeras son las de siempre; `version` y `target` son las que lee la app.
fn trusted_comment(name: &str, version: &str, target: &str, stamp: u64) -> Result<String, String> {
    let version = version.trim();
    let version = version.strip_prefix('v').unwrap_or(version);
    let plain = |text: &str, extra: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c));
    if !plain(version, ".-+") || !version.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!("la versión «{version}» no es x.y.z"));
    }
    if !plain(target, "-_") {
        return Err(format!("la plataforma «{target}» no es válida"));
    }
    // Un tabulador o un salto de línea en el nombre metería otra clave (o
    // rompería el `.sig`, que es de una línea por campo).
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(format!("no puedo firmar un archivo con ese nombre: {name:?}"));
    }
    // El instalador de Windows lleva la versión en el nombre: que no se firme
    // el de otra versión con la etiqueta de esta.
    if let Some(in_name) = name.strip_prefix("Atic_").and_then(|rest| rest.split('_').next()) {
        if in_name != version {
            return Err(format!("{name} es de la versión {in_name}, pero se pidió firmar la {version}"));
        }
    }
    Ok(format!("timestamp:{stamp}\tfile:{name}\tversion:{version}\ttarget:{target}"))
}

/// La clave secreta: `key_b64` es el contenido de `atic-updater.key`.
fn open_key(key_b64: &str, password: &str) -> Result<minisign::SecretKey, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let key_text = b64.decode(key_b64.trim()).map_err(|e| format!("la clave no es base64: {e}"))?;
    let key_text = String::from_utf8(key_text).map_err(|e| e.to_string())?;
    minisign::SecretKeyBox::from_string(&key_text)
        .and_then(|boxed| boxed.into_secret_key(Some(password.to_string())))
        .map_err(|e| format!("no pude abrir la clave (¿contraseña?): {e}"))
}

/// La firma de `data` en base64, como la escribe `tauri signer sign`, con
/// `trusted` de comentario de confianza.
fn sign(secret: &minisign::SecretKey, data: impl Read, trusted: &str) -> Result<String, String> {
    let signature = minisign::sign(None, secret, data, Some(trusted), Some("signature from tauri secret key"))
        .map_err(|e| format!("no pude firmar: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.into_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La clave pública de la app (`updater.rs`, `PUBKEY`).
    const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEEwNTM5QkM0MzhGOEJCRTIKUldUaXUvZzR4SnRUb01nS21TNU9Ed004dW5CVlhCQzJBK1NhVXpXNVN2WlVXTjY0LzZQMFJ2bnAK";

    const DATA: &[u8] = b"una actualizacion de prueba";

    fn text(b64: &str) -> String {
        String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b64.trim()).unwrap()).unwrap()
    }

    /// Una firma hecha con una clave de prueba, y esa clave como la lee la app.
    fn signed(trusted: &str) -> (minisign_verify::PublicKey, String) {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public = minisign_verify::PublicKey::from_base64(&pair.pk.to_base64()).unwrap();
        (public, text(&sign(&pair.sk, DATA, trusted).unwrap()))
    }

    /// El mismo texto que comprueba `updater.rs` (`lee_lo_que_firma_atic_sign`):
    /// si cambia el formato acá, cambia allá.
    #[test]
    fn el_comentario_dice_version_y_plataforma() {
        assert_eq!(
            trusted_comment("Atic_0.6.1_x64-setup.exe", "0.6.1", "windows-x86_64", 1).unwrap(),
            "timestamp:1\tfile:Atic_0.6.1_x64-setup.exe\tversion:0.6.1\ttarget:windows-x86_64"
        );
        assert_eq!(
            trusted_comment("Atic.app.tar.gz", "v0.6.1", "darwin", 2).unwrap(),
            "timestamp:2\tfile:Atic.app.tar.gz\tversion:0.6.1\ttarget:darwin"
        );
    }

    #[test]
    fn no_firma_etiquetas_que_no_calzan() {
        // El instalador de otra versión.
        assert!(trusted_comment("Atic_0.6.0_x64-setup.exe", "0.6.1", "windows-x86_64", 1).is_err());
        // Nada que pueda meter otra clave en el comentario.
        assert!(trusted_comment("Atic.app.tar.gz", "0.6.1\tversion:9.9.9", "darwin", 1).is_err());
        assert!(trusted_comment("Atic.app.tar.gz", "0.6.1", "darwin\ttarget:x", 1).is_err());
        assert!(trusted_comment("a\tversion:9.9.9", "0.6.1", "darwin", 1).is_err());
        assert!(trusted_comment("Atic.app.tar.gz", "", "darwin", 1).is_err());
        assert!(trusted_comment("Atic.app.tar.gz", "0.6.1", "", 1).is_err());
    }

    #[test]
    fn la_plataforma_sale_del_nombre() {
        assert_eq!(target_of("Atic_0.6.1_x64-setup.exe").unwrap(), "windows-x86_64");
        assert_eq!(target_of("Atic.app.tar.gz").unwrap(), "darwin");
        assert!(target_of("Atic_0.6.1_universal.dmg").is_err());
    }

    #[test]
    fn lee_la_version_del_cargo_toml_de_la_app() {
        let manifest = "[package]\nname = \"atic\"\nversion = \"0.6.1\"\r\n\n[dependencies]\nfoo = { version = \"1\" }\nversion = \"9\"\n";
        assert_eq!(version_in_manifest(manifest), Some("0.6.1"));
        assert_eq!(version_in_manifest("[package]\nversion.workspace = true\n"), None);
        // La de verdad: es lo que firma el release si no se pasa `--version`.
        let version = app_version().unwrap();
        assert!(trusted_comment("Atic.app.tar.gz", &version, "darwin", 1).is_ok(), "{version}");
    }

    /// Lo que hace la app: la firma vale para esos bytes y el comentario de
    /// confianza se lee tal cual se firmó.
    #[test]
    fn la_app_lee_el_comentario_firmado() {
        let trusted = trusted_comment("Atic_0.6.1_x64-setup.exe", "0.6.1", "windows-x86_64", 1).unwrap();
        let (public, signature) = signed(&trusted);
        let sig = minisign_verify::Signature::decode(&signature).unwrap();
        public.verify(DATA, &sig, true).expect("la app aceptaría la firma");
        assert_eq!(sig.trusted_comment(), trusted);
        assert!(public.verify(b"otra cosa", &sig, true).is_err());
    }

    /// La firma global cubre el comentario: cambiarle la versión a una firma
    /// válida la invalida, aunque los bytes firmados sean los mismos.
    #[test]
    fn cambiar_la_version_del_comentario_invalida_la_firma() {
        let trusted = trusted_comment("Atic.app.tar.gz", "0.6.1", "darwin", 1).unwrap();
        let (public, signature) = signed(&trusted);
        let forged = signature.replace("version:0.6.1", "version:9.9.9");
        assert_ne!(forged, signature);
        let sig = minisign_verify::Signature::decode(&forged).unwrap();
        assert_eq!(sig.trusted_comment(), trusted.replace("0.6.1", "9.9.9"));
        assert!(public.verify(DATA, &sig, true).is_err());
    }

    /// Firma con la clave de verdad y comprueba como lo hace la app. Necesita
    /// `~/.tauri/atic-updater.key` y su contraseña: `cargo test -p atic-sign -- --ignored`.
    #[test]
    #[ignore]
    fn la_app_acepta_lo_que_firma() {
        let keys = home().unwrap().join(".tauri");
        let key = std::fs::read_to_string(keys.join("atic-updater.key")).unwrap();
        let password = std::fs::read_to_string(keys.join("atic-updater.password")).unwrap();
        let secret = open_key(&key, password.trim()).unwrap();
        let trusted = trusted_comment("Atic.app.tar.gz", "0.6.1", "darwin", 1).unwrap();
        let signature = sign(&secret, DATA, &trusted).unwrap();

        let public = minisign_verify::PublicKey::decode(&text(PUBKEY)).unwrap();
        let sig = minisign_verify::Signature::decode(&text(&signature)).unwrap();
        public.verify(DATA, &sig, true).expect("la app aceptaría la firma");
        assert_eq!(sig.trusted_comment(), trusted);
        assert!(public.verify(b"otra cosa", &sig, true).is_err());
    }
}
