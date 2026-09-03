//! Backend OpenAI-compatible (`/v1/chat/completions` + SSE).

use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;

use atic_core::{Summary, Transcript};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use crate::chunk::{self, GROQ_TPM_FALLBACK};
use crate::claude::build_summary;
use crate::error::{Result, SummarizeError};
use crate::prompts::{self, SummaryTemplate};
use crate::thinking::{strip_thinking_blocks, ThinkingFilter};
use crate::{SummarizeProgress, Summarizer};

/// Groq on_demand cuenta `prompt + max_tokens` contra el TPM: 4096 de salida
/// deja sin cupo a una reunión mediana. El resumen no necesita más que esto.
const GROQ_MAX_OUT: u32 = 1024;
const GROQ_MAP_OUT: u32 = 512;
const DEFAULT_MAX_OUT: u32 = 4096;

pub struct OpenAiCompatSummarizer {
    provider_id: String,
    api_key: String,
    base_url: String,
    model: String,
    client: Client,
    english: bool,
}

impl OpenAiCompatSummarizer {
    pub fn new(
        provider_id: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        english: bool,
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
            english,
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
        self.summarize_with_progress(transcript, template, meeting_title, on_delta, &mut |_| {})
    }

    fn summarize_with_progress(
        &self,
        transcript: &Transcript,
        template: SummaryTemplate,
        meeting_title: &str,
        on_delta: &mut dyn FnMut(&str),
        on_progress: &mut dyn FnMut(&SummarizeProgress),
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
        let ep = Endpoint {
            url: &url,
            headers: &headers,
        };
        let max_out = self.max_out();
        let chars = plain.chars().count();

        // Groq: si ya se ve que no cabe, no gastes el minuto en un 413.
        let raw = if let Some(limit) = self.known_tpm_limit() {
            if !chunk::source_fits(chars, limit, max_out) {
                tracing::info!(
                    provider = %self.provider_id,
                    limit,
                    chars,
                    "resumen: la fuente no cabe en un request; va por partes"
                );
                let mut run = Run {
                    ep: &ep,
                    on_progress,
                };
                self.summarize_in_parts(&mut run, &plain, template, meeting_title, limit, on_delta)?
            } else {
                let mut run = Run {
                    ep: &ep,
                    on_progress,
                };
                self.chat_or_parts(&mut run, &plain, template, meeting_title, max_out, on_delta)?
            }
        } else {
            let mut run = Run {
                ep: &ep,
                on_progress,
            };
            self.chat_or_parts(&mut run, &plain, template, meeting_title, max_out, on_delta)?
        };

        Ok(build_summary(
            template,
            meeting_title,
            &self.provider_id,
            &raw,
            self.english,
        ))
    }
}

struct Endpoint<'a> {
    url: &'a str,
    headers: &'a HeaderMap,
}

struct Run<'a> {
    ep: &'a Endpoint<'a>,
    on_progress: &'a mut dyn FnMut(&SummarizeProgress),
}

impl Run<'_> {
    fn report(&mut self, stage: &'static str, part: u32, of: u32) {
        (self.on_progress)(&SummarizeProgress { stage, part, of });
    }
}

impl OpenAiCompatSummarizer {
    fn max_out(&self) -> u32 {
        if self.provider_id == "groq" {
            GROQ_MAX_OUT
        } else {
            DEFAULT_MAX_OUT
        }
    }

    fn map_out(&self) -> u32 {
        if self.provider_id == "groq" {
            GROQ_MAP_OUT
        } else {
            1536
        }
    }

    fn known_tpm_limit(&self) -> Option<u32> {
        (self.provider_id == "groq").then_some(GROQ_TPM_FALLBACK)
    }

    fn fallback_tpm_limit(&self) -> u32 {
        self.known_tpm_limit().unwrap_or(32_000)
    }

    /// Groq on_demand: el TPM es techo por minuto. Encadenar partes ya
    /// agota el cupo; sin pausa el siguiente 413 es inmediato.
    fn pace_between_calls(&self) {
        if self.provider_id == "groq" {
            thread::sleep(Duration::from_secs(45));
        }
    }

    fn wait_after_too_large(&self, err: &SummarizeError) {
        let default = if self.provider_id == "groq" { 45 } else { 5 };
        let secs = err.retry_secs().unwrap_or(default).clamp(1, 90);
        thread::sleep(Duration::from_secs(secs));
    }

    fn chat_or_parts(
        &self,
        run: &mut Run<'_>,
        plain: &str,
        template: SummaryTemplate,
        meeting_title: &str,
        max_out: u32,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let user = prompts::user_prompt_for(template, meeting_title, plain, self.english);
        match self.chat(run.ep, &user, max_out, on_delta) {
            Ok(raw) => Ok(raw),
            Err(err) if err.is_too_large() => {
                let limit = err.tpm_limit().unwrap_or(self.fallback_tpm_limit());
                run.report("wait", 0, 0);
                self.wait_after_too_large(&err);
                tracing::info!(
                    provider = %self.provider_id,
                    limit,
                    chars = plain.chars().count(),
                    "resumen: 413 de cupo; reintenta por partes"
                );
                self.summarize_in_parts(run, plain, template, meeting_title, limit, on_delta)
            }
            Err(err) => Err(err),
        }
    }

