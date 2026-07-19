//! Envío de resúmenes por correo (SMTP y borrador `mailto:`).

mod error;
mod mailto;
mod smtp;

pub use error::{MailerError, Result};
pub use mailto::{build_mailto_url, MailtoMailer};
pub use smtp::{SmtpConfig, SmtpMailer};

/// Mensaje listo para enviar o abrir como borrador.
#[derive(Debug, Clone)]
pub struct OutgoingMail {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// Contrato de un backend de envío de correo.
pub trait Mailer: Send + Sync {
    /// Nombre legible del backend (para la UI y los logs).
    fn name(&self) -> &str;

    /// Envía el mensaje (SMTP) o prepara el borrador (`mailto:`).
    ///
    /// Devuelve un mensaje descriptivo para la UI (p. ej. la URL mailto).
    fn send(&self, mail: &OutgoingMail) -> Result<String>;
}

/// Construye el backend configurado (`mailto` | `smtp`).
pub fn build_mailer(backend: &str, smtp: Option<SmtpConfig>) -> Result<Box<dyn Mailer>> {
    match backend {
        "mailto" => Ok(Box::new(MailtoMailer)),
        "smtp" => {
            let cfg = smtp.ok_or(MailerError::MissingSmtpConfig)?;
            Ok(Box::new(SmtpMailer::new(cfg)))
        }
        other => Err(MailerError::UnknownBackend(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailto_url_encodes() {
        let mail = OutgoingMail {
            to: vec!["a@example.com".into(), "b@example.com".into()],
            subject: "Hola & adiós".into(),
            body: "Línea 1\nLínea 2".into(),
        };
        let url = build_mailto_url(&mail).unwrap();
        assert!(url.starts_with("mailto:a@example.com,b@example.com?"));
        assert!(url.contains("subject="));
        assert!(url.contains("body="));
    }

    #[test]
    fn unknown_backend() {
        assert!(matches!(
            build_mailer("fax", None),
            Err(MailerError::UnknownBackend(_))
        ));
    }
}
