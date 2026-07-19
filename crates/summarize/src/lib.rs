//! Generación de resúmenes con IA (Claude, Ollama y OpenAI-compatible).

mod claude;
mod error;
mod ollama;
mod openai_compat;
mod prompts;
mod providers;
mod thinking;

pub use claude::ClaudeSummarizer;
pub use error::{Result, SummarizeError};
pub use ollama::OllamaSummarizer;
pub use openai_compat::OpenAiCompatSummarizer;
pub use prompts::SummaryTemplate;
pub use providers::{find as find_provider, ProviderInfo, ProviderKind, PROVIDERS};

use atic_core::{Summary, Transcript};

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
}

/// Parámetros para construir el summarizer activo.
pub struct SummarizerConfig {
    pub backend: String,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: String,
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
            Ok(Box::new(ClaudeSummarizer::new(key, model)))
        }
        ProviderKind::Ollama => Ok(Box::new(OllamaSummarizer::new(base_url, model))),
        ProviderKind::OpenAiCompat => {
            let key = cfg
                .api_key
                .as_ref()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .ok_or(SummarizeError::MissingApiKey)?;
            Ok(Box::new(OpenAiCompatSummarizer::new(
                info.id, key, base_url, model,
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
        };
        assert!(matches!(
            build_summarizer(&cfg),
            Err(SummarizeError::MissingApiKey)
        ));
    }
}
