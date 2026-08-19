//! Backend OpenAI-compatible (`/v1/chat/completions` + SSE).

use std::io::{BufRead, BufReader};

use atic_core::{Summary, Transcript};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::claude::build_summary;
use crate::error::{Result, SummarizeError};
use crate::prompts::{self, SummaryTemplate};
use crate::thinking::{strip_thinking_blocks, ThinkingFilter};
use crate::Summarizer;

pub struct OpenAiCompatSummarizer {
    provider_id: String,
    api_key: String,
    base_url: String,
    model: String,
    client: Client,
}

impl OpenAiCompatSummarizer {
    pub fn new(
        provider_id: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            api_key: api_key.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("cliente HTTP"),
        }
    }
}

impl Summarizer for OpenAiCompatSummarizer {
    fn name(&self) -> &str {
        &self.provider_id
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
        if self.base_url.trim().is_empty() {
            return Err(SummarizeError::BadResponse(
                "falta la URL base del proveedor (Ajustes)".into(),
            ));
        }
        if self.model.trim().is_empty() {
            return Err(SummarizeError::BadResponse(
                "falta el modelo del proveedor (Ajustes)".into(),
            ));
        }
        if self.api_key.trim().is_empty() {
            return Err(SummarizeError::MissingApiKey);
        }

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth = format!("Bearer {}", self.api_key.trim());
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth).map_err(|e| SummarizeError::BadResponse(e.to_string()))?,
        );

        let url = format!("{}/chat/completions", self.base_url);
        // MiniMax-M3 mete reasoning en `content` por defecto; lo desactivamos.
        let thinking = if self.provider_id == "minimax" {
            Some(ThinkingParam { kind: "disabled" })
        } else {
            None
        };
        let body = ChatRequest {
            model: &self.model,
            stream: true,
            max_tokens: 4096,
            thinking,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: prompts::system_prompt().to_string(),
                },
                ChatMessage {
                    role: "user",
                    content: prompts::user_prompt(template, meeting_title, &plain),
                },
            ],
        };

        let response = self.client.post(&url).headers(headers).json(&body).send()?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let text = response.text().unwrap_or_default();
            return Err(SummarizeError::from_http(status, &text, &self.model));
        }

        let mut raw = String::new();
        let mut filter = ThinkingFilter::new();
        let reader = BufReader::new(response);
        for line in reader.lines() {
            let line = line.map_err(|e| SummarizeError::BadResponse(e.to_string()))?;
            let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<SseChunk>(data) else {
                continue;
            };
            if let Some(err) = chunk.error {
                let msg = err
                    .message
                    .unwrap_or_else(|| "error del proveedor OpenAI-compatible".into());
                return Err(SummarizeError::BadResponse(msg));
            }
            if let Some(choices) = chunk.choices {
                for choice in choices {
                    if let Some(delta) = choice.delta {
                        // Ignorar reasoning_content si el proveedor lo separa.
                        if let Some(text) = delta.content {
                            if !text.is_empty() {
                                let visible = filter.push(&text);
                                if !visible.is_empty() {
                                    raw.push_str(&visible);
                                    on_delta(&visible);
                                }
                            }
                        }
                    }
                }
            }
        }
        let tail = filter.finish();
        if !tail.is_empty() {
            raw.push_str(&tail);
            on_delta(&tail);
        }

        let raw = strip_thinking_blocks(&raw);
        if raw.is_empty() {
            return Err(SummarizeError::BadResponse(
                "el modelo no devolvió texto (solo razonamiento interno)".into(),
            ));
        }

        Ok(build_summary(
            template,
            meeting_title,
            &self.provider_id,
            &raw,
        ))
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingParam>,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ThinkingParam {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct SseChunk {
    choices: Option<Vec<SseChoice>>,
    error: Option<SseError>,
}

#[derive(Deserialize)]
struct SseChoice {
    delta: Option<SseDelta>,
}

#[derive(Deserialize)]
struct SseDelta {
    content: Option<String>,
    #[allow(dead_code)]
    reasoning_content: Option<String>,
}

#[derive(Deserialize)]
struct SseError {
    message: Option<String>,
}
