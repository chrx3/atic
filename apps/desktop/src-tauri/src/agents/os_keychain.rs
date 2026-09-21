//! Lectura de contraseñas genéricas del llavero nativo.
//!
//! Los CLIs de macOS (Claude Code, `agy`, `cursor-agent`) ya no dejan el
//! token en un JSON de `~`: lo guardan en el llavero. Windows sigue usando
//! Credential Manager o archivos; esta pieza solo cubre macOS.

/// Lee una entrada genérica `service`/`account`. `None` si no está o no aplica.
pub fn generic_password(service: &str, account: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos_generic_password(service, account)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (service, account);
        None
    }
}

/// Sobrescribe la entrada. No crea una nueva si no existía.
pub fn set_generic_password(service: &str, account: &str, secret: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos_set_generic_password(service, account, secret)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (service, account, secret);
        Err("este sistema no guarda esta credencial en el llavero".into())
    }
}

#[cfg(target_os = "macos")]
fn macos_generic_password(service: &str, account: &str) -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(target_os = "macos")]
fn macos_set_generic_password(service: &str, account: &str, secret: &str) -> Result<(), String> {
    let status = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            service,
            "-a",
            account,
            "-w",
            secret,
        ])
        .status()
        .map_err(|e| format!("no se pudo escribir el llavero: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("el llavero rechazó actualizar la credencial".into())
    }
}

/// Cuenta con la que Claude Code guarda el OAuth en macOS: el usuario de la sesión.
pub fn claude_keychain_account() -> Option<String> {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|s| !s.trim().is_empty())
}

pub const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
pub const AGY_KEYCHAIN_SERVICE: &str = "gemini";
pub const AGY_KEYCHAIN_ACCOUNT: &str = "antigravity";
pub const CURSOR_KEYCHAIN_SERVICE: &str = "cursor-access-token";
pub const CURSOR_KEYCHAIN_ACCOUNT: &str = "cursor-user";
