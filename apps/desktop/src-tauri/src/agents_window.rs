//! Ventana dedicada de consolas de agentes.
//!
//! La mudanza (float ⇄ ventana) necesita un dueño con ventana propia: la
//! principal mezcla reuniones, clipboard y ajustes, y no es lugar para
//! terminales vivas. Esta ventana es solo consolas —nativa del SO (marco,
//! Mission Control, ⌘Tab)—, creada bajo demanda en el primer detach.
//!
//! Cerrar oculta, no destruye (`lib.rs`): las PTY siguen vivas y la ventana
//! se reabre desde el tray o el próximo detach. Perfil webview propio, como
//! el overlay: `localStorage` no cruza, el traspaso viaja por eventos.

use tauri::{AppHandle, Manager};

/// Etiqueta y ruta (`src/routes/agents`). Debe coincidir con el frontend.
pub const LABEL: &str = "agents";

/// La crea si no existe y la trae al frente. Idempotente.
pub fn ensure_agents_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("agents".into()))
            .title(crate::ui_lang::pick(
                crate::ui_lang::english(),
                "Consolas de agentes",
                "Agent consoles",
            ))
            .inner_size(1120.0, 760.0)
            .min_inner_size(680.0, 480.0);
    if let Ok(dir) = app.path().app_local_data_dir() {
        builder = builder.data_directory(dir.join("agents-webview"));
    }
    #[cfg(windows)]
    {
        builder = builder.additional_browser_args(
            "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion",
        );
    }
    let window = builder
        .build()
        .map_err(|err| format!("no se pudo abrir la ventana de consolas: {err}"))?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

/// Comando para el frontend (botón de la tool, detach): lo mismo por invoke.
#[tauri::command]
pub fn agents_ensure_window(app: AppHandle) -> Result<(), String> {
    ensure_agents_window(&app)
}
