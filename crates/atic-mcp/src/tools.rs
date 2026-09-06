//! Las ocho tools del servidor `atic`: proxy delgado al hub.
//!
//! Cada tool: localiza al hub (si Atic está cerrado, el copy de `hub_missing`,
//! no un stack), arma el pedido con padre/profundidad/raíz/host desde el env y
//! los args, espera con el presupuesto del host mandando progreso cada 5 s, y
//! devuelve el `Resultado` como JSON. Los errores van como `CallToolResult`
//! con `isError`: el modelo lee mejor eso que un error JSON-RPC.

use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ProgressNotificationParam, ProgressToken};
use rmcp::schemars;
use rmcp::{tool, tool_router, ErrorData as McpError};
use rmcp::{Peer, RoleServer};
use serde::Deserialize;

use crate::budget::wait_budget;
use crate::hub_client::{self, HubFallo};
use crate::payload;

/// De dónde viene el pedido: `ATIC_SESSION` si Atic lo arrancó, o un id
/// externo con el pid del sidecar si el padre es la app original.
fn padre(host: Option<&str>) -> String {
    if let Ok(sesion) = std::env::var("ATIC_SESSION") {
        if !sesion.trim().is_empty() {
            return sesion;
        }
    }
    format!(
        "external:{}:{}",
        host.unwrap_or("unknown"),
        std::process::id()
    )
}

fn profundidad() -> u8 {
    std::env::var("ATIC_DELEGATE_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn raiz() -> Option<String> {
    std::env::var("ATIC_ROOT")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Presupuesto ya recortado al host para este `tools/call`.
fn presupuesto(ctx_host: Option<&str>, flag: Option<u64>, cliente: Option<String>) -> u64 {
    wait_budget(ctx_host, flag, cliente.as_deref())
}

#[derive(Clone)]
pub struct AticServer {
    host: Option<String>,
    wait_flag: Option<u64>,
}

impl AticServer {
    pub fn new(host: Option<String>, wait_flag: Option<u64>) -> Self {
        Self { host, wait_flag }
    }

    /// Nombre del cliente: `_meta.clientInfo` (spec nueva) o el `initialize`
    /// legacy vía el peer. Para el presupuesto sin `--host`.
    fn cliente(ctx: &rmcp::service::RequestContext<RoleServer>) -> Option<String> {
        if let Some(info) = ctx.meta.client_info() {
            if !info.name.trim().is_empty() {
                return Some(info.name);
            }
        }
        ctx.peer
            .peer_info()
            .map(|info| info.client_info.name.clone())
            .filter(|n| !n.trim().is_empty())
    }

    /// Corre el futuro mandando `notifications/progress` cada 5 s si el request
    /// trae `progressToken`. Sin token, no se manda nada.
    async fn con_progreso<T, F>(
        peer: Peer<RoleServer>,
        token: Option<ProgressToken>,
        etiqueta: String,
        futuro: F,
    ) -> T
    where
        F: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let Some(token) = token else {
            return futuro.await;
        };
        let mut futuro = tokio::task::spawn(futuro);
        let mut pasados = 0u64;
        loop {
            tokio::select! {
                salida = &mut futuro => return salida.unwrap_or_else(|_| panic!("el pedido al hub se canceló")),
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    pasados += 5;
                    let mut aviso = ProgressNotificationParam::new(token.clone(), pasados as f64);
                    aviso.message = Some(format!("{etiqueta} ({pasados} s)"));
                    let _ = peer.notify_progress(aviso).await;
                }
            }
        }
    }

    fn error(fallo: HubFallo) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::error(vec![ContentBlock::text(
            fallo.mensaje(),
        )]))
    }

    fn exito<T: serde::Serialize>(valor: &T) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(valor).unwrap_or_default(),
        )]))
    }
}

// --- Parámetros de cada tool (schemas con `additionalProperties: false`) ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DelegateKind {
    Plan,
    Patch,
    Review,
    Apply,
}

impl DelegateKind {
    fn into_payload(self) -> payload::Kind {
        match self {
            Self::Plan => payload::Kind::Plan,
            Self::Patch => payload::Kind::Patch,
            Self::Review => payload::Kind::Review,
            Self::Apply => payload::Kind::Apply,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub enum BackendId {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "grok")]
    Grok,
    #[serde(rename = "antigravity")]
    Antigravity,
}

impl BackendId {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
            Self::Grok => "grok",
            Self::Antigravity => "antigravity",
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub enum BackendOrAuto {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "grok")]
    Grok,
    #[serde(rename = "antigravity")]
    Antigravity,
}

impl BackendOrAuto {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
            Self::Grok => "grok",
            Self::Antigravity => "antigravity",
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Plan,
    BypassPermissions,
    DontAsk,
}

