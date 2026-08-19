//! Lista viva de modelos de un proveedor (`GET /models`, `/api/tags`).
//!
//! El catálogo estático de [`providers`](crate::providers) es el fallback:
//! Groq (y el resto) apagan IDs sin avisar a Atic, y un 404 de modelo es
//! exactamente eso. Pedir la lista al proveedor deja el desplegable al día
//! y permite cambiar el ID guardado antes de gastar un resumen.

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;

use crate::error::{Result, SummarizeError};
use crate::providers::ProviderKind;

const TIMEOUT_SECS: u64 = 8;

/// ¿Sirve este ID para un resumen de texto?
///
/// El endpoint `/models` mezcla chat con audio, embeddings y sistemas
/// agente. Un desplegable con whisper no ayuda.
pub fn is_summarizer_model(id: &str) -> bool {
    let id = id.trim();
    if id.is_empty() {
        return false;
    }
    let lower = id.to_ascii_lowercase();
    const SKIP: &[&str] = &[
        "whisper",
        "tts",
        "orpheus",
        "prompt-guard",
        "llama-guard",
        "safeguard",
        "embedding",
        "dall-e",
        "dall_e",
        "moderation",
        "transcribe",
        "canopylabs",
        "text-embedding",
        "audio",
        "realtime",
        "image",
        "sora",
    ];
    if SKIP.iter().any(|needle| lower.contains(needle)) {
        return false;
    }
    if lower.starts_with("groq/compound") {
        return false;
    }
    true
}

/// Si `current` ya no está en la lista viva, el default del catálogo o el
/// primero que siga existiendo.
pub fn pick_available_model(current: &str, live: &[String], default: &str) -> String {
    if live.iter().any(|m| m == current) {
        return current.to_string();
    }
    if live.iter().any(|m| m == default) {
        return default.to_string();
    }
    live.first().cloned().unwrap_or_else(|| current.to_string())
}

/// Orden estable: default primero, el resto alfabético.
pub fn order_models(mut models: Vec<String>, default: &str) -> Vec<String> {
    models.sort();
    models.dedup();
    if let Some(i) = models.iter().position(|m| m == default) {
        let preferred = models.remove(i);
        models.insert(0, preferred);
    }
    models
}

/// Pregunta al proveedor qué modelos tiene ahora. Vacío o error = usar fallback.
pub fn list_remote_models(
    kind: ProviderKind,
    provider_id: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<String>> {
    match kind {
        ProviderKind::Ollama => list_ollama(base_url),
        ProviderKind::Claude => list_claude(api_key.unwrap_or("")),
        ProviderKind::OpenAiCompat => {
            list_openai_compat(provider_id, base_url, api_key.unwrap_or(""))
        }
    }
}

fn client() -> Result<Client> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| SummarizeError::BadResponse(e.to_string()))
}

fn list_openai_compat(provider_id: &str, base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(SummarizeError::BadResponse(
            "falta la URL base del proveedor".into(),
        ));
    }
    if api_key.trim().is_empty() {
        return Err(SummarizeError::MissingApiKey);
    }

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let auth = format!("Bearer {}", api_key.trim());
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth).map_err(|e| SummarizeError::BadResponse(e.to_string()))?,
    );

    let response = client()?
        .get(format!("{base}/models"))
        .headers(headers)
        .send()?;
    let status = response.status().as_u16();
    let text = response.text().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(SummarizeError::from_http(status, &text, provider_id));
    }
    let parsed: OpenAiModels =
        serde_json::from_str(&text).map_err(|e| SummarizeError::BadResponse(e.to_string()))?;
    Ok(parsed
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| is_summarizer_model(id))
        .collect())
}

fn list_claude(api_key: &str) -> Result<Vec<String>> {
    if api_key.trim().is_empty() {
        return Err(SummarizeError::MissingApiKey);
    }
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "x-api-key",
        HeaderValue::from_str(api_key.trim())
            .map_err(|e| SummarizeError::BadResponse(e.to_string()))?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

    let response = client()?
        .get("https://api.anthropic.com/v1/models")
        .headers(headers)
        .send()?;
    let status = response.status().as_u16();
    let text = response.text().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(SummarizeError::from_http(status, &text, "claude"));
    }
    let parsed: OpenAiModels =
        serde_json::from_str(&text).map_err(|e| SummarizeError::BadResponse(e.to_string()))?;
    Ok(parsed
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| is_summarizer_model(id))
        .collect())
}

fn list_ollama(base_url: &str) -> Result<Vec<String>> {
    let base = base_url.trim().trim_end_matches('/');
    let base = if base.is_empty() {
        "http://127.0.0.1:11434"
    } else {
        base
    };
    let response = client()?.get(format!("{base}/api/tags")).send()?;
    let status = response.status().as_u16();
    let text = response.text().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(SummarizeError::Api {
            status,
            body: text.chars().take(500).collect(),
        });
    }
    let parsed: OllamaTags =
        serde_json::from_str(&text).map_err(|e| SummarizeError::BadResponse(e.to_string()))?;
    Ok(parsed
        .models
        .into_iter()
        .map(|m| m.name)
        .filter(|id| !id.trim().is_empty())
        .collect())
}

#[derive(Deserialize)]
struct OpenAiModels {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

#[derive(Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_audio_and_agent_systems() {
        assert!(!is_summarizer_model("whisper-large-v3-turbo"));
        assert!(!is_summarizer_model("canopylabs/orpheus-v1-english"));
        assert!(!is_summarizer_model("groq/compound"));
        assert!(!is_summarizer_model("text-embedding-3-small"));
        assert!(is_summarizer_model("openai/gpt-oss-120b"));
        assert!(is_summarizer_model("qwen/qwen3.6-27b"));
        assert!(is_summarizer_model("claude-opus-4-8"));
    }

    #[test]
    fn replaces_a_retired_id() {
        let live = vec!["openai/gpt-oss-20b".into(), "openai/gpt-oss-120b".into()];
        assert_eq!(
            pick_available_model(
                "meta-llama/llama-4-maverick-17b-128e-instruct",
                &live,
                "openai/gpt-oss-120b",
            ),
            "openai/gpt-oss-120b"
        );
        assert_eq!(
            pick_available_model("openai/gpt-oss-20b", &live, "openai/gpt-oss-120b"),
            "openai/gpt-oss-20b"
        );
    }

    #[test]
    fn default_comes_first() {
        let ordered = order_models(
            vec!["b".into(), "openai/gpt-oss-120b".into(), "a".into()],
            "openai/gpt-oss-120b",
        );
        assert_eq!(ordered[0], "openai/gpt-oss-120b");
        assert_eq!(ordered[1], "a");
    }
}
