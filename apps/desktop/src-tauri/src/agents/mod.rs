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

pub mod acp;
pub mod bridge;
pub mod claude_code;
pub mod claude_sessions;
pub mod claude_usage;
pub mod codex;
pub mod console;
pub mod discover;
pub mod exe;
pub mod fs_browse;
pub mod media;
pub mod model;
pub mod skills;
pub mod ssh;
pub mod store;
pub mod turns;

use serde::Serialize;

pub use model::AgentDelta;

/// Qué se contesta a un item [`model::ItemKind::Permission`].
///
/// «Siempre» no es «sí» repetido: el agente manda, junto al pedido, la regla
/// que habría que grabar para no volver a preguntar por eso —el modo de
/// permisos, o el patrón de herramienta— y contestar «siempre» es aceptar esa
/// regla, no solo esta invocación. Sin la variante, la única forma de no vivir
/// contestando lo mismo es abrir la sesión en `acceptEdits` desde el principio,
/// que renuncia a preguntar por todo lo demás.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDecision {
    Allow,
    /// Aceptar y grabar la regla que sugirió el agente, por esta sesión.
    AllowAlways,
    Deny,
}

/// Una skill encontrada en disco.
///
/// El agente ya las ofrece como comandos de barra, pero solo el nombre: ni la
/// descripción ni de dónde salió. Eso alcanza para invocarla si ya sabés que
/// existe, y no alcanza para descubrirla, que es justo lo que un selector
/// tiene que resolver.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub name: String,
    pub description: String,
    /// Ruta del `SKILL.md`, para poder abrirlo desde la interfaz.
    pub path: String,
    /// `user` (config del CLI) o `project` (la carpeta de trabajo).
    pub scope: String,
}

/// Un comando de barra que ofrece el agente (incluidas las skills).
///
/// `Deserialize` porque viaja dentro del hilo persistido: al reabrir una
/// conversación guardada, los comandos que el agente ofrecía tienen que volver
/// del disco. Sin eso, el autocompletado quedaría vacío hasta que el backend
/// arranque de nuevo y los vuelva a informar.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    /// Qué espera después del nombre, si espera algo.
    pub argument_hint: String,
}

/// Un servidor MCP tal como lo reporta el agente al arrancar.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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
    /// En remoto es un path POSIX en el host; no se canoniciza en local.
    pub cwd: Option<String>,
    /// Destino SSH. `None` = proceso local (comportamiento histórico).
    pub remote: Option<ssh::RemoteTarget>,
    /// Id de sesión previa a reanudar.
    pub resume: Option<String>,
    /// Id a usar para una sesión nueva, elegido por nosotros.
    ///
    /// Esperar a que el backend lo informe deja un hueco: entre arrancar y ver
    /// el primer evento la sesión no tiene nombre con el que guardarse, y si la
    /// app se cierra en ese rato la conversación queda huérfana en el disco del
    /// CLI, sin forma de encontrarla. Eligiéndolo nosotros, existe desde antes
    /// que el proceso.
    pub session_id: Option<String>,
    /// Al reanudar, seguir en una rama nueva en vez de escribir sobre la vieja.
    pub fork: bool,
    /// Modelo o alias (`opus`, `sonnet`, …).
    pub model: Option<String>,
    /// Cuánto tiene que pensar: `low`, `medium`, `high`, `xhigh`, `max`.
    ///
    /// Los nombres los define cada backend y por eso viaja como texto: Claude
    /// Code lo toma por `--effort` y Codex lo manda en cada turno. Quien no lo
    /// entienda simplemente lo ignora.
    pub effort: Option<String>,
    /// Variante rápida (Cursor `*-fast`). Independiente del nivel de esfuerzo.
    pub fast: Option<bool>,
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
    /// `on_delta` corre en los hilos lectores del backend, **y también en el
    /// hilo que manda mensajes**: el turno del usuario lo abre quien escribe,
    /// no quien lee. Pide `Sync` por eso y porque un backend puede tener más de
    /// un lector —Claude Code lee stdout y stderr en paralelo— compartiendo el
    /// mismo callback.
    fn start(
        &self,
        options: StartOptions,
        on_delta: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>,
    ) -> Result<Box<dyn AgentSession>, String>;
}

/// Una sesión viva con un agente.
pub trait AgentSession: Send {
    /// Manda un mensaje del usuario.
    ///
    /// `origin` dice por qué puente entró —dictado, captura, portapapeles— y no
    /// viaja al agente: es para la conversación. Va acá y no en un comando
    /// aparte porque el item del usuario lo crea el adaptador, al abrir el
    /// turno, y ese es el único momento en que existe algo a lo que colgárselo.
    fn send(&mut self, text: &str, origin: Option<model::Origin>) -> Result<(), String>;

    /// Contesta un item [`model::ItemKind::Permission`] pendiente.
    ///
    /// Por defecto no hace nada: un backend que no sabe pedir permiso tampoco
    /// puede recibir una respuesta, y devolver error obligaría a la UI a
    /// distinguir backends. Mientras no emita `Permission`, nadie llama a esto.
    fn respond_permission(
        &mut self,
        _id: &str,
        _decision: PermissionDecision,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Cambia el modelo, el esfuerzo y (si aplica) la variante rápida.
    ///
    /// Por defecto no hace nada: hay backends que no saben cambiarlo en
    /// caliente —ACP ni siquiera nombra los modelos en su protocolo— y devolver
    /// error obligaría a la vista a saber cuál es cuál. Un backend que no
    /// informa modelos tampoco recibe esta llamada, porque no hay qué elegir.
    fn set_model(
        &mut self,
        _model: &str,
        _effort: Option<&str>,
        _fast: Option<bool>,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Interrumpe el turno en curso sin cerrar la sesión.
    ///
    /// La conversación, el cwd, el modelo y el proceso del agente siguen vivos.
    /// Por defecto no hace nada: un backend sin cancelación nativa no debe
    /// tumbar la sesión solo porque la UI pidió «Detener».
    fn interrupt(&mut self) -> Result<(), String> {
        Ok(())
    }

    /// Termina la sesión y libera el proceso.
    fn stop(&mut self);
}
