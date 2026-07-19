//! Icono de bandeja (tray) y su menú.

use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::state;

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("show", "Abrir Atic")
        .text("capture", "Capturar pantalla")
        .text("toggle_pill", "Mostrar / ocultar pill")
        .text("summon_pill", "Traer pill al cursor")
        .separator()
        .text("quit", "Salir")
        .build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .expect("icono de ventana embebido");

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Atic")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => state::show_main(app),
            "capture" => {
                if let Err(error) = crate::capture_session::trigger(app) {
                    tracing::warn!(%error, "no se pudo abrir el overlay de captura");
                }
            }
            "toggle_pill" => state::toggle_pill(app),
            "summon_pill" => state::summon_pill_to_cursor(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                state::show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
