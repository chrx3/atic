//! Registrar el servidor `atic` en la config de un CLI del usuario. La lógica
//! vive en `atic_agents::mcp_install` (la comparte la pill GPUI).

use atic_agents::mcp_install::{instalado, instalar, quitar};

/// La ruta del sidecar, o el motivo por el que no se puede ofrecer.
fn sidecar() -> Result<std::path::PathBuf, String> {
    super::hub::mcp_path()
        .ok_or_else(|| "No se encontró atic-mcp. En dev, ejecuta `pnpm mcp:build` primero.".to_string())
}

/// ¿Está el servidor `atic` en la config de este CLI?
#[tauri::command]
pub async fn agent_mcp_status(cli: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || instalado(&cli))
        .await
        .map_err(|e| format!("consulta cancelada: {e}"))?
}

/// Conecta o desconecta este CLI del hub, y devuelve cómo quedó.
#[tauri::command]
pub async fn agent_mcp_toggle(cli: String, on: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if on {
            instalar(&cli, &sidecar()?)?;
        } else {
            quitar(&cli)?;
        }
        instalado(&cli)
    })
    .await
    .map_err(|e| format!("operación cancelada: {e}"))?
}