impl PermissionMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::Plan => "plan",
            Self::BypassPermissions => "bypassPermissions",
            Self::DontAsk => "dontAsk",
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListarSesiones {
    /// Id del harness (`claude-code`, `codex`, `opencode`, `cursor`, `grok`,
    /// `antigravity`). Vacío = todas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendId>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbrirSesion {
    pub backend: BackendId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MandarMensaje {
    /// Id de la sesión o el nombre que tenga (`atic_rename` se lo pone).
    pub session: String,
    pub text: String,
    /// En esta versión siempre espera; `false` se rechaza.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

/// Ponerle nombre a una sesión para poder llamarla por él.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Renombrar {
    /// Id de la sesión, o el nombre que tenga ahora.
    pub session: String,
    /// El nombre nuevo, corto y sin espacios de más.
    pub label: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delegar {
    /// Id del harness o `"auto"` (default). Nunca las dos cosas a la vez.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendOrAuto>,
    /// `plan` | `patch` | `review` | `apply`. Solo se usa con `"auto"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<DelegateKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EsperarTurno {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CortarTurno {
    pub session: String,
}

#[tool_router]
impl AticServer {
    #[tool(
        name = "atic_list_agents",
        description = "Lista los agentes CLI que Atic puede levantar (Claude Code, Codex, Cursor, OpenCode, Grok, Antigravity), si están instalados y si tienen sesión iniciada. Llámala antes de elegir un backend concreto."
    )]
    async fn atic_list_agents(&self) -> Result<CallToolResult, McpError> {
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        match hub.agents().await {
            Ok(lista) => Self::exito(&lista),
            Err(e) => Self::error(e),
        }
    }

    #[tool(
        name = "atic_list_sessions",
        description = "Lista las sesiones de agente vivas en Atic: id, backend, carpeta y si están trabajando. Sirve para seguir una conversación con atic_prompt."
    )]
    async fn atic_list_sessions(
        &self,
        params: Parameters<ListarSesiones>,
    ) -> Result<CallToolResult, McpError> {
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        match hub
            .sessions(params.0.backend.as_ref().map(|b| b.as_str()))
            .await
        {
            Ok(lista) => Self::exito(&lista),
            Err(e) => Self::error(e),
        }
    }

    #[tool(
        name = "atic_spawn",
        description = "Abre una sesión nueva de un agente en una carpeta, sin mandarle nada todavía. Devuelve el id de sesión. Si ya hay una viva del mismo agente en esa carpeta, falla y te dice cuál usar."
    )]
    async fn atic_spawn(
        &self,
        params: Parameters<AbrirSesion>,
    ) -> Result<CallToolResult, McpError> {
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        let p = params.0;
        let pedido = payload::SpawnRequest {
            backend: p.backend.as_str().to_string(),
            cwd: p.cwd,
            model: p.model,
            permission_mode: p.permission_mode.map(|m| m.as_str().to_string()),
            label: p.label,
            parent: Some(padre(self.host.as_deref())),
            depth: profundidad(),
            root: raiz(),
            host: self.host.clone(),
        };
        match hub.spawn(&pedido).await {
            Ok(s) => Self::exito(&s),
            Err(e) => Self::error(e),
        }
    }

    #[tool(
        name = "atic_prompt",
        description = "Manda un mensaje a una sesión viva —por id o por nombre— y espera la respuesta. Sirve para hablar con cualquier sesión abierta en Atic, no solo con las que tú creaste. Si el host corta antes de que termine, recibes el texto parcial y el id: sigue con atic_wait."
    )]
    async fn atic_prompt(
        &self,
        ctx: rmcp::service::RequestContext<RoleServer>,
        params: Parameters<MandarMensaje>,
    ) -> Result<CallToolResult, McpError> {
        if params.0.wait == Some(false) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "En esta versión `atic_prompt` siempre espera. Usa `atic_cancel` si quieres cortarlo.",
            )]));
        }
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        let espera_s = presupuesto(self.host.as_deref(), self.wait_flag, Self::cliente(&ctx));
        let pedido = payload::PromptRequest {
            session: params.0.session.clone(),
            text: params.0.text.clone(),
            from: Some(padre(self.host.as_deref())),
            wait_s: Some(espera_s),
        };
        let token = ctx.meta.get_progress_token();
        let etiqueta = "Esperando al agente…".to_string();
        let peer = ctx.peer.clone();
        Self::con_progreso(peer, token, etiqueta, async move {
            match hub.prompt(&pedido, espera_s).await {
                Ok(r) => Self::exito(&r),
                Err(e) => Self::error(e),
            }
        })
        .await
    }

    #[tool(
        name = "atic_rename",
        description = "Le pone nombre a una sesión viva para poder llamarla así después (por ejemplo «agy»). Devuelve el nombre que quedó, que lleva un número al final si ya había otra sesión con ese."
    )]
    async fn atic_rename(&self, params: Parameters<Renombrar>) -> Result<CallToolResult, McpError> {
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        let pedido = payload::RenameRequest {
            session: params.0.session.clone(),
            label: params.0.label.clone(),
        };
        match hub.rename(&pedido).await {
            Ok(r) => Self::exito(&r),
            Err(e) => Self::error(e),
        }
    }

    #[tool(
        name = "atic_delegate",
        description = "Encárgale una tarea a otro agente en un solo paso: lo abre, le manda el texto y espera. Con backend \"auto\", elige según kind (plan, patch, review, apply). La sesión queda viva para seguir con atic_prompt."
    )]
    async fn atic_delegate(
        &self,
        ctx: rmcp::service::RequestContext<RoleServer>,
        params: Parameters<Delegar>,
    ) -> Result<CallToolResult, McpError> {
        // `backend` explícito y `kind` a la vez no tiene sentido: el `kind`
        // solo rutea con `"auto"`.
        if params
            .0
            .backend
            .as_ref()
            .is_some_and(|b| !matches!(b, BackendOrAuto::Auto))
            && params.0.kind.is_some()
        {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "Pasa `backend` o `kind`, no los dos: `kind` solo se usa con `backend: \"auto\"`.",
            )]));
        }
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        let espera_s = presupuesto(self.host.as_deref(), self.wait_flag, Self::cliente(&ctx));
        let p = params.0;
        let pedido = payload::DelegateRequest {
            backend: p
                .backend
                .map(|b| b.as_str().to_string())
                .unwrap_or_else(|| "auto".to_string()),
            kind: p.kind.map(|k| k.into_payload()),
            cwd: p.cwd,
            text: p.text,
            permission_mode: p.permission_mode.map(|m| m.as_str().to_string()),
            model: p.model,
            label: p.label,
            parent: Some(padre(self.host.as_deref())),
            depth: profundidad(),
            root: raiz(),
            host: self.host.clone(),
            wait_s: Some(espera_s),
        };
        let token = ctx.meta.get_progress_token();
        let etiqueta = "Esperando al agente…".to_string();
        let peer = ctx.peer.clone();
        Self::con_progreso(peer, token, etiqueta, async move {
            match hub.delegate(&pedido, espera_s).await {
                Ok(r) => Self::exito(&r),
                Err(e) => Self::error(e),
            }
        })
        .await
    }

    #[tool(
        name = "atic_wait",
        description = "Espera a que termine el turno que ya está corriendo en una sesión, sin mandar nada nuevo. Úsala cuando atic_delegate o atic_prompt volvieron con status timeout."
    )]
    async fn atic_wait(
        &self,
        ctx: rmcp::service::RequestContext<RoleServer>,
        params: Parameters<EsperarTurno>,
    ) -> Result<CallToolResult, McpError> {
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        let tope = presupuesto(self.host.as_deref(), self.wait_flag, Self::cliente(&ctx));
        let espera_s = params.0.timeout_s.unwrap_or(tope).min(tope);
        let pedido = payload::WaitRequest {
            session: params.0.session.clone(),
            wait_s: Some(espera_s),
        };
        let token = ctx.meta.get_progress_token();
        let etiqueta = "Esperando al agente…".to_string();
        let peer = ctx.peer.clone();
        Self::con_progreso(peer, token, etiqueta, async move {
            match hub.wait(&pedido, espera_s).await {
                Ok(r) => Self::exito(&r),
                Err(e) => Self::error(e),
            }
        })
        .await
    }

    #[tool(
        name = "atic_cancel",
        description = "Interrumpe el turno en curso de una sesión. La sesión sigue viva."
    )]
    async fn atic_cancel(
        &self,
        params: Parameters<CortarTurno>,
    ) -> Result<CallToolResult, McpError> {
        let hub = match hub_client::locate().await {
            Ok(h) => h,
            Err(e) => return Self::error(e),
        };
        let pedido = payload::CancelRequest {
            session: params.0.session,
        };
        match hub.cancel(&pedido).await {
            Ok(r) => Self::exito(&r),
            Err(e) => Self::error(e),
        }
    }
}

