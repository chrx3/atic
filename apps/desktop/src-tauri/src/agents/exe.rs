//! Encontrar los CLIs de agentes. La lógica vive en `atic_agents::exe` (la
//! comparte la pill GPUI).

pub use atic_agents::exe::*;

/// ¿Este binario está en el PATH (con la misma regla que al spawnear)?
#[tauri::command]
pub fn cli_on_path(name: String) -> bool {
    resolve(name.trim()).is_some()
}
