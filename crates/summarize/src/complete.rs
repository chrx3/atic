//! Una respuesta corta del proveedor de IA, sin plantilla de resumen ni
//! streaming: la usa la traducción de capturas de la pill. Mismo catálogo,
//! llave y modelo que el resumen ([`SummarizerConfig`]).

use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::{Result, SummarizeError};
use crate::providers::{find, ProviderKind};
use crate::thinking::strip_thinking_blocks;
use crate::SummarizerConfig;

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Alcanza para la traducción de una captura llena de texto.
const MAX_TOKENS: u32 = 4096;

/// Le pide al proveedor de `cfg` una respuesta a `user` con las instrucciones
/// `system`. `timeout` cubre toda la petición. Sin reintentos: quien llama
/// muestra el error y ofrece reintentar.
pub fn complete(cfg: &SummarizerConfig, system: &str, user: &str, timeout: Duration) -> Result<String> {
    let info = find(&cfg.backend).ok_or_else(|| SummarizeError::UnknownBackend(cfg.backend.clone()))?;
    let model = if cfg.model.trim().is_empty() { info.default_model.to_string() } else { cfg.model.trim().to_string() };
    let base_url = if cfg.base_url.trim().is_empty() { info.default_base_url } else { cfg.base_url.trim() };
    let base_url = base_url.trim_end_matches('/');
    if model.is_empty() {
        return Err(SummarizeError::BadResponse("falta el modelo del proveedor (Ajustes)".into()));
    }
    let key = cfg.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty());
    let client = Client::builder().timeout(timeout).build()?;

    let raw = match info.kind {
        ProviderKind::Claude => {
            let key = key.ok_or(SummarizeError::MissingApiKey)?;
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert("x-api-key", HeaderValue::from_str(key).map_err(|e| SummarizeError::BadResponse(e.to_string()))?);
            headers.insert("anthropic-version", HeaderValue::from_static(ANTHROPIC_VERSION));
            let body = json!({
                "model": model,
                "max_tokens": MAX_TOKENS,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            });
            claude_text(&post(&client, ANTHROPIC_URL, headers, &body, &model)?)
        }
        ProviderKind::OpenAiCompat => {
            let key = key.ok_or(SummarizeError::MissingApiKey)?;
            if base_url.is_empty() {
                return Err(SummarizeError::BadResponse("falta la URL base del proveedor (Ajustes)".into()));
            }
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {key}")).map_err(|e| SummarizeError::BadResponse(e.to_string()))?,
            );
            let mut body = json!({
                "model": model,
                "stream": false,
                "max_tokens": MAX_TOKENS,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            });
            // Lo mismo que el resumen: sin razonar, que se come el tope de salida.
            if info.id == "minimax" {
                body["thinking"] = json!({ "type": "disabled" });
            }
            if let Some(effort) = crate::openai_compat::reasoning_effort(info.id, &model) {
                body["reasoning_effort"] = json!(effort);
            }
            openai_text(&post(&client, &format!("{base_url}/chat/completions"), headers, &body, &model)?)
        }
        ProviderKind::Ollama => {
            if !crate::ollama::ping(base_url) {
                return Err(SummarizeError::OllamaUnavailable(base_url.to_string()));
            }
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            let body = json!({
                "model": model,
                "stream": false,
                "options": { "temperature": 0.2 },
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            });
            ollama_text(&post(&client, &format!("{base_url}/api/chat"), headers, &body, &model)?)
        }
    };
    let text = strip_thinking_blocks(&raw);
    if text.trim().is_empty() {
        return Err(SummarizeError::BadResponse("el modelo no devolvió texto".into()));
    }
    Ok(text)
}

fn post(client: &Client, url: &str, headers: HeaderMap, body: &Value, model: &str) -> Result<Value> {
    let response = client.post(url).headers(headers).json(body).send()?;
    let status = response.status().as_u16();
    let text = response.text()?;
    if !(200..300).contains(&status) {
        return Err(SummarizeError::from_http(status, &text, model));
    }
    serde_json::from_str(&text).map_err(|e| SummarizeError::BadResponse(e.to_string()))
}

/// Messages API: los bloques `text` de `content`, unidos.
fn claude_text(value: &Value) -> String {
    #[derive(Deserialize)]
    struct Block {
        #[serde(rename = "type")]
        kind: String,
        text: Option<String>,
    }
    let blocks: Vec<Block> = value.get("content").cloned().and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
    blocks.into_iter().filter(|b| b.kind == "text").filter_map(|b| b.text).collect()
}

/// `/chat/completions`: el contenido del primer mensaje (sin `reasoning_content`).
fn openai_text(value: &Value) -> String {
    value.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// `/api/chat` de Ollama sin streaming.
fn ollama_text(value: &Value) -> String {
    value.pointer("/message/content").and_then(Value::as_str).unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_texto_de_cada_proveedor() {
        let claude = json!({"content": [{"type": "thinking", "thinking": "x"}, {"type": "text", "text": "Hola"}, {"type": "text", "text": " mundo"}]});
        assert_eq!(claude_text(&claude), "Hola mundo");
        let openai = json!({"choices": [{"message": {"role": "assistant", "content": "Hi", "reasoning_content": "hmm"}}]});
        assert_eq!(openai_text(&openai), "Hi");
        let ollama = json!({"message": {"role": "assistant", "content": "Ciao"}, "done": true});
        assert_eq!(ollama_text(&ollama), "Ciao");
        assert_eq!(openai_text(&json!({})), "");
    }

    #[test]
    fn sin_llave_no_llama() {
        let cfg = SummarizerConfig {
            backend: "claude".into(),
            api_key: Some("  ".into()),
            model: String::new(),
            base_url: String::new(),
            english: false,
            instructions: None,
        };
        assert!(matches!(complete(&cfg, "s", "u", Duration::from_secs(1)), Err(SummarizeError::MissingApiKey)));
        let unknown = SummarizerConfig { backend: "nada".into(), ..cfg };
        assert!(matches!(complete(&unknown, "s", "u", Duration::from_secs(1)), Err(SummarizeError::UnknownBackend(_))));
    }
}
