//! Comandos y eventos de resumen con IA (multi-proveedor BYOK).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::{RecordingStatus, SecretKind, Summary, Transcript};
use atic_summarize::{self as summarize, ProviderInfo, SummarizerConfig, SummaryTemplate};

use crate::state::AppState;

#[derive(Clone, Serialize)]
struct IdPayload {
    id: String,
}

#[derive(Clone, Serialize)]
struct ErrorIdPayload {
    id: String,
    message: String,
}

#[derive(Clone, Serialize)]
struct DeltaPayload {
    id: String,
    delta: String,
}

#[derive(Clone, Serialize)]
pub struct TemplateInfo {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Serialize)]
pub struct ProviderDto {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub default_base_url: String,
    pub default_model: String,
    pub needs_api_key: bool,
    pub base_url_editable: bool,
    pub secret_kind: Option<String>,
}

impl From<&ProviderInfo> for ProviderDto {
    fn from(p: &ProviderInfo) -> Self {
        Self {
            id: p.id.to_string(),
            display_name: p.display_name.to_string(),
            kind: match p.kind {
                summarize::ProviderKind::Claude => "claude".into(),
                summarize::ProviderKind::Ollama => "ollama".into(),
                summarize::ProviderKind::OpenAiCompat => "openai_compat".into(),
            },
            default_base_url: p.default_base_url.to_string(),
            default_model: p.default_model.to_string(),
            needs_api_key: p.needs_api_key,
            base_url_editable: p.base_url_editable,
            secret_kind: p.secret_kind.map(|s| s.to_string()),
        }
    }
}

/// Lista las plantillas de resumen disponibles.
#[tauri::command]
pub fn list_summary_templates() -> Vec<TemplateInfo> {
    SummaryTemplate::all()
        .iter()
        .map(|t| TemplateInfo {
            id: t.as_str().to_string(),
            label: t.label().to_string(),
        })
        .collect()
}

/// Catálogo de proveedores BYOK.
#[tauri::command]
pub fn list_summary_providers() -> Vec<ProviderDto> {
    summarize::PROVIDERS.iter().map(ProviderDto::from).collect()
}

/// ¿Ollama responde en la URL configurada?
#[tauri::command]
pub fn ollama_available(state: State<AppState>) -> bool {
    let url = state.config.lock().unwrap().summary_base_url.clone();
    let url = if url.trim().is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        url
    };
    summarize::ollama_available(&url)
}

fn summarizer_config_from_app(cfg: &atic_core::Config) -> Result<SummarizerConfig, String> {
    // Distinguir None (no hay clave) de Err (fallo del keyring): tragar el
    // error haría que la UI pida una API key que sí está guardada.
    let api_key = match SecretKind::for_summary_provider(&cfg.summary_backend) {
        Some(kind) => atic_core::secrets::get_secret(kind).map_err(|e| e.to_string())?,
        None => None,
    };

    Ok(SummarizerConfig {
        backend: cfg.summary_backend.clone(),
        api_key,
        model: cfg.summary_model.clone(),
        base_url: cfg.summary_base_url.clone(),
    })
}

/// Genera un resumen en segundo plano (con streaming de deltas).
#[tauri::command]
pub fn summarize_recording(app: AppHandle, id: String, template: String) -> Result<(), String> {
    let state = app.state::<AppState>();

    let rec = state
        .db
        .lock()
        .unwrap()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Grabación no encontrada.".to_string())?;

    let transcript = Transcript::load(&state.dirs.transcript_path(&id))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No hay transcripción. Transcribe primero.".to_string())?;

    let template = SummaryTemplate::parse(&template).map_err(|e| e.to_string())?;
    let cfg = state.config.lock().unwrap().clone();
    let summary_path = state.dirs.summary_path(&id);
    let title = rec.title.clone();

    let summarizer_cfg = summarizer_config_from_app(&cfg)?;
    // Validar backend antes de marcar el estado.
    let _ = summarize::build_summarizer(&summarizer_cfg).map_err(|e| e.to_string())?;

    state
        .db
        .lock()
        .unwrap()
        .update_status(&id, RecordingStatus::Summarizing)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("recordings-changed", ());

    let app2 = app.clone();
    std::thread::spawn(move || {
        run_summarize(
            app2,
            id,
            title,
            transcript,
            template,
            summarizer_cfg,
            summary_path,
        );
    });
    Ok(())
}

fn run_summarize(
    app: AppHandle,
    id: String,
    title: String,
    transcript: Transcript,
    template: SummaryTemplate,
    summarizer_cfg: SummarizerConfig,
    summary_path: std::path::PathBuf,
) {
    let app_delta = app.clone();
    let id_delta = id.clone();
    let result = (|| {
        let summarizer = summarize::build_summarizer(&summarizer_cfg)?;
        let mut on_delta = |delta: &str| {
            let _ = app_delta.emit(
                "summarize-delta",
                DeltaPayload {
                    id: id_delta.clone(),
                    delta: delta.to_string(),
                },
            );
        };
        summarizer.summarize(&transcript, template, &title, &mut on_delta)
    })();

    let state = app.state::<AppState>();
    match result {
        Ok(summary) => {
            if let Err(err) = summary.save(&summary_path) {
                tracing::error!(%err, "no se pudo guardar el resumen");
                let _ = state
                    .db
                    .lock()
                    .unwrap()
                    .update_status(&id, RecordingStatus::Error);
                let _ = app.emit(
                    "summarize-error",
                    ErrorIdPayload {
                        id: id.clone(),
                        message: err.to_string(),
                    },
                );
            } else {
                let _ = state
                    .db
                    .lock()
                    .unwrap()
                    .update_status(&id, RecordingStatus::Summarized);
                let _ = app.emit("summary-ready", IdPayload { id: id.clone() });
            }
        }
        Err(err) => {
            let _ = state
                .db
                .lock()
                .unwrap()
                .update_status(&id, RecordingStatus::Transcribed);
            let _ = app.emit(
                "summarize-error",
                ErrorIdPayload {
                    id: id.clone(),
                    message: err.to_string(),
                },
            );
        }
    }
    let _ = app.emit("recordings-changed", ());
}

/// Devuelve el resumen guardado, si existe.
#[tauri::command]
pub fn get_summary(state: State<AppState>, id: String) -> Result<Option<Summary>, String> {
    Summary::load(&state.dirs.summary_path(&id)).map_err(|e| e.to_string())
}

/// Guarda un resumen editado por el usuario.
#[tauri::command]
pub fn save_summary(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    summary: Summary,
) -> Result<(), String> {
    let _ = state
        .db
        .lock()
        .unwrap()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Grabación no encontrada.".to_string())?;

    summary
        .save(&state.dirs.summary_path(&id))
        .map_err(|e| e.to_string())?;
    state
        .db
        .lock()
        .unwrap()
        .update_status(&id, RecordingStatus::Summarized)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("recordings-changed", ());
    Ok(())
}