    fn summarize_in_parts(
        &self,
        run: &mut Run<'_>,
        plain: &str,
        template: SummaryTemplate,
        meeting_title: &str,
        tpm_limit: u32,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let budget = chunk::chunk_char_budget(tpm_limit, self.map_out());
        let mut parts = chunk::split_plain(plain, budget);
        if parts.len() == 1 {
            // El 413 fue el techo de salida, no la fuente: reintenta más chico.
            let user = prompts::user_prompt_for(template, meeting_title, plain, self.english);
            match self.chat(run.ep, &user, self.map_out(), on_delta) {
                Ok(raw) => return Ok(raw),
                Err(err) if err.is_too_large() => {
                    run.report("wait", 0, 0);
                    self.wait_after_too_large(&err);
                    let tighter = budget.max(2) / 2;
                    parts = chunk::split_plain(plain, tighter);
                    if parts.len() == 1 {
                        return Err(err);
                    }
                }
                Err(err) => return Err(err),
            }
        }

        let n = parts.len() as u32;
        let mut notes = Vec::with_capacity(parts.len());
        for (i, part) in parts.iter().enumerate() {
            let part_n = (i as u32) + 1;
            if i > 0 {
                run.report("wait", part_n, n);
                self.pace_between_calls();
            }
            run.report("map", part_n, n);
            notes.push(self.map_chunk(run.ep, part, i + 1, parts.len(), tpm_limit, 0)?);
        }

        run.report("wait", n, n);
        self.pace_between_calls();
        run.report("reduce", n, n);
        self.reduce_notes(run.ep, notes, template, meeting_title, 0, on_delta)
    }

    fn map_chunk(
        &self,
        ep: &Endpoint<'_>,
        part: &str,
        index: usize,
        of: usize,
        tpm_limit: u32,
        depth: u8,
    ) -> Result<String> {
        let prompt = prompts::map_chunk_prompt(index, of, part, self.english);
        match self.chat(ep, &prompt, self.map_out(), &mut |_| {}) {
            Ok(note) => Ok(note),
            Err(err) if err.is_too_large() && depth < 4 => {
                self.wait_after_too_large(&err);
                let limit = err.tpm_limit().unwrap_or(tpm_limit);
                let half = (part.chars().count() / 2).max(1);
                let bits = chunk::split_plain(part, half);
                if bits.len() == 1 {
                    return Err(err);
                }
                let mut notes = Vec::with_capacity(bits.len());
                for (j, bit) in bits.iter().enumerate() {
                    if j > 0 {
                        self.pace_between_calls();
                    }
                    notes.push(self.map_chunk(ep, bit, index, of, limit, depth + 1)?);
                }
                Ok(notes.join("\n\n"))
            }
            Err(err) => Err(err),
        }
    }

    fn reduce_notes(
        &self,
        ep: &Endpoint<'_>,
        notes: Vec<String>,
        template: SummaryTemplate,
        meeting_title: &str,
        depth: u8,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let source = prompts::reduce_source(&notes, self.english);
        let user = prompts::user_prompt_for(template, meeting_title, &source, self.english);
        match self.chat(ep, &user, self.max_out(), on_delta) {
            Ok(raw) => Ok(raw),
            Err(err) if err.is_too_large() && notes.len() > 1 && depth < 4 => {
                self.wait_after_too_large(&err);
                let mid = notes.len() / 2;
                let left: Vec<String> = notes[..mid].to_vec();
                let right: Vec<String> = notes[mid..].to_vec();
                let left =
                    self.reduce_notes(ep, left, template, meeting_title, depth + 1, &mut |_| {})?;
                self.pace_between_calls();
                let right =
                    self.reduce_notes(ep, right, template, meeting_title, depth + 1, &mut |_| {})?;
                self.pace_between_calls();
                self.reduce_notes(
                    ep,
                    vec![left, right],
                    template,
                    meeting_title,
                    depth + 1,
                    on_delta,
                )
            }
            Err(err) if err.is_too_large() => {
                self.wait_after_too_large(&err);
                self.chat(ep, &user, self.map_out(), on_delta)
            }
            Err(err) => Err(err),
        }
    }

    fn chat(
        &self,
        ep: &Endpoint<'_>,
        user: &str,
        max_tokens: u32,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        let mut last_rate: Option<u64> = None;
        for attempt in 0..3 {
            if let Some(secs) = last_rate.take() {
                thread::sleep(Duration::from_secs(secs.clamp(1, 60)));
            }
            match self.chat_once(ep, user, max_tokens, on_delta) {
                Ok(raw) => return Ok(raw),
                Err(SummarizeError::RateLimited { secs }) if attempt < 2 => {
                    last_rate = Some(secs);
                }
                Err(err) => return Err(err),
            }
        }
        Err(SummarizeError::RateLimited {
            secs: last_rate.unwrap_or(15),
        })
    }

    fn chat_once(
        &self,
        ep: &Endpoint<'_>,
        user: &str,
        max_tokens: u32,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String> {
        // MiniMax-M3 mete reasoning en `content` por defecto; lo desactivamos.
        let thinking = if self.provider_id == "minimax" {
            Some(ThinkingParam { kind: "disabled" })
        } else {
            None
        };
        let body = ChatRequest {
            model: &self.model,
            stream: true,
            max_tokens,
            thinking,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: prompts::system_prompt_for(self.english).to_string(),
                },
                ChatMessage {
                    role: "user",
                    content: user.to_string(),
                },
            ],
        };

        let response = self
            .client
            .post(ep.url)
            .headers(ep.headers.clone())
            .json(&body)
            .send()?;

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
            return Err(SummarizeError::BadResponse(if self.english {
                "the model returned no text (only internal reasoning)".into()
            } else {
                "el modelo no devolvió texto (solo razonamiento interno)".into()
            }));
        }
        Ok(raw)
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
