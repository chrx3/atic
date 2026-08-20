use thiserror::Error;

#[derive(Debug, Error)]
pub enum MailerError {
    #[error("backend de correo desconocido: {0}")]
    UnknownBackend(String),

    #[error("falta la configuración SMTP")]
    MissingSmtpConfig,

    #[error("indica al menos un destinatario")]
    NoRecipients,

    #[error("asunto vacío")]
    EmptySubject,

    #[error("error SMTP: {0}")]
    Smtp(String),

    #[error("dirección de correo inválida: {0}")]
    InvalidAddress(String),
}

impl MailerError {
    pub fn to_ui(&self, en: bool) -> String {
        match self {
            Self::UnknownBackend(id) => {
                if en {
                    format!("Unknown mail backend: {id}")
                } else {
                    self.to_string()
                }
            }
            Self::MissingSmtpConfig => {
                if en {
                    "SMTP settings are missing".into()
                } else {
                    self.to_string()
                }
            }
            Self::NoRecipients => {
                if en {
                    "Add at least one recipient".into()
                } else {
                    self.to_string()
                }
            }
            Self::EmptySubject => {
                if en {
                    "Subject is empty".into()
                } else {
                    self.to_string()
                }
            }
            Self::Smtp(err) => {
                if en {
                    format!("SMTP error: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::InvalidAddress(addr) => {
                if en {
                    format!("Invalid email address: {addr}")
                } else {
                    self.to_string()
                }
            }
        }
    }
}

pub type Result<T> = std::result::Result<T, MailerError>;