pub fn router(
    host: Option<String>,
    wait_flag: Option<u64>,
) -> (AticServer, ToolRouter<AticServer>) {
    let servidor = AticServer::new(host, wait_flag);
    // `#[tool_router]` genera `tool_router()` privada: solo se usa acá mismo.
    let router = AticServer::tool_router();
    (servidor, router)
}

/// El handler vive acá (y no en `main.rs`) porque el router generado es
/// privado del módulo: fuera de acá no se puede nombrar.
impl rmcp::ServerHandler for AticServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        let mut info = rmcp::model::ServerInfo::default();
        info.capabilities.tools = Some(rmcp::model::ToolsCapability::default());
        info.server_info.name = "atic".to_string();
        // `Default` trae la versión del SDK (`from_build_env` corre en `rmcp`):
        // se pisa con la del workspace.
        info.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info.instructions = Some(
            "Directorio de agentes de Atic: lista, abre y encarga turnos a otros agentes CLI."
                .to_string(),
        );
        info
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        // La lista es estática: el hub se consulta recién en `tools/call`,
        // porque OpenCode corta el listado a los 5 s.
        Ok(rmcp::model::ListToolsResult {
            tools: AticServer::tool_router().list_all(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        AticServer::tool_router().call(tcc).await
    }
}
