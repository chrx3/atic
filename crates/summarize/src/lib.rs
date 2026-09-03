//! Generación de resúmenes con IA (Claude, Ollama y OpenAI-compatible).

mod chunk;
mod claude;
mod error;
mod models;
mod ollama;
mod openai_compat;
mod prompts;
mod providers;
mod thinking;

pub use claude::ClaudeSummarizer;
pub use error::{Result, SummarizeError};
pub use models::{list_remote_models, order_models, pick_available_model};
pub use ollama::OllamaSummarizer;
pub use openai_compat::OpenAiCompatSummarizer;
pub use prompts::SummaryTemplate;
pub use providers::{find as find_provider, ProviderInfo, ProviderKind, PROVIDERS};

use atic_core::{Summary, Transcript};

/// Aviso de etapa cuando el resumen va por partes (Groq / cupo chico).
///
/// `stage`: `map` (notas de una parte), `wait` (pausa de cupo), `reduce`
/// (documento final). `part`/`of` son 1-based; en `reduce` `part` es `of`.
pub struct SummarizeProgress {
    pub stage: &'static str,
    pub part: u32,
    pub of: u32,
}

/// Contrato de un motor de resumen.
pub trait Summarizer: Send + Sync {
    /// Nombre legible del backend (para la UI y los logs).
    fn name(&self) -> &str;

    /// Genera un resumen a partir de una transcripción.
    ///
    /// `on_delta` recibe trozos de texto a medida que el modelo los genera
    /// (streaming). Puede ser un no-op.
    fn summarize(
        &self,
        transcript: &Transcript,
        template: SummaryTemplate,
        meeting_title: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Summary>;

    /// Igual que [`summarize`], con avisos de etapa (map/wait/reduce).
    fn summarize_with_progress(
        &self,
        transcript: &Transcript,
        template: SummaryTemplate,
        meeting_title: &str,
        on_delta: &mut dyn FnMut(&str),
        on_progress: &mut dyn FnMut(&SummarizeProgress),
    ) -> Result<Summary> {
        let _ = on_progress;
        self.summarize(transcript, template, meeting_title, on_delta)
    }
}

/// Parámetros para construir el summarizer activo.
pub struct SummarizerConfig {
    pub backend: String,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: String,
    pub english: bool,
}

/// Construye el backend configurado según el catálogo de proveedores.
pub fn build_summarizer(cfg: &SummarizerConfig) -> Result<Box<dyn Summarizer>> {
    let info = find_provider(&cfg.backend)
        .ok_or_else(|| SummarizeError::UnknownBackend(cfg.backend.clone()))?;

    let model = if cfg.model.trim().is_empty() {
        info.default_model.to_string()
    } else {
        cfg.model.clone()
    };

    let base_url = if cfg.base_url.trim().is_empty() {
        info.default_base_url.to_string()
    } else {
        cfg.base_url.clone()
    };

    match info.kind {
        ProviderKind::Claude => {
            let key = cfg
                .api_key
                .as_ref()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .ok_or(SummarizeError::MissingApiKey)?;
            Ok(Box::new(ClaudeSummarizer::new(key, model, cfg.english)))
        }
        ProviderKind::Ollama => Ok(Box::new(OllamaSummarizer::new(
            base_url,
            model,
            cfg.english,
        ))),
        ProviderKind::OpenAiCompat => {
            let key = cfg
                .api_key
                .as_ref()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .ok_or(SummarizeError::MissingApiKey)?;
            Ok(Box::new(OpenAiCompatSummarizer::new(
                info.id,
                key,
                base_url,
                model,
                cfg.english,
            )))
        }
    }
}

/// Comprueba si Ollama responde en la URL dada.
pub fn ollama_available(base_url: &str) -> bool {
    ollama::ping(base_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_variants() {
        assert_ne!(
            SummaryTemplate::ExecutiveMinutes,
            SummaryTemplate::ActionItems
        );
        assert_eq!(
            SummaryTemplate::parse("summary_key_points").unwrap(),
            SummaryTemplate::SummaryKeyPoints
        );
        assert_eq!(
            SummaryTemplate::parse("followup_email").unwrap(),
            SummaryTemplate::FollowupEmail
        );
    }

    #[test]
    fn unknown_backend() {
        let cfg = SummarizerConfig {
            backend: "nope".into(),
            api_key: None,
            model: String::new(),
            base_url: String::new(),
            english: false,
        };
        match build_summarizer(&cfg) {
            Err(SummarizeError::UnknownBackend(_)) => {}
            Ok(_) => panic!("expected UnknownBackend"),
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn openai_compat_requires_key() {
        let cfg = SummarizerConfig {
            backend: "minimax".into(),
            api_key: None,
            model: "MiniMax-M3".into(),
            base_url: "https://api.minimax.io/v1".into(),
            english: false,
        };
        assert!(matches!(
            build_summarizer(&cfg),
            Err(SummarizeError::MissingApiKey)
        ));
    }

    #[test]
    fn model_404_is_a_clear_unknown_model() {
        let err = SummarizeError::from_http(
            404,
            r#"{"error":{"message":"The model `x` does not exist","code":"model_not_found"}}"#,
            "meta-llama/llama-4-maverick-17b-128e-instruct",
        );
        match err {
            SummarizeError::UnknownModel { model } => {
                assert!(model.contains("maverick"));
            }
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn groq_413_is_request_too_large_not_raw_json() {
        let err = SummarizeError::from_http(
            413,
            r#"{"error":{"message":"Request too large for model `openai/gpt-oss-120b` on tokens per minute (TPM): Limit 8000, Requested 8272","code":"rate_limit_exceeded"}}"#,
            "openai/gpt-oss-120b",
        );
        match err {
            SummarizeError::RequestTooLarge { limit, .. } => assert_eq!(limit, Some(8000)),
            other => panic!("unexpected {other}"),
        }
        let es = err.to_ui(false);
        assert!(!es.contains("Request too"));
        assert!(es.contains("demasiado larga"));
    }

    #[test]
    fn groq_429_under_limit_is_rate_not_size() {
        let err = SummarizeError::from_http(
            429,
            "Rate limit reached for model `openai/gpt-oss-120b` on tokens per minute (TPM): Limit 8000, Requested 2100, please wait 7 seconds and try again",
            "openai/gpt-oss-120b",
        );
        match err {
            SummarizeError::RateLimited { secs } => assert_eq!(secs, 7),
            other => panic!("unexpected {other}"),
        }
    }
}
