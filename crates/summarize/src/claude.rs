//! Backend Claude vía Messages API (HTTP + SSE streaming).

use std::io::{BufRead, BufReader};

use atic_core::{Summary, Transcript};
use chrono::Utc;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SummarizeError};
use crate::prompts::{self, SummaryTemplate};
use crate::Summarizer;

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// El largo máximo del resumen. Con 4096 las actas de una reunión de dos
/// horas quedaban cortadas.
const MAX_TOKENS: u32 = 8192;
/// Intentos ante un 429, un 5xx (529 es «overloaded») o un corte de red.
const ATTEMPTS: u32 = 3;

pub struct ClaudeSummarizer {
    api_key: String,
    model: String,
    client: Client,
    english: bool,
    instructions: Option<String>,
}

impl ClaudeSummarizer {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>, english: bool) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("cliente HTTP"),
            english,
            instructions: None,
        }
    }

    /// Instrucciones propias en vez de las de la plantilla (un prompt que
    /// escribió la persona). `None`: las de la plantilla.
    pub fn with_instructions(mut self, instructions: Option<String>) -> Self {
        self.instructions = instructions.filter(|i| !i.trim().is_empty());
        self
    }

}

impl Summarizer for ClaudeSummarizer {
    fn name(&self) -> &str {
        "claude"
    }

    fn summarize(
        &self,
        transcript: &Transcript,
        template: SummaryTemplate,
        meeting_title: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Summary> {
        let plain = transcript.to_plain_text();
        if plain.trim().is_empty() {
            return Err(SummarizeError::EmptyTranscript);
        }

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key)
                .map_err(|e| SummarizeError::BadResponse(e.to_string()))?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        let body = MessagesRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            stream: true,
            system: prompts::system_prompt_for(self.english),
            messages: vec![Message {
                role: "user",
                content: prompts::user_prompt_custom(template, self.instructions.as_deref(), meeting_title, &plain, self.english),
            }],
        };

        // Se reintenta solo si todavía no llegó texto: lo que ya se mostró no
        // se puede retirar.
        let mut streamed = false;
        let mut attempt = 0;
        let (raw, truncated) = loop {
            match self.stream_once(&headers, &body, on_delta, &mut streamed) {
                Ok(done) => break done,
                Err(err) => match retry_wait(&err, attempt) {
                    Some(secs) if !streamed && attempt + 1 < ATTEMPTS => {
                        std::thread::sleep(std::time::Duration::from_secs(secs));
                        attempt += 1;
                    }
                    _ => return Err(err),
                },
            }
        };

        let raw = raw.trim().to_string();
        if raw.is_empty() {
            return Err(SummarizeError::BadResponse(if self.english {
                "the model returned no text".into()
            } else {
                "el modelo no devolvió texto".into()
            }));
        }

        let mut summary = build_summary(
            template,
            meeting_title,
            "claude",
            &raw,
            self.english,
        );
        if truncated {
            // Mejor un resumen cortado y avisado que uno que parece completo.
            summary.body.push_str(if self.english {
                "\n\n(The summary was cut short: the model reached its maximum length.)"
            } else {
                "\n\n(El resumen quedó cortado: el modelo llegó a su largo máximo.)"
            });
        }
        Ok(summary)
    }
}

impl ClaudeSummarizer {
    /// Una petición con streaming: el texto y si quedó cortado por
    /// `max_tokens`. `streamed` queda en `true` apenas llega texto.
    fn stream_once(
        &self,
        headers: &HeaderMap,
        body: &MessagesRequest<'_>,
        on_delta: &mut dyn FnMut(&str),
        streamed: &mut bool,
    ) -> Result<(String, bool)> {
        let response = self
            .client
            .post(ANTHROPIC_URL)
            .headers(headers.clone())
            .json(body)
            .send()?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let text = response.text().unwrap_or_default();
            return Err(SummarizeError::from_http(status, &text, &self.model));
        }

        let mut raw = String::new();
        let mut truncated = false;
        let reader = BufReader::new(response);
        for line in reader.lines() {
            let line = line.map_err(|e| SummarizeError::BadResponse(e.to_string()))?;
            let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            let Ok(event) = serde_json::from_str::<SseEvent>(data) else {
                continue;
            };
            if event.event_type == "content_block_delta" {
                if let Some(delta) = event.delta {
                    if delta.delta_type.as_deref() == Some("text_delta") {
                        if let Some(text) = delta.text {
                            if !text.is_empty() {
                                raw.push_str(&text);
                                *streamed = true;
                                on_delta(&text);
                            }
                        }
                    }
                }
            } else if event.event_type == "message_delta" {
                truncated |= event.delta.and_then(|d| d.stop_reason).as_deref() == Some("max_tokens");
            }
            if event.event_type == "error" {
                let msg = event.error.and_then(|e| e.message).unwrap_or_else(|| {
                    if self.english {
                        "Claude SSE error".into()
                    } else {
                        "error SSE de Claude".into()
                    }
                });
                return Err(SummarizeError::BadResponse(msg));
            }
        }
        Ok((raw, truncated))
    }
}

/// Cuánto esperar antes de reintentar, o `None` si no vale la pena: la llave
/// mala o el modelo que no existe no se arreglan esperando.
fn retry_wait(err: &SummarizeError, attempt: u32) -> Option<u64> {
    let backoff = 2u64 * 3u64.pow(attempt);
    match err {
        SummarizeError::RateLimited { secs } => Some((*secs).clamp(1, 60)),
        SummarizeError::Api { status, .. } if *status >= 500 => Some(backoff),
        SummarizeError::Http(e) if !e.is_builder() && !e.is_status() => Some(backoff),
        _ => None,
    }
}

pub(crate) fn build_summary(
    template: SummaryTemplate,
    meeting_title: &str,
    backend: &str,
    raw: &str,
    english: bool,
) -> Summary {
    let (subject, body) = if template == SummaryTemplate::FollowupEmail {
        let (subj, body) = prompts::split_followup_email(raw);
        (subj, body)
    } else {
        (None, raw.trim().to_string())
    };

    Summary {
        template: template.as_str().to_string(),
        title: format!("{} — {}", template.label_for(english), meeting_title),
        body,
        subject,
        backend: backend.to_string(),
        created_at: Utc::now(),
    }
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    stream: bool,
    system: &'a str,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<SseDelta>,
    error: Option<SseError>,
}

#[derive(Deserialize)]
struct SseDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    /// En `message_delta`: por qué terminó (`max_tokens` si quedó cortado).
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseError {
    message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reintenta_lo_pasajero_y_no_lo_demas() {
        let api = |status| SummarizeError::Api { status, body: String::new() };
        assert_eq!(retry_wait(&api(529), 0), Some(2));
        assert_eq!(retry_wait(&api(500), 1), Some(6));
        assert_eq!(retry_wait(&SummarizeError::RateLimited { secs: 20 }, 0), Some(20));
        assert_eq!(retry_wait(&api(401), 0), None);
        assert_eq!(retry_wait(&SummarizeError::UnknownModel { model: "x".into() }, 0), None);
    }

    #[test]
    fn lee_el_corte_por_largo() {
        let event: SseEvent = serde_json::from_str(r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null}}"#).unwrap();
        assert_eq!(event.delta.and_then(|d| d.stop_reason).as_deref(), Some("max_tokens"));
    }
}
