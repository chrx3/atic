//! Tipos del protocolo hub: copia de `agents::hub::api` del desktop.
//!
//! Copiados a propósito y no en un crate compartido (decisión del plan): el
//! sidecar es un binario chico y el desktop no extrae `agents/` en v1. Si
//! cambias el contrato, cambia los dos lados a la vez.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Plan,
    Patch,
    Review,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Done,
    Failed,
    Timeout,
    PermissionTimeout,
    /// El hijo se detuvo a pedir un permiso: contéstalo con `atic_permission`.
    PermissionRequired,
}

/// Un permiso que el hijo tiene esperando.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingPermission {
    pub id: String,
    pub tool: String,
    pub description: String,
    /// Input de la herramienta en JSON compacto y recortado.
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub session: String,
    pub backend: String,
    pub status: OutcomeStatus,
    pub text: String,
    pub hint: Option<String>,
    pub elapsed_s: u64,
    /// Permisos sin contestar. Un hub viejo no lo manda: por eso `default`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<PendingPermission>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    /// `Some(false)` = instalado pero sin sesión iniciada. Ausente en hubs
    /// viejos, y por eso opcional: el sidecar puede ser más nuevo que Atic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
    pub blurb: String,
}

/// Ponerle nombre a una sesión viva.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRequest {
    pub session: String,
    pub label: String,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Id de la sesión o su nombre.
    pub session: String,
    pub text: String,
    /// Quién pregunta, para que el mensaje llegue firmado a la otra consola.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_s: Option<u64>,
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
pub struct CancelRequest {
    pub session: String,
}

/// Qué se le contesta a un permiso del hijo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAnswer {
    Allow,
    AllowAlways,
    Deny,
}

/// Contestar un permiso de una sesión hija.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_id: Option<String>,
    pub decision: PermissionAnswer,
    /// Quién contesta: el hub solo deja al padre de la sesión o a un ancestro.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// Cerrar la sesión entera y liberar el proceso del agente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseRequest {
    pub session: String,
}

/// Respuesta de `POST /v1/close`: `{ "session", "status": "closed" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Closed {
    pub session: String,
    pub status: String,
}

/// Envoltorios de lista del hub.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentList {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionList {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionOnly {
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cancelling {
    pub session: String,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_resultado_de_ejemplo_deserializa() {
        let texto = r#"{"session":"s1","backend":"codex","status":"timeout","text":"hola","hint":"sigue","elapsed_s":51}"#;
        let r: Outcome = serde_json::from_str(texto).unwrap();
        assert_eq!(r.status, OutcomeStatus::Timeout);
        assert_eq!(r.session, "s1");
        assert!(r.permissions.is_empty(), "un hub viejo no manda permisos");
    }

    #[test]
    fn un_permiso_pendiente_llega_con_id_y_lo_que_quiere_hacer() {
        let texto = r#"{"session":"s1","backend":"codex","status":"permission_required","text":"","hint":"contesta","elapsed_s":3,"permissions":[{"id":"p1","tool":"Bash","description":"correr ls","input":"{\"command\":\"ls\"}"}]}"#;
        let r: Outcome = serde_json::from_str(texto).unwrap();
        assert_eq!(r.status, OutcomeStatus::PermissionRequired);
        assert_eq!(r.permissions[0].id, "p1");
        assert_eq!(r.permissions[0].input, r#"{"command":"ls"}"#);
    }

    #[test]
    fn el_pedido_de_permiso_viaja_en_el_formato_del_hub() {
        let pedido = PermissionRequest {
            session: "s1".into(),
            permission_id: None,
            decision: PermissionAnswer::AllowAlways,
            from: Some("external:claude-code:1".into()),
        };
        let v = serde_json::to_value(&pedido).unwrap();
        assert_eq!(v["decision"], "allow_always");
        assert!(v.get("permissionId").is_none(), "sin id se omite");
        assert_eq!(v["from"], "external:claude-code:1");
    }
}
