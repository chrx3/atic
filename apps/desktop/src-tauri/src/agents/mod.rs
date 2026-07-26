//! Capa de agentes: un modelo de eventos propio y un adaptador por backend.
//!
//! # Por qué existe esta capa
//!
//! La UI podría hablar directamente el formato de Claude Code. Sería menos
//! código hoy y una reescritura mañana: cada agente de consola (Claude Code,
//! Codex, Cursor, OpenCode) emite su propia forma de eventos, y acoplar las
//! vistas a una de ellas convierte "agregar un backend" en "rehacer la UI".
//!
//! Acá el contrato es al revés: cada backend traduce **hacia** [`AgentEvent`],
//! y la UI solo conoce ese tipo. Agregar un agente es escribir un traductor, no
//! tocar pantallas.
//!
//! # Lo que esta capa NO intenta resolver
//!
//! Los agentes no son intercambiables. Tienen herramientas distintas, modelos
//! de permisos distintos y semánticas de sesión distintas. [`AgentEvent`] cubre
//! el denominador común —conversar, ver qué herramienta se usó, saber cuánto
//! costó— y nada más. Lo propio de cada agente (skills, plugins, indexado de
//! repo) no se fuerza dentro de este molde: o queda afuera, o vive en una vista
//! específica de ese backend.

pub mod bridge;
pub mod claude_code;

use serde::Serialize;

/// Un evento de agente, ya normalizado.
///
/// Se serializa a la UI en camelCase con `kind` como discriminante.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentEvent {
    /// La sesión arrancó. `session_id` permite reanudarla más tarde.
    #[serde(rename_all = "camelCase")]
    Started {
        session_id: String,
        /// Herramientas disponibles, para que la UI sepa qué puede aparecer.
        tools: Vec<String>,
        /// Directorio de trabajo del agente.
        cwd: String,
        /// Modelo en uso, tal como lo informa el backend.
        model: String,
        /// Comandos de barra ofrecidos por el agente (skills incluidas).
        slash_commands: Vec<String>,
        /// Servidores MCP que el agente cargó y con qué estado.
        mcp_servers: Vec<McpServerState>,
    },

    /// Texto del asistente, ya completo.
    #[serde(rename_all = "camelCase")]
    Message { text: String },

    /// Un trozo de texto según se escribe.
    ///
    /// El mensaje completo llega DESPUÉS igual, así que la vista acumula los
    /// trozos aparte y los descarta al recibirlo. Sumarlos al registro los
    /// duplicaría; ignorarlos dejaría la respuesta congelada hasta el final,
    /// que en una tarea larga se ve como que el agente se colgó.
    #[serde(rename_all = "camelCase")]
    Delta { text: String },

    /// Razonamiento del modelo, cuando lo expone.
    ///
    /// Va aparte de `Message` porque no es la respuesta: es el trabajo previo.
    /// La vista lo pliega.
    #[serde(rename_all = "camelCase")]
    Thinking { text: String },

    /// El agente va a usar una herramienta.
    ///
    /// `input` queda como JSON sin interpretar a propósito: cada herramienta
    /// tiene su propia forma y esta capa no puede conocerlas todas. La UI
    /// decide si lo renderiza en crudo o con una vista especializada.
    #[serde(rename_all = "camelCase")]
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Resultado de una herramienta.
    #[serde(rename_all = "camelCase")]
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },

    /// El agente pide permiso para usar una herramienta y **está esperando**.
    ///
    /// Nada avanza hasta que se conteste con
    /// [`AgentSession::respond_permission`]. Es el único evento con esa
    /// propiedad, y por eso la UI lo trata distinto: no es una línea más del
    /// registro, es un turno que quedó bloqueado en el usuario.
    #[serde(rename_all = "camelCase")]
    Permission {
        /// Con qué contestar. Es del canal de control, no del turno.
        id: String,
        tool: String,
        /// Resumen corto que ya arma el agente (el archivo, el comando…).
        description: String,
        input: serde_json::Value,
        /// Atajos que sugiere el propio agente, p. ej. «aceptar ediciones
        /// por el resto de la sesión».
        suggestions: serde_json::Value,
    },

    /// Cuánto contexto lleva consumido el turno.
    ///
    /// Se emite por mensaje del asistente, no al final: sirve para una barra
    /// que se mueve mientras trabaja, que es cuando importa saberlo.
    #[serde(rename_all = "camelCase")]
    Context {
        /// Tokens de entrada, incluido el caché: el tamaño real del contexto.
        tokens: u64,
    },

    /// Aviso que no interrumpe: límites de uso, advertencias del backend, o un
    /// tipo de evento que este adaptador todavía no traduce.
    #[serde(rename_all = "camelCase")]
    Notice { text: String },

    /// El turno terminó.
    #[serde(rename_all = "camelCase")]
    Finished {
        stop_reason: Option<String>,
        is_error: bool,
        /// Costo del turno en USD, si el backend lo informa.
        cost_usd: Option<f64>,
    },

    /// El backend falló de una forma de la que no se puede seguir.
    #[serde(rename_all = "camelCase")]
    Failed { message: String },
}

