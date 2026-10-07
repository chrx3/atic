//! Abrir la ventana principal de Atic (la app de Tauri) bajo demanda: la
//! biblioteca, los Ajustes completos y el workspace de agentes todavía viven
//! ahí. Con `--open` muestra la ventana al arrancar; si ya estaba corriendo,
//! su instancia única la trae al frente.
//!
//! El exe se busca junto al de la pill (así lo deja el instalador), después en
//! la instalación por usuario y, para desarrollo, en `ATIC_DESKTOP_EXE`.

use std::path::PathBuf;

const EXE: &str = "atic-desktop.exe";

fn candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(path) = std::env::var_os("ATIC_DESKTOP_EXE") {
        list.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)) {
        list.push(dir.join(EXE));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        list.push(PathBuf::from(local).join("Atic").join(EXE));
    }
    list
}

pub fn open() {
    let Some(exe) = candidates().into_iter().find(|path| path.is_file()) else {
        tracing::warn!("no se encontró {EXE} para abrir la ventana de Atic");
        return;
    };
    if let Err(error) = std::process::Command::new(&exe).arg("--open").spawn() {
        tracing::warn!(%error, exe = %exe.display(), "no se pudo abrir Atic");
    }
}
