//! Firma un archivo para el updater de Atic, sin el CLI de Tauri.
//!
//! El formato es el mismo que dejaba `tauri signer sign`, que es el que
//! comprueba la app (`updater.rs`, con `minisign-verify`):
//! - La clave privada (`~/.tauri/atic-updater.key`) es el texto de una clave
//!   secreta de minisign, cifrada con contraseña y en base64.
//! - El `.sig` es el texto de la firma de minisign, en base64.
//!
//! Uso: `atic-sign <archivo> [--key <ruta>] [--password-file <ruta>]`.
//! Por omisión lee `~/.tauri/atic-updater.key` y `~/.tauri/atic-updater.password`
//! (en Windows, bajo `%USERPROFILE%`). Deja `<archivo>.sig` al lado.

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
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--key" => key_path = args.next().map(PathBuf::from),
            "--password-file" => password_path = args.next().map(PathBuf::from),
            _ => file = Some(PathBuf::from(arg)),
        }
    }
    let file = file.ok_or("falta el archivo a firmar: atic-sign <archivo>")?;
    let keys = home()?.join(".tauri");
    let key_path = key_path.unwrap_or_else(|| keys.join("atic-updater.key"));
    let password_path = password_path.unwrap_or_else(|| keys.join("atic-updater.password"));
    let key = std::fs::read_to_string(&key_path).map_err(|e| format!("{}: {e}", key_path.display()))?;
    let password = std::fs::read_to_string(&password_path).map_err(|e| format!("{}: {e}", password_path.display()))?;
    let signature = sign(&file, &key, password.trim())?;
    let out = PathBuf::from(format!("{}.sig", file.display()));
    std::fs::write(&out, signature).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("{}", out.display());
    Ok(())
}

/// La firma de `file` en base64, como la escribe `tauri signer sign`.
pub fn sign(file: &Path, key_b64: &str, password: &str) -> Result<String, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let key_text = b64.decode(key_b64.trim()).map_err(|e| format!("la clave no es base64: {e}"))?;
    let key_text = String::from_utf8(key_text).map_err(|e| e.to_string())?;
    let secret = minisign::SecretKeyBox::from_string(&key_text)
        .and_then(|boxed| boxed.into_secret_key(Some(password.to_string())))
        .map_err(|e| format!("no pude abrir la clave (¿contraseña?): {e}"))?;
    let data = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let trusted = format!("timestamp:{stamp}\tfile:{name}");
    let signature = minisign::sign(None, &secret, data, Some(&trusted), Some("signature from tauri secret key"))
        .map_err(|e| format!("no pude firmar: {e}"))?;
    Ok(b64.encode(signature.into_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La clave pública de la app (`updater.rs`, `PUBKEY`).
    const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEEwNTM5QkM0MzhGOEJCRTIKUldUaXUvZzR4SnRUb01nS21TNU9Ed004dW5CVlhCQzJBK1NhVXpXNVN2WlVXTjY0LzZQMFJ2bnAK";

    /// Firma con la clave de verdad y comprueba como lo hace la app. Necesita
    /// `~/.tauri/atic-updater.key` y su contraseña: `cargo test -p atic-sign -- --ignored`.
    #[test]
    #[ignore]
    fn la_app_acepta_lo_que_firma() {
        let keys = home().unwrap().join(".tauri");
        let key = std::fs::read_to_string(keys.join("atic-updater.key")).unwrap();
        let password = std::fs::read_to_string(keys.join("atic-updater.password")).unwrap();
        let file = std::env::temp_dir().join(format!("atic-sign-{}.bin", std::process::id()));
        std::fs::write(&file, b"una actualizacion de prueba").unwrap();
        let signature = sign(&file, &key, password.trim()).unwrap();

        let b64 = base64::engine::general_purpose::STANDARD;
        let text = |s: &str| String::from_utf8(b64.decode(s.trim()).unwrap()).unwrap();
        let public = minisign_verify::PublicKey::decode(&text(PUBKEY)).unwrap();
        let sig = minisign_verify::Signature::decode(&text(&signature)).unwrap();
        public.verify(b"una actualizacion de prueba", &sig, true).expect("la app aceptaría la firma");
        assert!(public.verify(b"otra cosa", &sig, true).is_err());
        std::fs::remove_file(file).ok();
    }
}
