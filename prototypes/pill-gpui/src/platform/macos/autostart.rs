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
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        escape(exe)
    )
}

/// Deja el LaunchAgent como pide `enabled`, solo si hace falta.
pub fn sync(enabled: bool) {
    let (Some(path), Ok(exe)) = (agent_path(), std::env::current_exe()) else {
        return;
    };
    if let Err(error) = sync_at(&path, &exe.display().to_string(), enabled) {
        tracing::warn!(%error, enabled, "no se pudo actualizar el inicio con macOS");
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
