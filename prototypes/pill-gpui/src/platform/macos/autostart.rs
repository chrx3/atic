//! Iniciar con macOS (`autostart` de `config.json`): un LaunchAgent en
//! `~/Library/LaunchAgents` que abre el ejecutable de la pill al iniciar
//! sesión. Se reescribe solo si cambió.

use std::path::PathBuf;

const LABEL: &str = "com.ciat.atic.pill";

fn agent_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn plist(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>EnvironmentVariables</key>
    <dict>
        <key>MallocLargeCache</key>
        <string>0</string>
    </dict>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        escape(exe)
    )
}

/// Deja el inicio como pide `enabled`, solo si hace falta. Empaquetada, la
/// pill se registra como ítem de inicio (`SMAppService`, macOS 13+): aparece
/// con su nombre en Ajustes del Sistema → General → Ítems de inicio. Suelta,
/// un LaunchAgent.
pub fn sync(enabled: bool) {
    forget_tauri_agent();
    let (Some(path), Ok(exe)) = (agent_path(), std::env::current_exe()) else {
        return;
    };
    if super::notify::bundled() && login_item::sync(enabled) {
        // El LaunchAgent de antes sobra: abriría la pill dos veces.
        let _ = sync_at(&path, "", false);
        return;
    }
    if let Err(error) = sync_at(&path, &exe.display().to_string(), enabled) {
        tracing::warn!(%error, enabled, "no se pudo actualizar el inicio con macOS");
    }
}

/// La Atic de Tauri se abría al iniciar sesión con su propio LaunchAgent
/// (`Atic.plist`, de `tauri-plugin-autostart`) apuntando a `atic-desktop`.
/// Después de pasar a la pill ese ejecutable ya no existe: se borra, y la
/// pill se registra sola con la misma preferencia de `config.json`.
fn forget_tauri_agent() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = PathBuf::from(home).join("Library/LaunchAgents/Atic.plist");
    let stale = std::fs::read_to_string(&path).is_ok_and(|text| text.contains("/Contents/MacOS/atic-desktop"));
    if stale && std::fs::remove_file(&path).is_ok() {
        tracing::info!("inicio con macOS: se quitó el de la Atic de Tauri");
    }
}

fn sync_at(path: &std::path::Path, exe: &str, enabled: bool) -> std::io::Result<()> {
    if enabled {
        let wanted = plist(exe);
        if std::fs::read_to_string(path).ok().as_deref() != Some(wanted.as_str()) {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(path, wanted)?;
            tracing::info!("inicio con macOS: apunta a la pill");
        }
        Ok(())
    } else {
        match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// `SMAppService.mainAppService`, por mensajes (ServiceManagement).
mod login_item {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    #[link(name = "ServiceManagement", kind = "framework")]
    extern "C" {}

    /// `true` si quedó como se pidió.
    pub fn sync(enabled: bool) -> bool {
        let Some(class) = AnyClass::get(c"SMAppService") else {
            return false;
        };
        // SAFETY: `mainAppService` y sus métodos, con el `NSError` opcional.
        unsafe {
            let service: Option<Retained<AnyObject>> = msg_send![class, mainAppService];
            let Some(service) = service else {
                return false;
            };
            // `SMAppServiceStatusEnabled` = 1.
            let status: isize = msg_send![&service, status];
            if (status == 1) == enabled {
                return true;
            }
            let mut error: *mut AnyObject = std::ptr::null_mut();
            let ok: Bool = if enabled {
                msg_send![&service, registerAndReturnError: &mut error]
            } else {
                msg_send![&service, unregisterAndReturnError: &mut error]
            };
            if !ok.as_bool() {
                tracing::warn!(enabled, "inicio con macOS: SMAppService no aceptó el cambio");
            }
            ok.as_bool()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enciende_apaga_y_no_reescribe_si_ya_esta() {
        let dir = std::env::temp_dir().join(format!("atic-pill-autostart-{}", std::process::id()));
        let path = dir.join("agent.plist");
        let exe = "/Applications/Atic & Co.app/Contents/MacOS/pill-gpui";

        sync_at(&path, exe, true).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("Atic &amp; Co.app"));
        sync_at(&path, exe, true).unwrap();
        sync_at(&path, exe, false).unwrap();
        assert!(!path.exists());
        sync_at(&path, exe, false).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }
}
