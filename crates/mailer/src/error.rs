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

pub type Result<T> = std::result::Result<T, MailerError>;
