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

/// Carpeta del perfil de WebView2 de esta ventana.
fn profile_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join("agents-webview"))
}

/// La crea si no existe y la trae al frente. Idempotente.
pub fn ensure_agents_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let window = build(app, true)?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

/// Espera tras el arranque antes de precalentar: que no compita con él.
const PREWARM_AFTER: std::time::Duration = std::time::Duration::from_secs(8);

/// Crea la ventana escondida para que la primera apertura sea inmediata.
///
/// La primera vez cuesta ~1,8 s: WebView2 arranca un navegador entero para
/// el perfil propio de esta ventana, y recién después carga la pizarra. Solo
/// se adelanta para quien ya la usó —su carpeta de perfil existe—: a quien
/// nunca la abre no le cuesta nada. Y a quien la usa tampoco le suma, porque
/// cerrarla solo la esconde y queda viva el resto de la sesión igual.
pub fn prewarm_if_used(app: &AppHandle) {
    let used = profile_dir(app).is_some_and(|dir| dir.is_dir());
    if !used {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("atic-agents-prewarm".into())
        .spawn(move || {
            std::thread::sleep(PREWARM_AFTER);
            if app.get_webview_window(LABEL).is_some() {
                return;
            }
            if let Err(err) = build(&app, false) {
                tracing::warn!(%err, "no se pudo precargar la ventana de consolas");
            }
        });
}

/// Arma la ventana. `visible: false` la deja lista sin mostrarla ni robar
/// el foco (el precalentado).
fn build(app: &AppHandle, visible: bool) -> Result<tauri::WebviewWindow, String> {
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::App("agents".into()))
            .visible(visible)
            .title(crate::ui_lang::pick(
                crate::ui_lang::english(),
                "Consolas de agentes",
                "Agent consoles",
            ))
            .inner_size(1120.0, 760.0)
            .min_inner_size(680.0, 480.0);
    if let Some(dir) = profile_dir(app) {
        builder = builder.data_directory(dir);
    }
    #[cfg(windows)]
    {
        // Dev: puerto CDP propio, como el overlay (9223). Perfil webview
        // propio: sin esto la ventana queda indepurable.
        #[cfg(debug_assertions)]
        let args = "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion --remote-debugging-port=9224";
        #[cfg(not(debug_assertions))]
        let args = "--disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling --disable-features=CalculateNativeWinOcclusion";
        builder = builder.additional_browser_args(args);
    }
    builder
        .build()
        .map_err(|err| format!("no se pudo abrir la ventana de consolas: {err}"))
}

/// Mitad «esconder» del atajo: solo si la ventana está a la vista y con el
/// foco. Tapada por otra app, el atajo la trae al frente en vez de ocultarla.
/// Devuelve si la escondió.
pub fn hide_if_focused(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(LABEL) else {
        return false;
    };
    let shown = window.is_visible().unwrap_or(false)
        && !window.is_minimized().unwrap_or(false)
        && window.is_focused().unwrap_or(false);
    if shown {
        let _ = window.hide();
    }
    shown
}

/// Comando para el frontend (la pill, el botón de la tool): lo mismo por invoke.
///
/// `async` a propósito: en Windows, crear un WebView2 dentro de un comando
/// síncrono se traba —la llamada no vuelve y la ventana nace en `about:blank`
/// sin navegar—. Desde la bandeja no pasaba porque corre en el hilo principal.
#[tauri::command]
pub async fn agents_ensure_window(app: AppHandle) -> Result<(), String> {
    ensure_agents_window(&app)
}
