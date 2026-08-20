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

pub struct ClaudeSummarizer {
    api_key: String,
    model: String,
    client: Client,
    english: bool,
}

impl ClaudeSummarizer {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>, english: bool) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("cliente HTTP"),
            english,
        }
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
            max_tokens: 4096,
            stream: true,
            system: prompts::system_prompt_for(self.english),
            messages: vec![Message {
                role: "user",
                content: prompts::user_prompt_for(template, meeting_title, &plain, self.english),
            }],
        };

        let response = self
            .client
            .post(ANTHROPIC_URL)
            .headers(headers)
            .json(&body)
            .send()?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let text = response.text().unwrap_or_default();
            return Err(SummarizeError::from_http(status, &text, &self.model));
        }

        let mut raw = String::new();
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
                                on_delta(&text);
                            }
                        }
                    }
                }
            }
            if event.event_type == "error" {
                let msg = event
                    .error
                    .and_then(|e| e.message)
                    .unwrap_or_else(|| {
                    if self.english {
                        "Claude SSE error".into()
                    } else {
                        "error SSE de Claude".into()
                    }
                });
                return Err(SummarizeError::BadResponse(msg));
            }
        }

        let raw = raw.trim().to_string();
        if raw.is_empty() {
            return Err(SummarizeError::BadResponse(
                if self.english {
                    "the model returned no text".into()
                } else {
                    "el modelo no devolvió texto".into()
                },
            ));
        }

        Ok(build_summary(
            template,
            meeting_title,
            "claude",
            &raw,
            self.english,
        ))
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
}

#[derive(Deserialize)]
struct SseError {
    message: Option<String>,
}
