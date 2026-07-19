//! Catálogo de proveedores de resumen (BYOK).

use serde::Serialize;

/// Cómo habla el cliente HTTP con el proveedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Claude,
    Ollama,
    OpenAiCompat,
}

/// Metadatos de un proveedor ofrecido en Ajustes.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub kind: ProviderKind,
    pub default_base_url: &'static str,
    pub default_model: &'static str,
    pub needs_api_key: bool,
    pub base_url_editable: bool,
    /// Nombre del secreto en el llavero (`claude_api_key`, …), si aplica.
    pub secret_kind: Option<&'static str>,
}

/// Catálogo de la primera oleada BYOK.
pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "claude",
        display_name: "Claude (Anthropic)",
        kind: ProviderKind::Claude,
        default_base_url: "",
        default_model: "claude-opus-4-8",
        needs_api_key: true,
        base_url_editable: false,
        secret_kind: Some("claude_api_key"),
    },
    ProviderInfo {
        id: "ollama",
        display_name: "Ollama (local)",
        kind: ProviderKind::Ollama,
        default_base_url: "http://127.0.0.1:11434",
        default_model: "llama3.2",
        needs_api_key: false,
        base_url_editable: true,
        secret_kind: None,
    },
    ProviderInfo {
        id: "openai",
        display_name: "OpenAI",
        kind: ProviderKind::OpenAiCompat,
        default_base_url: "https://api.openai.com/v1",
        default_model: "gpt-4.1-mini",
        needs_api_key: true,
        base_url_editable: false,
        secret_kind: Some("openai_api_key"),
    },
    ProviderInfo {
        id: "openrouter",
        display_name: "OpenRouter",
        kind: ProviderKind::OpenAiCompat,
        default_base_url: "https://openrouter.ai/api/v1",
        default_model: "openai/gpt-4.1-mini",
        needs_api_key: true,
        base_url_editable: false,
        secret_kind: Some("openrouter_api_key"),
    },
    ProviderInfo {
        id: "groq",
        display_name: "Groq",
        kind: ProviderKind::OpenAiCompat,
        default_base_url: "https://api.groq.com/openai/v1",
        default_model: "llama-3.3-70b-versatile",
        needs_api_key: true,
        base_url_editable: false,
        secret_kind: Some("groq_api_key"),
    },
    ProviderInfo {
        id: "minimax",
        display_name: "MiniMax",
        kind: ProviderKind::OpenAiCompat,
        default_base_url: "https://api.minimax.io/v1",
        default_model: "MiniMax-M3",
        needs_api_key: true,
        base_url_editable: false,
        secret_kind: Some("minimax_api_key"),
    },
    ProviderInfo {
        id: "custom",
        display_name: "Custom (OpenAI-compatible)",
        kind: ProviderKind::OpenAiCompat,
        default_base_url: "",
        default_model: "",
        needs_api_key: true,
        base_url_editable: true,
        secret_kind: Some("custom_api_key"),
    },
];

pub fn find(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_minimax_and_custom() {
        assert!(find("minimax").is_some());
        assert!(find("custom").unwrap().base_url_editable);
        assert_eq!(find("claude").unwrap().kind, ProviderKind::Claude);
    }
}
