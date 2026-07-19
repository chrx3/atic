//! Fallback: abrir el cliente de correo del usuario con un borrador.

use crate::error::{MailerError, Result};
use crate::{Mailer, OutgoingMail};

pub struct MailtoMailer;

impl Mailer for MailtoMailer {
    fn name(&self) -> &str {
        "mailto"
    }

    fn send(&self, mail: &OutgoingMail) -> Result<String> {
        build_mailto_url(mail)
    }
}

/// Construye una URL `mailto:` con destinatarios, asunto y cuerpo.
pub fn build_mailto_url(mail: &OutgoingMail) -> Result<String> {
    if mail.to.is_empty() {
        return Err(MailerError::NoRecipients);
    }
    if mail.subject.trim().is_empty() {
        return Err(MailerError::EmptySubject);
    }

    let to = mail
        .to
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .collect::<Vec<_>>()
        .join(",");

    if to.is_empty() {
        return Err(MailerError::NoRecipients);
    }

    let subject = urlencoding::encode(mail.subject.trim());
    let body = urlencoding::encode(&mail.body);
    Ok(format!("mailto:{to}?subject={subject}&body={body}"))
}