/// Un servidor MCP tal como lo reporta el agente al arrancar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerState {
    pub name: String,
    /// `connected`, `failed`, … Lo informa el backend; no se interpreta acá.
    pub status: String,
}

/// Con qué arrancar una sesión.
///
/// Todo opcional: sin nada, el agente usa su propia configuración. Estas
/// opciones **agregan** a lo que el usuario ya tenga en su CLI, no lo
/// reemplazan; ese es el trato de colgarse de una instalación existente.
#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    /// Directorio de trabajo. `None` = el de la app.
    pub cwd: Option<String>,
    /// Id de sesión previa a reanudar.
    pub resume: Option<String>,
    /// Modelo o alias (`opus`, `sonnet`, …).
    pub model: Option<String>,
    /// Cómo se piden permisos. `None` = pedirlos todos.
    pub permission_mode: Option<String>,
    /// Servidores MCP extra, como JSON `{"mcpServers": {…}}`.
    ///
    /// Son para **el agente**: le suman herramientas a él. Atic solo los
    /// administra y se los pasa al arrancar.
    pub mcp_config: Option<String>,
    /// Carpetas adicionales a las que el agente puede acceder.
    pub add_dirs: Vec<String>,
}

/// Un agente de consola que Atic sabe manejar.
///
/// Implementarlo es todo lo que hace falta para sumar un backend nuevo.
pub trait AgentBackend: Send + Sync {
    /// Identificador estable, para config y persistencia.
    fn id(&self) -> &'static str;

    /// Nombre para mostrar.
    fn display_name(&self) -> &'static str;

    /// ¿Está instalado y utilizable en este equipo?
    ///
    /// Se consulta antes de ofrecerlo: un backend que no está no debería
    /// aparecer como opción y fallar recién al usarlo.
    fn is_available(&self) -> bool;

    /// Arranca una sesión.
    ///
    /// `on_event` corre en los hilos lectores del backend, no en el que llama.
    /// Pide `Sync` porque un backend puede tener más de un lector —Claude Code
    /// lee stdout y stderr en paralelo— y ambos comparten el mismo callback.
    fn start(
        &self,
        options: StartOptions,
        on_event: Box<dyn Fn(AgentEvent) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String>;
}

/// Una sesión viva con un agente.
pub trait AgentSession: Send {
    /// Manda un mensaje del usuario.
    fn send(&mut self, text: &str) -> Result<(), String>;

    /// Contesta un [`AgentEvent::Permission`] pendiente.
    ///
    /// Por defecto no hace nada: un backend que no sabe pedir permiso tampoco
    /// puede recibir una respuesta, y devolver error obligaría a la UI a
    /// distinguir backends. Mientras no emita `Permission`, nadie llama a esto.
    fn respond_permission(&mut self, _id: &str, _allow: bool) -> Result<(), String> {
        Ok(())
    }

    /// Termina la sesión y libera el proceso.
    fn stop(&mut self);
}
