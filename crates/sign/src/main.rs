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
//!
//! Otros dos modos, para el release de GitHub Actions:
//! - `atic-sign --verify <archivo> [--pubkey <ruta>] [--version …] [--target …]`
//!   comprueba `<archivo>.sig` como la app (`minisign-verify`): la firma, y que
//!   el comentario de confianza diga ese archivo, esa versión y esa
//!   plataforma. Por omisión usa la clave pública de la app (`PUBKEY` de
//!   `apps/atic/src/updater.rs`, leída al compilar); `--pubkey` es un archivo
//!   con otra, en el mismo formato (base64).
//! - `atic-sign --nueva-clave <ruta> --password-file <ruta>` crea un par de
//!   claves de prueba con esa contraseña: `<ruta>` (privada, como
//!   `atic-updater.key`) y `<ruta>.pub` (pública, como `PUBKEY`). Las corridas
//!   de prueba firman con una así, no con la de verdad.

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

#[derive(PartialEq)]
enum Mode {
    Sign,
    Verify,
    NewKey,
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut mode = Mode::Sign;
    let mut file = None;
    let mut key_path = None;
    let mut password_path = None;
    let mut pubkey_path = None;
    let mut version = None;
    let mut target = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verify" => mode = Mode::Verify,
            "--nueva-clave" => mode = Mode::NewKey,
            "--key" => key_path = args.next().map(PathBuf::from),
            "--password-file" => password_path = args.next().map(PathBuf::from),
            "--pubkey" => pubkey_path = Some(PathBuf::from(args.next().ok_or("--pubkey sin valor")?)),
            "--version" => version = Some(args.next().ok_or("--version sin valor")?),
            "--target" => target = Some(args.next().ok_or("--target sin valor")?),
            _ => file = Some(PathBuf::from(arg)),
        }
    }
    let file = file.ok_or("falta el archivo: atic-sign <archivo>")?;
    if mode == Mode::NewKey {
        let password_path = password_path.ok_or("--nueva-clave necesita --password-file")?;
        let password = std::fs::read_to_string(&password_path).map_err(|e| format!("{}: {e}", password_path.display()))?;
        let (key, public) = new_key(password.trim())?;
        let public_path = PathBuf::from(format!("{}.pub", file.display()));
        std::fs::write(&file, key).map_err(|e| format!("{}: {e}", file.display()))?;
        std::fs::write(&public_path, public).map_err(|e| format!("{}: {e}", public_path.display()))?;
        println!("{}", public_path.display());
        return Ok(());
    }
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let version = match version {
        Some(version) => version,
        None => app_version()?,
    };
    let target = match target {
        Some(target) => target,
        None => target_of(&name)?.to_string(),
    };
    if mode == Mode::Verify {
        let public = match pubkey_path {
            Some(path) => std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            None => app_pubkey()?.to_string(),
        };
        let sig_path = PathBuf::from(format!("{}.sig", file.display()));
        let signature = std::fs::read_to_string(&sig_path).map_err(|e| format!("{}: {e}", sig_path.display()))?;
        let data = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let trusted = verify(&public, &data, &signature)?;
        check_comment(&trusted, &name, &version, &target)?;
        println!("{}: firma válida ({})", sig_path.display(), trusted.replace('\t', " "));
        return Ok(());
    }
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

/// Un par de claves nuevo, en el formato de `atic-updater.key` (cifrada con
/// `password`) y de `PUBKEY`: el texto de minisign, en base64.
fn new_key(password: &str) -> Result<(String, String), String> {
    if password.is_empty() {
        return Err("la contraseña está vacía".into());
    }
    let pair = minisign::KeyPair::generate_encrypted_keypair(Some(password.to_string())).map_err(|e| e.to_string())?;
    let secret = pair.sk.to_box(Some("clave de prueba de atic-sign")).map_err(|e| e.to_string())?.into_string();
    let public = pair.pk.to_box().map_err(|e| e.to_string())?.into_string();
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok((b64.encode(secret), b64.encode(public)))
}

