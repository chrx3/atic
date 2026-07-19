//! Comandos de correo y secretos (API keys / SMTP).

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use atic_core::secrets::{self, SecretKind};
use atic_mailer::{self as mailer, OutgoingMail, SmtpConfig};

use crate::state::AppState;

#[derive(Clone, Serialize)]
pub struct SecretsStatus {
    /// `provider_id` → hay API key guardada.
    pub providers: HashMap<String, bool>,
    pub has_smtp_password: bool,
}

/// Indica qué secretos están guardados (sin revelar valores).
#[tauri::command]
pub fn secrets_status() -> SecretsStatus {
    let mut providers = HashMap::new();
    for kind in SecretKind::summary_api_keys() {
        if let Some(id) = kind.provider_id() {
            providers.insert(id.to_string(), secrets::has_secret(*kind));
        }
    }
    SecretsStatus {
        providers,
        has_smtp_password: secrets::has_secret(SecretKind::SmtpPassword),
    }
}

/// Guarda o borra un secreto. `value` vacío elimina la entrada.
#[tauri::command]
pub fn set_secret(kind: String, value: String) -> Result<(), String> {
    let kind = SecretKind::parse(&kind).map_err(|e| e.to_string())?;
    secrets::set_secret(kind, &value).map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
pub struct SendMailResult {
    pub backend: String,
    pub message: String,
    /// Si el backend es mailto, la URL para abrir el cliente.
    pub mailto_url: Option<String>,
}

/// Envía el resumen por SMTP o abre un borrador mailto:.
#[tauri::command]
pub fn send_summary_email(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    to: Vec<String>,
    subject: String,
    body: String,
) -> Result<SendMailResult, String> {
    let _ = state
        .db
        .lock()
        .unwrap()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Grabación no encontrada.".to_string())?;

    let cfg = state.config.lock().unwrap().clone();
    let mail = OutgoingMail {
        to: to
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect(),
        subject,
        body,
    };

    let smtp = if cfg.mail_backend == "smtp" {
        let password = secrets::get_secret(SecretKind::SmtpPassword)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        Some(SmtpConfig {
            host: cfg.smtp_host.clone(),
            port: cfg.smtp_port,
            username: cfg.smtp_username.clone(),
            password,
            from: if cfg.smtp_from.is_empty() {
                cfg.smtp_username.clone()
            } else {
                cfg.smtp_from.clone()
            },
            use_starttls: cfg.smtp_use_tls,
        })
    } else {
        None
    };

    let backend = mailer::build_mailer(&cfg.mail_backend, smtp).map_err(|e| e.to_string())?;
    let message = backend.send(&mail).map_err(|e| e.to_string())?;

    let mailto_url = if backend.name() == "mailto" {
        app.opener()
            .open_url(&message, None::<&str>)
            .map_err(|e| e.to_string())?;
        Some(message.clone())
    } else {
        None
    };

    Ok(SendMailResult {
        backend: backend.name().to_string(),
        message,
        mailto_url,
    })
}
