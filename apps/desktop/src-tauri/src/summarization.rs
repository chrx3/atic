//! Comandos y eventos de resumen con IA (multi-proveedor BYOK).

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::{RecordingStatus, SecretKind, Summary, Transcript};
use atic_summarize::{self as summarize, ProviderInfo, SummarizerConfig, SummaryTemplate};

use crate::state::AppState;
use atic_core::MutexExt;

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
    pub suggested_models: Vec<String>,
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
            suggested_models: p
                .suggested_models
                .iter()
                .map(|m| (*m).to_string())
                .collect(),
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

#[derive(Clone, Serialize)]
pub struct LiveModelsDto {
    pub models: Vec<String>,
    /// True si la lista salió del proveedor ahora; false = catálogo estático.
    pub live: bool,
    pub selected: String,
}

/// Modelos que el proveedor ofrece ahora (`GET /models`). Si el ID guardado
/// ya no está, lo cambia y persiste.
#[tauri::command]
pub async fn list_live_summary_models(app: AppHandle) -> LiveModelsDto {
    tauri::async_runtime::spawn_blocking(move || resolve_and_persist_models(&app))
        .await
        .unwrap_or_else(|_| LiveModelsDto {
            models: Vec::new(),
            live: false,
            selected: String::new(),
        })
}

/// ¿Ollama responde en la URL configurada?
#[tauri::command]
pub fn ollama_available(state: State<AppState>) -> bool {
    let url = state.config.lock_or_recover().summary_base_url.clone();
    let url = if url.trim().is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        url
    };
    summarize::ollama_available(&url)
}

fn catalog_models(info: &ProviderInfo) -> Vec<String> {
    info.suggested_models
        .iter()
        .map(|m| (*m).to_string())
        .collect()
}

struct ResolvedModels {
    models: Vec<String>,
    live: bool,
    selected: String,
}

fn resolve_models(cfg: &SummarizerConfig) -> ResolvedModels {
    let Some(info) = summarize::find_provider(&cfg.backend) else {
        return ResolvedModels {
            models: Vec::new(),
            live: false,
            selected: cfg.model.clone(),
        };
    };
    let fallback = summarize::order_models(catalog_models(info), info.default_model);
    let base = if cfg.base_url.trim().is_empty() {
        info.default_base_url
    } else {
        cfg.base_url.as_str()
    };
    let live = summarize::list_remote_models(info.kind, info.id, base, cfg.api_key.as_deref())
        .ok()
        .filter(|m| !m.is_empty())
        .map(|m| summarize::order_models(m, info.default_model));
    let live_ok = live.is_some();
    let models = live.unwrap_or(fallback);
    let selected = summarize::pick_available_model(&cfg.model, &models, info.default_model);
    ResolvedModels {
        models,
        live: live_ok,
        selected,
    }
}

fn persist_summary_model(app: &AppHandle, model: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let snapshot = {
        let mut cfg = state.config.lock_or_recover();
        if cfg.summary_model == model {
            return;
        }
        tracing::info!(
            from = %cfg.summary_model,
            to = %model,
            "modelo de resumen actualizado (el proveedor ya no lo ofrece)"
        );
        cfg.summary_model = model.to_string();
        cfg.clone()
    };
    let _ = snapshot.save(&state.dirs.config_path());
}

fn resolve_and_persist_models(app: &AppHandle) -> LiveModelsDto {
    let Some(state) = app.try_state::<AppState>() else {
        return LiveModelsDto {
            models: Vec::new(),
            live: false,
            selected: String::new(),
        };
    };
    let cfg = state.config.lock_or_recover().clone();
    let summarizer_cfg = match summarizer_config_from_app(&cfg) {
        Ok(c) => c,
        Err(_) => SummarizerConfig {
            backend: cfg.summary_backend.clone(),
            api_key: None,
            model: cfg.summary_model.clone(),
            base_url: cfg.summary_base_url.clone(),
            english: cfg.ui_language == "en",
        },
    };
    let resolved = resolve_models(&summarizer_cfg);
    persist_summary_model(app, &resolved.selected);
    LiveModelsDto {
        models: resolved.models,
        live: resolved.live,
        selected: resolved.selected,
    }
}

fn heal_summarizer_model(app: &AppHandle, cfg: &mut SummarizerConfig) {
    let resolved = resolve_models(cfg);
    if resolved.selected != cfg.model {
        persist_summary_model(app, &resolved.selected);
        cfg.model = resolved.selected;
    }
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
        english: cfg.ui_language == "en",
    })
}

/// Genera un resumen en segundo plano (con streaming de deltas).
#[tauri::command]
pub fn summarize_recording(app: AppHandle, id: String, template: String) -> Result<(), String> {
    let state = app.state::<AppState>();

    let rec = state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(crate::ui_lang::rec_missing)?;

    let transcript = Transcript::load(&state.dirs.transcript_path(&id))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            crate::ui_lang::msg(
                "No hay transcripción. Transcribe primero.",
                "There is no transcript. Transcribe first.",
            )
        })?;

    let template = SummaryTemplate::parse(&template).map_err(|e| e.to_string())?;
    let cfg = state.config.lock_or_recover().clone();
    let summary_path = state.dirs.summary_path(&id);
    let title = rec.title.clone();

    let summarizer_cfg = summarizer_config_from_app(&cfg)?;
    // Validar backend antes de marcar el estado.
    let _ = summarize::build_summarizer(&summarizer_cfg)
        .map_err(|e| e.to_ui(summarizer_cfg.english))?;

    state
        .db
        .lock_or_recover()
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
    mut summarizer_cfg: SummarizerConfig,
    summary_path: std::path::PathBuf,
) {
    let app_delta = app.clone();
    let id_delta = id.clone();
    let result = (|| {
        heal_summarizer_model(&app, &mut summarizer_cfg);
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
                    .lock_or_recover()
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
                    .lock_or_recover()
                    .update_status(&id, RecordingStatus::Summarized);
                let _ = app.emit("summary-ready", IdPayload { id: id.clone() });
            }
        }
        Err(err) => {
            let _ = state
                .db
                .lock_or_recover()
                .update_status(&id, RecordingStatus::Transcribed);
            let _ = app.emit(
                "summarize-error",
                ErrorIdPayload {
                    id: id.clone(),
                    message: err.to_ui(summarizer_cfg.english),
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
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(crate::ui_lang::rec_missing)?;

    summary
        .save(&state.dirs.summary_path(&id))
        .map_err(|e| e.to_string())?;
    state
        .db
        .lock_or_recover()
        .update_status(&id, RecordingStatus::Summarized)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("recordings-changed", ());
    Ok(())
}