/// La clave pública con que la app comprueba las actualizaciones: la de
/// `updater.rs`, para que no haya dos copias que puedan diferir.
fn app_pubkey() -> Result<&'static str, String> {
    const UPDATER: &str = include_str!("../../../apps/atic/src/updater.rs");
    UPDATER
        .lines()
        .find_map(|line| line.trim().strip_prefix("const PUBKEY: &str = \"")?.strip_suffix("\";"))
        .ok_or_else(|| "no encontré PUBKEY en apps/atic/src/updater.rs".to_string())
}

/// Lo que hace la app (`updater.rs`, `verify`): `public` y `signature` en
/// base64. Devuelve el comentario de confianza, que la firma global cubre.
fn verify(public: &str, data: &[u8], signature: &str) -> Result<String, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let decode = |text: &str| -> Result<String, String> {
        let raw = b64.decode(text.trim()).map_err(|e| e.to_string())?;
        String::from_utf8(raw).map_err(|e| e.to_string())
    };
    let key = minisign_verify::PublicKey::decode(&decode(public)?).map_err(|e| format!("la clave pública no se lee: {e}"))?;
    let sig = minisign_verify::Signature::decode(&decode(signature)?).map_err(|e| format!("el .sig no se lee: {e}"))?;
    key.verify(data, &sig, true).map_err(|e| format!("firma inválida para esta clave: {e}"))?;
    Ok(sig.trusted_comment().to_string())
}

/// El comentario firmado dice este archivo, esta versión y esta plataforma.
fn check_comment(trusted: &str, name: &str, version: &str, target: &str) -> Result<(), String> {
    let version = version.trim();
    let version = version.strip_prefix('v').unwrap_or(version);
    for (key, expected) in [("file", name), ("version", version), ("target", target)] {
        let mut values = trusted.split('\t').filter_map(|part| part.split_once(':')).filter(|(k, _)| *k == key).map(|(_, v)| v);
        match (values.next(), values.next()) {
            (Some(value), None) if value == expected => {}
            (Some(value), None) => return Err(format!("la firma dice {key}:{value}, no {expected}")),
            (None, _) => return Err(format!("la firma no dice {key}")),
            _ => return Err(format!("la firma repite {key}")),
        }
    }
    Ok(())
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

    /// `--verify` sin `--pubkey` usa la clave de la app, no una copia.
    #[test]
    fn la_clave_publica_es_la_de_la_app() {
        assert_eq!(app_pubkey().unwrap(), PUBKEY);
    }

    /// Lo que hace el release: una clave de prueba nueva, firmar con ella como
    /// con la de verdad y comprobar con `--verify`.
    #[test]
    fn verifica_lo_que_firma_con_una_clave_nueva() {
        let (key, public) = new_key("prueba").unwrap();
        let secret = open_key(&key, "prueba").unwrap();
        let trusted = trusted_comment("Atic_0.6.1_x64-setup.exe", "0.6.1", "windows-x86_64", 1).unwrap();
        let signature = sign(&secret, DATA, &trusted).unwrap();

        let read = verify(&public, DATA, &signature).unwrap();
        assert_eq!(read, trusted);
        check_comment(&read, "Atic_0.6.1_x64-setup.exe", "v0.6.1", "windows-x86_64").unwrap();
        // Otros bytes, otra clave u otra etiqueta no pasan.
        assert!(verify(&public, b"otra cosa", &signature).is_err());
        assert!(verify(PUBKEY, DATA, &signature).is_err());
        assert!(check_comment(&read, "Atic_0.6.1_x64-setup.exe", "0.6.2", "windows-x86_64").is_err());
        assert!(check_comment(&read, "Atic_0.6.1_x64-setup.exe", "0.6.1", "darwin").is_err());
        assert!(check_comment(&read, "Atic.app.tar.gz", "0.6.1", "windows-x86_64").is_err());
        assert!(check_comment("timestamp:1\tversion:0.6.1\ttarget:darwin", "Atic.app.tar.gz", "0.6.1", "darwin").is_err());
    }

    #[test]
    fn no_crea_claves_sin_contrasena() {
        assert!(new_key("").is_err());
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
