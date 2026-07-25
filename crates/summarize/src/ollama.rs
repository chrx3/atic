//! Backend Ollama local (HTTP `/api/chat` con streaming NDJSON).

use std::io::{BufRead, BufReader};

use atic_core::{Summary, Transcript};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::claude::build_summary;
use crate::error::{Result, SummarizeError};
use crate::prompts::{self, SummaryTemplate};
use crate::thinking::{strip_thinking_blocks, ThinkingFilter};
use crate::Summarizer;

pub struct OllamaSummarizer {
    base_url: String,
    model: String,
    client: Client,
}

impl OllamaSummarizer {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let base = base_url.into().trim_end_matches('/').to_string();
        Self {
            base_url: base,
            model: model.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("cliente HTTP"),
        }
    }
}

impl Summarizer for OllamaSummarizer {
    fn name(&self) -> &str {
        "ollama"
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

        if !ping(&self.base_url) {
            return Err(SummarizeError::OllamaUnavailable(self.base_url.clone()));
        }

        let url = format!("{}/api/chat", self.base_url);
        let body = ChatRequest {
            model: &self.model,
            stream: true,
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

        let response = self.client.post(&url).json(&body).send()?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let text = response.text().unwrap_or_default();
            return Err(SummarizeError::Api {
                status,
                body: text.chars().take(500).collect(),
            });
        }

        let mut raw = String::new();
        let mut filter = ThinkingFilter::new();
        let reader = BufReader::new(response);
        for line in reader.lines() {
            let line = line.map_err(|e| SummarizeError::BadResponse(e.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(&line) else {
                continue;
            };
            if let Some(msg) = chunk.message {
                if !msg.content.is_empty() {
                    let visible = filter.push(&msg.content);
                    if !visible.is_empty() {
                        raw.push_str(&visible);
                        on_delta(&visible);
                    }
                }
            }
            if chunk.done {
                break;
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
                "Ollama no devolvió texto".into(),
            ));
        }

        Ok(build_summary(template, meeting_title, "ollama", &raw))
    }
}

/// ¿Responde Ollama en `base_url`?
pub fn ping(base_url: &str) -> bool {
    let base = base_url.trim_end_matches('/');
    let client = match Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(format!("{base}/api/tags"))
        .send()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    stream: bool,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ChatStreamChunk {
    message: Option<ChatMessageOut>,
    #[serde(default)]
    done: bool,
}

#[derive(Deserialize)]
struct ChatMessageOut {
    content: String,
}
