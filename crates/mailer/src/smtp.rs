//! Envío SMTP con `lettre` (bloqueante, pensado para hilo de fondo).

use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};

use crate::error::{MailerError, Result};
use crate::{Mailer, OutgoingMail};

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    /// `true` = STARTTLS (típico en 587); `false` = TLS implícito (465).
    pub use_starttls: bool,
}

pub struct SmtpMailer {
    config: SmtpConfig,
}

impl SmtpMailer {
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }
}

impl Mailer for SmtpMailer {
    fn name(&self) -> &str {
        "smtp"
    }

    fn send(&self, mail: &OutgoingMail) -> Result<String> {
        if mail.to.is_empty() {
            return Err(MailerError::NoRecipients);
        }
        if mail.subject.trim().is_empty() {
            return Err(MailerError::EmptySubject);
        }
        if self.config.host.trim().is_empty() || self.config.from.trim().is_empty() {
            return Err(MailerError::MissingSmtpConfig);
        }

        let from: Mailbox = self
            .config
            .from
            .parse()
            .map_err(|e| MailerError::InvalidAddress(format!("{} ({e})", self.config.from)))?;

        let mut builder = Message::builder().from(from).subject(mail.subject.trim());

        for addr in &mail.to {
            let addr = addr.trim();
            if addr.is_empty() {
                continue;
            }
            let mailbox: Mailbox = addr
                .parse()
                .map_err(|e| MailerError::InvalidAddress(format!("{addr} ({e})")))?;
            builder = builder.to(mailbox);
        }

        let message = builder
            .body(mail.body.clone())
            .map_err(|e| MailerError::Smtp(e.to_string()))?;

        let creds = Credentials::new(self.config.username.clone(), self.config.password.clone());

        let transport = if self.config.use_starttls {
            SmtpTransport::starttls_relay(&self.config.host)
                .map_err(|e| MailerError::Smtp(e.to_string()))?
                .port(self.config.port)
                .credentials(creds)
                .build()
        } else {
            SmtpTransport::relay(&self.config.host)
                .map_err(|e| MailerError::Smtp(e.to_string()))?
                .port(self.config.port)
                .credentials(creds)
                .build()
        };

        transport
            .send(&message)
            .map_err(|e| MailerError::Smtp(e.to_string()))?;

        Ok(format!("Correo enviado a {}", mail.to.join(", ")))
    }
}
