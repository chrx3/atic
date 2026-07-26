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

/// Sesiones abiertas, por clave local.
///
/// La clave la genera Atic y no el backend: el id de sesión del agente llega
/// recién en el primer evento, y para entonces la UI ya necesita algo con qué
/// referirse a la conversación.
static SESSIONS: Mutex<Option<HashMap<String, Box<dyn AgentSession>>>> = Mutex::new(None);

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
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventPayload {
    session: String,
    #[serde(flatten)]
    event: AgentEvent,
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

    let emit_key = key.clone();
    let session = agent.start(
        StartOptions { cwd, resume },
        Box::new(move |event| {
            let _ = app.emit(
                "agent-event",
                EventPayload {
                    session: emit_key.clone(),
                    event,
                },
            );
        }),
    )?;

    SESSIONS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(key.clone(), session);
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
    if let Some(mut session) = taken {
        session.stop();
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
    for mut session in taken {
        session.stop();
    }
}
