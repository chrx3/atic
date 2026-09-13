//! Permisos TCC de macOS expuestos a la UI de inicio.
//!
//! La pantalla que ve el usuario al arrancar pregunta por tres accesos:
//! micrófono, grabación de pantalla y accesibilidad. Acá vive solo el puente
//! Tauri; los detalles de cada API están en `macos_notes` (macOS) y los
//! stubs para el resto de plataformas devuelven «todo concedido».

use serde::Serialize;

/// `AVAuthorizationStatus` serializado para la UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicrophoneStatus {
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}

#[derive(Debug, Clone, Serialize)]
pub struct MacPermissions {
    pub microphone: MicrophoneStatus,
    /// Grabación de pantalla (capturas, pizarra y audio del sistema).
    pub screen_recording: bool,
    /// Accesibilidad (pegado sintético, cuentagotas).
    pub accessibility: bool,
}

/// Estado actual de los tres permisos. La UI lo consulta al abrir la pantalla
/// y mientras está visible (el usuario los concede en Ajustes, fuera de Atic).
#[tauri::command]
pub fn mac_permissions_status() -> MacPermissions {
    #[cfg(target_os = "macos")]
    {
        use crate::macos_notes::MicrophonePermission as P;

        MacPermissions {
            microphone: match crate::macos_notes::microphone_permission() {
                P::NotDetermined => MicrophoneStatus::NotDetermined,
                P::Restricted => MicrophoneStatus::Restricted,
                P::Denied => MicrophoneStatus::Denied,
                P::Authorized => MicrophoneStatus::Authorized,
            },
            screen_recording: crate::macos_notes::screen_recording_trusted(),
            accessibility: crate::macos_notes::accessibility_trusted(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        MacPermissions {
            microphone: MicrophoneStatus::Authorized,
            screen_recording: true,
            accessibility: true,
        }
    }
}

/// Pide un permiso. Devuelve si quedó concedido en el momento.
///
/// - `microphone`: diálogo del sistema si está sin decidir.
/// - `screen_recording`: diálogo del sistema; si ya estaba denegado, macOS no
///   vuelve a preguntar y hay que ir a Ajustes (la UI muestra el botón).
/// - `accessibility`: no se puede conceder desde código; el diálogo agrega
///   Atic a la lista y se abre Ajustes para que el usuario active el switch.
#[tauri::command]
pub fn mac_request_permission(kind: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        use crate::macos_notes::{self, PrivacyPane};

        match kind.as_str() {
            "microphone" => Ok(macos_notes::request_microphone()),
            "screen_recording" => Ok(macos_notes::request_screen_recording()),
            "accessibility" => {
                let _ = macos_notes::request_accessibility();
                macos_notes::open_privacy_pane(PrivacyPane::Accesibilidad);
                Ok(macos_notes::accessibility_trusted())
            }
            otro => Err(format!("permiso desconocido: {otro}")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        Ok(true)
    }
}

/// Abre el panel de Privacidad y seguridad correspondiente.
#[tauri::command]
pub fn mac_open_privacy_pane(kind: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::macos_notes::{self, PrivacyPane};

        let pane = match kind.as_str() {
            "microphone" => PrivacyPane::Microfono,
            "screen_recording" => PrivacyPane::GrabacionDePantalla,
            "accessibility" => PrivacyPane::Accesibilidad,
            otro => return Err(format!("permiso desconocido: {otro}")),
        };
        macos_notes::open_privacy_pane(pane);
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        Ok(())
    }
}
