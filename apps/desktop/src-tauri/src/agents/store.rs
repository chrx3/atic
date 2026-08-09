//! Persistencia de conversaciones con agentes.
//!
//! # Qué problema resuelve
//!
//! Hasta acá el hilo vivía en la RAM de la ventana y en la del proceso del CLI,
//! y en ningún disco que Atic supiera leer. Cerrar la app evaporaba la
//! conversación: el CLI sí la había guardado en su propio almacén, pero sin el
//! id con el que encontrarla, y sin el turno del usuario, que nunca había sido
//! un evento.
//!
//! # Cuándo se escribe
//!
//! En los **bordes del turno**, no con cada delta. Los trozos de texto llegan
//! cada pocos milisegundos mientras el agente escribe, y bajar el hilo entero
//! con cada uno costaría mucho para no ganar nada: el texto autoritativo llega
//! igual al cerrar el bloque, así que un trozo perdido no cambia lo guardado.
//!
//! Los puntos de guardado son: fin de turno, fallo del backend, y el cierre de
//! la sesión. El caso que esto no cubre es una caída dura de Atic con un turno a
//! medio correr — ahí se pierde ese turno, no la conversación.

use std::collections::HashMap;
use std::sync::Mutex;

use atic_core::{AgentThreadRow, Db};

use super::model::{AgentDelta, ItemKind, Role, Thread, Turn};

/// Los hilos vivos, indexados por la clave local de la sesión.
///
/// Se mantienen acá y no dentro de cada `AgentSession` porque el hilo tiene que
/// sobrevivir al proceso del agente: al terminar la sesión todavía hay que
/// escribirlo, y para entonces la sesión ya se llevó su `stop()`.
static THREADS: Mutex<Option<HashMap<String, Thread>>> = Mutex::new(None);

/// Cuántos hilos se ofrecen al listar. Es una lista que se abre y se mira; sin
/// tope, meses de uso cargarían megabytes para mostrar diez líneas.
pub const LIST_LIMIT: u32 = 200;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Empieza a seguir una sesión.
pub fn open(
    id: &str,
    backend_id: &str,
    backend_name: &str,
    cwd: &str,
    remote_host_id: Option<&str>,
) {
    let thread = Thread {
        id: id.to_string(),
        backend_id: backend_id.to_string(),
        backend_name: backend_name.to_string(),
        provider_session: None,
        cwd: cwd.to_string(),
        remote_host_id: remote_host_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        model: String::new(),
        mode: String::new(),
        turns: Vec::new(),
        updated_at: now(),
    };
    if let Ok(mut guard) = THREADS.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(id.to_string(), thread);
    }
}

/// Aplica un delta al hilo en memoria y dice si toca bajarlo a disco.
///
/// Devuelve el booleano en vez de escribir acá porque quien llama tiene el
/// `Db` y este módulo corre en el hilo lector del backend: pedir el candado de
/// la base con cada trozo de texto lo bloquearía por nada.
pub fn apply(id: &str, delta: &AgentDelta) -> bool {
    let Ok(mut guard) = THREADS.lock() else {
        return false;
    };
    let Some(thread) = guard.as_mut().and_then(|m| m.get_mut(id)) else {
        return false;
    };
    thread.apply(delta);
    thread.updated_at = now();
    matches!(
        delta,
        AgentDelta::TurnEnd { .. } | AgentDelta::Failed { .. }
    )
}

/// Baja el hilo a disco. Silencioso si la sesión ya no existe.
pub fn flush(db: &Db, id: &str) {
    let row = {
        let Ok(guard) = THREADS.lock() else {
            return;
        };
        let Some(thread) = guard.as_ref().and_then(|m| m.get(id)) else {
            return;
        };
        // Un hilo sin turnos no se guarda: abrir el panel, mirar y cerrarlo
        // llenaría la lista de conversaciones vacías.
        if thread.turns.is_empty() {
            return;
        }
        let preview = thread_preview(&thread.turns);
        match serde_json::to_string(&thread.turns) {
            Ok(turns) => AgentThreadRow {
                id: thread.id.clone(),
                backend_id: thread.backend_id.clone(),
                backend_name: thread.backend_name.clone(),
                provider_session: thread.provider_session.clone(),
                cwd: thread.cwd.clone(),
                remote_host_id: thread.remote_host_id.clone(),
                model: thread.model.clone(),
                updated_at: thread.updated_at,
                preview,
                turns,
            },
            Err(e) => {
                tracing::warn!("no se pudo serializar el hilo {id}: {e}");
                return;
            }
        }
    };
    if let Err(e) = db.save_agent_thread(&row) {
        tracing::warn!("no se pudo guardar el hilo {id}: {e}");
    }
}

/// Deja de seguir una sesión, después de bajarla.
pub fn close(db: &Db, id: &str) {
    flush(db, id);
    if let Ok(mut guard) = THREADS.lock() {
        if let Some(map) = guard.as_mut() {
            map.remove(id);
        }
    }
}

/// Las claves de todas las sesiones seguidas. Para el cierre de la app.
pub fn tracked() -> Vec<String> {
    THREADS
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|m| m.keys().cloned().collect()))
        .unwrap_or_default()
}

/// Un hilo guardado, ya listo para la interfaz.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredThread {
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
    pub provider_session: Option<String>,
    pub cwd: String,
    pub remote_host_id: Option<String>,
    pub model: String,
    pub updated_at: i64,
    /// Primeras palabras del usuario. Es con lo que se reconoce una
    /// conversación en una lista; el id no le dice nada a nadie.
    pub preview: String,
    pub turns: Vec<Turn>,
}

fn to_stored(row: AgentThreadRow, with_turns: bool) -> StoredThread {
    let turns = if with_turns {
        serde_json::from_str(&row.turns).unwrap_or_default()
    } else {
        Vec::new()
    };
    StoredThread {
        id: row.id,
        backend_id: row.backend_id,
        backend_name: row.backend_name,
        provider_session: row.provider_session,
        cwd: row.cwd,
        remote_host_id: row.remote_host_id,
        model: row.model,
        updated_at: row.updated_at,
        preview: row.preview,
        // Al listar no se mandan: son la conversación entera de cada hilo, y
        // la lista solo necesita con qué reconocerlos.
        turns,
    }
}

fn thread_preview(turns: &[Turn]) -> String {
    turns
        .iter()
        .flat_map(|turn| &turn.items)
        .find_map(|i| match &i.kind {
            ItemKind::Message {
                role: Role::User,
                text,
                ..
            } => Some(text.chars().take(120).collect::<String>()),
            _ => None,
        })
        .unwrap_or_default()
}

pub fn list(db: &Db) -> Result<Vec<StoredThread>, String> {
    db.list_agent_threads(LIST_LIMIT)
        .map(|rows| rows.into_iter().map(|r| to_stored(r, false)).collect())
        .map_err(|e| e.to_string())
}

pub fn get(db: &Db, id: &str) -> Result<Option<StoredThread>, String> {
    db.get_agent_thread(id)
        .map(|row| row.map(|r| to_stored(r, true)))
        .map_err(|e| e.to_string())
}

pub fn delete(db: &Db, id: &str) -> Result<(), String> {
    db.delete_agent_thread(id).map_err(|e| e.to_string())
}
