//! Puente entre la capa de agentes y el frontend.
//!
//! Registro de sesiones vivas + los comandos de Tauri que las manejan. Los
//! eventos viajan por `agent-event`, cada uno etiquetado con la sesión que lo
//! produjo: la app puede tener varias abiertas a la vez y la UI necesita saber
//! a cuál pertenece cada línea.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::{claude_code::ClaudeCode, AgentBackend, AgentEvent, AgentSession, StartOptions};

/// Una sesión viva más lo que hace falta para nombrarla sin volver a mirar la
/// lista de backends.
struct Entry {
    backend: String,
    display_name: String,
    session: Box<dyn AgentSession>,
}

/// Sesiones abiertas, por clave local.
///
/// La clave la genera Atic y no el backend: el id de sesión del agente llega
/// recién en el primer evento, y para entonces la UI ya necesita algo con qué
/// referirse a la conversación.
static SESSIONS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

/// Backends conocidos. Sumar uno es agregarlo a esta lista.
fn backends() -> Vec<Box<dyn AgentBackend>> {
    vec![Box::new(ClaudeCode)]
}

fn find(id: &str) -> Option<Box<dyn AgentBackend>> {
    backends().into_iter().find(|b| b.id() == id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub id: String,
    pub display_name: String,
    /// Si está instalado. Un backend ausente se muestra deshabilitado en vez
    /// de ofrecerse y fallar recién al usarlo.
    pub available: bool,
}

/// Lo que viaja al frontend en cada evento.
///
/// Lleva el backend además de la sesión porque los eventos son globales: una
/// ventana puede ver la conversación de una sesión que arrancó otra, y sin este
/// dato no tendría con qué nombrarla.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventPayload {
    session: String,
    backend_id: String,
    backend_name: String,
    #[serde(flatten)]
    event: AgentEvent,
}

/// Una sesión abierta, para que una vista que se monta tarde se ponga al día.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
}

/// Qué sesiones siguen vivas.
///
/// El proceso del agente lo tiene Rust, no la ventana que lo arrancó: sigue
/// corriendo con el panel cerrado, y cualquier vista puede adoptarlo.
#[tauri::command]
pub fn agent_sessions() -> Vec<SessionInfo> {
    SESSIONS
        .lock()
        .unwrap()
        .as_ref()
        .map(|map| {
            map.iter()
                .map(|(id, entry)| SessionInfo {
                    id: id.clone(),
                    backend_id: entry.backend.clone(),
                    backend_name: entry.display_name.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Abre la consola de agentes.
///
/// Es una ventana propia y no un panel de la pill: lo que se hace acá —leer
/// salida larga, revisar qué tocó una herramienta, aprobar— necesita espacio y
/// quedarse abierto, que es lo contrario de lo que hace la pill.
#[tauri::command]
pub fn show_agents_window(app: AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("agents") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Qué agentes hay y cuáles se pueden usar.
///
/// `is_available` lanza un proceso por backend, así que esto no es gratis: la
/// UI debería llamarlo al abrir la vista, no en cada render.
#[tauri::command]
pub fn agent_backends() -> Vec<BackendInfo> {
    backends()
        .iter()
        .map(|b| BackendInfo {
            id: b.id().to_string(),
            display_name: b.display_name().to_string(),
            available: b.is_available(),
        })
        .collect()
}

/// Lo que la UI elige antes de arrancar. Todo opcional: sin nada, el agente
/// corre con su propia configuración.
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub cwd: Option<String>,
    pub resume: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    /// JSON `{"mcpServers": {…}}` con los servidores que sume Atic.
    pub mcp_config: Option<String>,
    #[serde(default)]
    pub add_dirs: Vec<String>,
}

/// Arranca una sesión y devuelve su clave local.
#[tauri::command]
pub fn agent_start(
    app: AppHandle,
    backend: String,
    options: Option<StartRequest>,
) -> Result<String, String> {
    let options = options.unwrap_or_default();
    let StartRequest {
        cwd,
        resume,
        model,
        permission_mode,
        mcp_config,
        add_dirs,
    } = options;
    let agent = find(&backend).ok_or_else(|| format!("backend desconocido: {backend}"))?;
    let key = uuid::Uuid::new_v4().to_string();
    let display_name = agent.display_name().to_string();

    let emit_key = key.clone();
    let emit_backend = backend.clone();
    let emit_name = display_name.clone();
    let session = agent.start(
        StartOptions {
            cwd,
            resume,
            model,
            permission_mode,
            mcp_config,
            add_dirs,
        },
        Box::new(move |event| {
            let _ = app.emit(
                "agent-event",
                EventPayload {
                    session: emit_key.clone(),
                    backend_id: emit_backend.clone(),
                    backend_name: emit_name.clone(),
                    event,
                },
            );
        }),
    )?;

    SESSIONS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(
            key.clone(),
            Entry {
                backend,
                display_name,
                session,
            },
        );
    Ok(key)
}

#[tauri::command]
pub fn agent_send(session: String, text: String) -> Result<(), String> {
    let mut guard = SESSIONS.lock().unwrap();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    sessions
        .get_mut(&session)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .send(&text)
}

/// Contesta un permiso pendiente. El turno del agente está detenido hasta acá.
#[tauri::command]
pub fn agent_permission(session: String, id: String, allow: bool) -> Result<(), String> {
    let mut guard = SESSIONS.lock().unwrap();
    guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?
        .get_mut(&session)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .respond_permission(&id, allow)
}

#[tauri::command]
pub fn agent_stop(session: String) {
    let taken = SESSIONS
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|s| s.remove(&session));
    // Fuera del lock: `stop` espera a que el proceso termine de vaciar, y
    // sostener el mutex mientras tanto congelaría cualquier otra sesión.
    if let Some(mut entry) = taken {
        entry.session.stop();
    }
}

/// Cierra todo. Se llama al salir para no dejar procesos huérfanos.
pub fn stop_all() {
    let taken: Vec<_> = SESSIONS
        .lock()
        .unwrap()
        .as_mut()
        .map(|s| s.drain().map(|(_, v)| v).collect())
        .unwrap_or_default();
    for mut entry in taken {
        entry.session.stop();
    }
}
