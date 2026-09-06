//! Tipos del hub de orquestación (fase 0: contrato puro, sin I/O).
//!
//! Vive aparte del servidor (fase 1) para que el contrato se pueda probar sin
//! levantar procesos ni sockets: lo que acá cambie rompe al sidecar `atic-mcp`
//! y a la UI, así que estos tipos son la verdad compartida.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `kind` de `atic_delegate`: lo único que mira el ruteo `"auto"`.
/// Nunca se olfatea el texto libre del recado para elegir backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Plan,
    Patch,
    Review,
    Apply,
}

/// Cómo terminó un turno visto por el MCP.
/// Difiere de `model::TurnStatus` a propósito: `timeout` y
/// `permission_timeout` son traspasos (la sesión sigue viva), no estados del hilo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Done,
    Failed,
    Timeout,
    PermissionTimeout,
}

/// Lo que siempre devuelve `atic_prompt` / `atic_delegate` / `atic_wait`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub session: String,
    pub backend: String,
    pub status: OutcomeStatus,
    pub text: String,
    pub hint: Option<String>,
    pub elapsed_s: u64,
}

/// Error del hub: el `message` lo lee el modelo, así que va en tuteo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl HubError {
    pub fn nueva(code: &str, message: String) -> Self {
        Self {
            code: code.into(),
            message,
            data: None,
        }
    }

    pub fn con_datos(code: &str, message: String, data: Value) -> Self {
        Self {
            code: code.into(),
            message,
            data: Some(data),
        }
    }
}

/// Un backend que Atic sabe levantar, con su disponibilidad cacheada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    /// `Some(false)` = está instalado pero sin sesión iniciada. Se informa en
    /// vez de esconderlo: el que delega tiene que poder distinguir «no está»
    /// de «hay que hacer login», que se arreglan distinto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
    pub blurb: String,
}

/// Una sesión viva en Atic. No incluye TUI ajenas que Atic no abrió.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session: String,
    pub backend: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnRequest {
    pub backend: String,
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub depth: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    /// Id de la sesión **o** su nombre: quien pregunta suele tener el nombre a
    /// mano y no el uuid.
    pub session: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_s: Option<u64>,
    /// Quién pregunta (`<uuid>` de Atic o `external:<host>:<pid>`). Lo pone el
    /// sidecar; el hub lo traduce a un nombre para sellar el mensaje.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// Ponerle nombre a una sesión viva, para poder referirse a ella por él.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRequest {
    /// Id o nombre actual.
    pub session: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegateRequest {
    #[serde(default = "por_defecto_auto")]
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default)]
    pub depth: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_s: Option<u64>,
}

fn por_defecto_auto() -> String {
    "auto".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitRequest {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_s: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequest {
    pub session: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El enum `Kind` es cerrado: un `kind` inventado no pasa el contrato y
    /// el hub lo rechaza antes de rutear.
    #[test]
    fn el_kind_que_no_existe_no_pasa_el_contrato() {
        let texto = r#"{"backend":"auto","kind":"magia","text":"hola","depth":0}"#;
        let r: Result<DelegateRequest, _> = serde_json::from_str(texto);
        assert!(r.is_err(), "el enum `Kind` es cerrado");
    }
}
