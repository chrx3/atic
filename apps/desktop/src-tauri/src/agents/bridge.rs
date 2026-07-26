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

/// Arranca una sesión y devuelve su clave local.
#[tauri::command]
pub fn agent_start(
    app: AppHandle,
    backend: String,
    cwd: Option<String>,
    resume: Option<String>,
) -> Result<String, String> {
    let agent = find(&backend).ok_or_else(|| format!("backend desconocido: {backend}"))?;
    let key = uuid::Uuid::new_v4().to_string();
    let display_name = agent.display_name().to_string();

    let emit_key = key.clone();
    let emit_backend = backend.clone();
    let emit_name = display_name.clone();
    let session = agent.start(
        StartOptions { cwd, resume },
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
