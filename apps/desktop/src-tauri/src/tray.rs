//! Icono de bandeja (tray) y su menú.

use serde::Deserialize;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::state::{self, AppState};
use atic_core::MutexExt;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayLabels {
    pub show: String,
    pub capture: String,
    pub toggle_pill: String,
    pub summon_pill: String,
    pub quit: String,
}

fn tray_menu(app: &AppHandle, labels: &TrayLabels) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .text("show", &labels.show)
        .text("capture", &labels.capture)
        .text("toggle_pill", &labels.toggle_pill)
        .text("summon_pill", &labels.summon_pill)
        .separator()
        .text("quit", &labels.quit)
        .build()
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let en = app
        .try_state::<AppState>()
        .map(|s| s.config.lock_or_recover().ui_language == "en")
        .unwrap_or(false);
    let labels = if en {
        TrayLabels {
            show: "Open Atic".into(),
            capture: "Capture screen".into(),
            toggle_pill: "Show / hide pill".into(),
            summon_pill: "Bring pill to cursor".into(),
            quit: "Quit".into(),
        }
    } else {
        TrayLabels {
            show: "Abrir Atic".into(),
            capture: "Capturar pantalla".into(),
            toggle_pill: "Mostrar / ocultar pill".into(),
            summon_pill: "Traer pill al cursor".into(),
            quit: "Salir".into(),
        }
    };
    let menu = tray_menu(app, &labels)?;

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

#[tauri::command]
pub fn set_tray_menu(app: AppHandle, labels: TrayLabels) -> Result<(), String> {
    let menu = tray_menu(&app, &labels).map_err(|e| e.to_string())?;
    let tray = app
        .tray_by_id("main-tray")
        .ok_or_else(|| {
            crate::ui_lang::msg(
                "No hay icono de bandeja.",
                "The tray icon is missing.",
            )
        })?;
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())
}
