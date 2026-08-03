//! Ajustes de WebView2 para ventanas flotantes (pill, capturas).

use tauri::{Manager, WebviewWindow};

/// Desactiva atajos del navegador (Ctrl+P imprimir, Ctrl+F buscar, F12, etc.).
#[cfg(windows)]
pub fn disable_browser_accelerator_keys(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows_core::Interface;

    let label = window.label().to_string();
    let label_for_cb = label.clone();
    if let Err(err) = window.with_webview(move |webview| {
        unsafe {
            let Ok(core) = webview.controller().CoreWebView2() else {
                tracing::warn!(label = %label_for_cb, "WebView2: sin CoreWebView2");
                return;
            };
            let Ok(settings) = core.Settings() else {
                tracing::warn!(label = %label_for_cb, "WebView2: sin Settings");
                return;
            };
            let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() else {
                tracing::warn!(label = %label_for_cb, "WebView2: Settings3 no disponible");
                return;
            };
            if let Err(err) = settings3.SetAreBrowserAcceleratorKeysEnabled(false) {
                tracing::warn!(label = %label_for_cb, %err, "no se pudo desactivar atajos del navegador");
            }
        }
    }) {
        tracing::warn!(%label, %err, "with_webview falló al desactivar atajos");
    }
}

#[cfg(not(windows))]
pub fn disable_browser_accelerator_keys(_window: &WebviewWindow) {}

/// Aplica los tweaks a pill + ventanas de captura.
pub fn apply_to_overlay_windows(app: &tauri::AppHandle) {
    for label in ["overlay", "capture-shelf", "capture-overlay", "launcher"] {
        if let Some(window) = app.get_webview_window(label) {
            disable_browser_accelerator_keys(&window);
        }
    }
}
