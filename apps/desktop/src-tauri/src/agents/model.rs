//! El modelo canónico: hilo → turnos → items con identidad.
//!
//! # Por qué no alcanzaba un registro plano
//!
//! La primera versión de esta capa emitía una lista de eventos sueltos
//! ([`super::AgentEvent`]): la llamada a una herramienta era un evento y su
//! resultado otro, unidos solo por un id que la vista tenía que emparejar
//! recorriendo el registro. Con eso, una tarjeta no puede pasar de «ejecutando»
//! a «listo» —se vuelve a dibujar, o se duplica—, los trozos de texto no dicen a
//! qué mensaje pertenecen (de ahí el `streaming` suelto que vivía en paralelo al
//! registro, y que se rompía apenas dos bloques transmitían a la vez), y nada de
//! eso se puede guardar: no hay nada con identidad que escribir en disco.
//!
//! Acá lo visible son **items con id estable y estado mutable**, agrupados en
//! turnos. Lo que viaja a la interfaz ya no es «qué pasó» sino **qué cambió**
//! ([`AgentDelta`]), y ese mismo delta es la unidad que se persiste.
//!
//! # Por qué esta forma y no una propia
//!
//! Es, a propósito, la forma de ACP (`session/update`): `tool_call` que después
//! se muta con `tool_call_update`, trozos de mensaje y de razonamiento, plan,
//! comandos disponibles, uso. OpenCode y Cursor hablan ACP nativo, así que sus
//! adaptadores quedan casi como passthrough; Claude Code y Codex traducen hacia
//! un estándar en vez de hacia una invención local que habría que redefinir con
//! cada agente nuevo.
//!
//! # Lo que este modelo NO intenta resolver
//!
//! El denominador común: conversar, ver qué herramienta se usó y en qué quedó,
//! decidir permisos, saber cuánto costó. Lo propio de cada agente —plugins,
//! indexado de repo, modos de razonamiento— no se fuerza acá dentro.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Id de un item. Estable durante toda la vida del item.
///
/// No es opaco por casualidad: cuando el backend ya trae uno estable —el
/// `tool_use.id` de Claude, el `toolCallId` de ACP— se usa **ese**, y así el
/// adaptador no tiene que mantener una tabla de equivalencias que sincronizar.
pub type ItemId = String;

/// Id de un turno: un ciclo usuario → agente.
pub type TurnId = String;

/// Quién habla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    User,
    Assistant,
}

/// Qué clase de herramienta es, para que la vista elija ícono sin adivinar.
///
/// Son las categorías de ACP tal cual. Antes la interfaz deducía esto del
/// nombre y de la forma de la entrada cruda (`file_path` → es un archivo,
/// `command` → es un comando), que funciona con las herramientas que ya
/// conocés y con ninguna otra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Collab,
    Other,
}

impl ToolKind {
    /// Deduce la categoría por el nombre de la herramienta.
    ///
    /// Es un apaño para los backends que no la informan —Claude Code no la
    /// manda— y no un mecanismo general: los nombres son los del CLI de
    /// Anthropic. Un backend que sí traiga `kind` NO debe pasar por acá.
    pub fn guess(name: &str) -> Self {
        match name {
            "Read" | "NotebookRead" => Self::Read,
            "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => Self::Edit,
            "Grep" | "Glob" => Self::Search,
            "Bash" | "BashOutput" | "KillShell" => Self::Execute,
            "WebFetch" | "WebSearch" => Self::Fetch,
            "Task" | "Agent" => Self::Collab,
            "TodoWrite" | "ExitPlanMode" => Self::Think,
            _ => Self::Other,
        }
    }
}

/// En qué anda una herramienta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// En qué quedó un pedido de permiso.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionStatus {
    /// El turno está detenido esperando respuesta.
    Pending,
    Allowed,
    Denied,
}

/// Un paso del plan que el agente propone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub text: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

/// Todo lo que la conversación muestra.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ItemKind {
    /// Un mensaje. `streaming` dice si todavía se está escribiendo.
    #[serde(rename_all = "camelCase")]
    Message {
        role: Role,
        text: String,
        streaming: bool,
    },

    /// Razonamiento del modelo. Va aparte de `Message` porque no es la
    /// respuesta: es el trabajo previo, y la vista lo pliega.
    #[serde(rename_all = "camelCase")]
    Reasoning { text: String, streaming: bool },

    /// Una herramienta, **de principio a fin en un solo item**.
    ///
    /// Antes eran dos eventos (`ToolCall` y `ToolResult`). Unidos, «qué hizo y
    /// cómo le fue» se responde mirando un objeto y no recorriendo el registro.
    #[serde(rename_all = "camelCase")]
    Tool {
        /// Nombre técnico, tal como lo llama el agente.
        name: String,
        /// Texto legible. Lo escribe el agente cuando puede (ACP lo manda en
        /// `title`); si no, lo arma el adaptador a partir de la entrada.
        title: String,
        tool_kind: ToolKind,
        status: ToolStatus,
        /// Entrada sin interpretar: cada herramienta tiene su propia forma y
        /// esta capa no puede conocerlas todas.
        input: Value,
        output: String,
        /// Archivos que toca, para poder seguirla desde la interfaz.
        locations: Vec<String>,
    },

    /// Trabajo delegado a otro agente por el proveedor.
    #[serde(rename_all = "camelCase")]
    Collab {
        /// Etiqueta corta: tipo de subagente o nombre de la herramienta.
        name: String,
        title: String,
        /// `explore` | `review` | `build` | `plan` | `task` | `other`.
        subagent_type: String,
        status: ToolStatus,
        /// Avance o resumen del resultado.
        summary: String,
        /// Turno padre, cuando el proveedor permite seguir la procedencia.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_turn_id: Option<String>,
        #[serde(default)]
        creation_source: String,
    },

    /// El plan que el agente propone, con el estado de cada paso.
    #[serde(rename_all = "camelCase")]
    Plan { entries: Vec<PlanEntry> },

    /// El agente pide permiso y **está detenido**.
    ///
    /// Es el único item que bloquea: nada avanza hasta que se conteste. Por eso
    /// la vista no lo trata como una línea más del registro.
    #[serde(rename_all = "camelCase")]
    Permission {
        tool: String,
        description: String,
        input: Value,
        status: PermissionStatus,
    },

    /// Aviso que no interrumpe: límites de uso, advertencias del backend, o un
    /// tipo de evento que el adaptador todavía no traduce.
    #[serde(rename_all = "camelCase")]
    Notice { text: String },
}

/// De dónde salió lo que escribió el usuario.
///
/// Es lo propio de Atic. El dictado, la captura y el portapapeles ya vivían en
/// la pill; esto es el rastro que dejan cuando se usan para hablarle a un
/// agente, y es lo que ninguna otra GUI puede copiar porque no tiene con qué.
///
/// El texto del origen (`via`, `file`) es solo para la conversación guardada.
/// Las rutas en `files` **sí** viajan al backend: se leen y se mandan como
/// bloques de imagen en el content del turno.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    /// Etiqueta corta y legible: «dictado», «captura», «portapapeles».
    ///
    /// Texto y no enum, por lo mismo que `title` en las herramientas: la lista
    /// de puentes va a crecer, y cada uno nuevo no debería obligar a tocar el
    /// modelo, los tres adaptadores y la vista para poder nombrarse.
    pub via: String,
    /// Nombre visible del adjunto, si lo hubo. La vista lo dibuja como tarjeta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Rutas absolutas de imágenes a embeber en el content del turno.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

/// Un item: identidad más contenido.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: ItemId,
    #[serde(flatten)]
    pub kind: ItemKind,
    /// Sólo lo llevan los mensajes del usuario que entraron por un puente.
    ///
    /// Vive en el item y no dentro de `ItemKind::Message` para no obligar a los
    /// otros dieciocho sitios que construyen mensajes a escribir `None`. Y
    /// `default` para que los hilos guardados antes de que esto existiera se
    /// sigan leyendo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

impl Item {
    pub fn new(id: impl Into<ItemId>, kind: ItemKind) -> Self {
        Self {
            id: id.into(),
            kind,
            origin: None,
        }
    }

    /// El mismo item, diciendo por dónde entró.
    pub fn con_origen(mut self, origin: Option<Origin>) -> Self {
        self.origin = origin;
        self
    }
}

/// Qué cambió de un item.
///
/// Campos opcionales y no un enum de variantes: un `tool_call_update` de ACP
/// puede traer estado, salida y ubicaciones **en el mismo mensaje**, y partirlo
/// en tres deltas obligaría a la vista a redibujar tres veces lo que cambió una.
/// Lo ausente no se toca.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<PlanEntry>>,
}

impl ItemPatch {
    pub fn tool_status(status: ToolStatus) -> Self {
        Self {
            status: serde_json::to_value(status).ok(),
            ..Default::default()
        }
    }
}

/// Cómo terminó un turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// Un nivel de esfuerzo que un modelo acepta.
///
/// Con descripción porque «medium» no le dice nada a nadie: los CLIs que lo
/// ofrecen mandan el texto que explica cuándo conviene, y perderlo dejaría un
/// selector de palabras sueltas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortOption {
    pub id: String,
    pub description: String,
}

/// Un modelo que el agente ofrece.
///
/// **Lo informa el backend**, no una lista escrita a mano en la vista. Esa era
/// la forma vieja y envejecía sola: cada versión del CLI suma o saca modelos, y
/// una constante en el frontend no se entera. Codex los da con
/// `model/list` —nombre, descripción y esfuerzos soportados—; Claude Code no
/// tiene esa llamada y su adaptador informa sus alias, que son estables a
/// propósito. Los agentes ACP pueden informarlos en `config_options` al crear
/// la sesión, o vía descubrimiento previo al arranque.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Lo que se le pasa al CLI (o id de grupo, en Cursor).
    pub id: String,
    /// Nombre legible.
    pub name: String,
    /// Para qué sirve. Vacío si el backend no lo dice.
    pub description: String,
    /// Niveles de esfuerzo que ESTE modelo acepta. Vacío = no se puede elegir.
    #[serde(default)]
    pub efforts: Vec<EffortOption>,
    /// El que trae puesto, si el backend lo dice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// Si el modelo tiene variantes rápidas (Cursor `*-fast`). Fast es un
    /// switch aparte del nivel de esfuerzo, al estilo Synara / T3 Code.
    #[serde(default)]
    pub supports_fast: bool,
}

/// Lo que cambia del hilo, no de un item suelto.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPatch {
    /// Los modelos que ofrece este agente. Llega una vez, al arrancar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<ModelInfo>>,
    /// Esfuerzo en curso, cuando el backend lo maneja.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Variante rápida activa (Cursor). Independiente del nivel de esfuerzo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast: Option<bool>,
    /// Id de sesión **del backend**, con el que se reanuda. Distinto del id
    /// local del hilo: el local existe desde antes de que el proceso arranque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Contexto consumido. Se informa **durante** el turno, no al final: lo que
    /// se quiere ver es cómo sube mientras el agente trabaja.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// Tamaño de la ventana de contexto, tal como lo informa el agente.
    ///
    /// Lo manda ACP en `usage_update.size`. Sin esto la vista tenía que
    /// adivinarlo con una constante —había un `CONTEXT_WINDOW = 1_000_000`
    /// escrito a mano—, que es un número distinto por modelo y por proveedor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<super::SlashCommand>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<super::McpServerState>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}

impl ThreadPatch {
    pub fn tokens(n: u64) -> Self {
        Self {
            tokens: Some(n),
            ..Default::default()
        }
    }
}

/// Qué cambió. Es lo único que viaja del backend a la interfaz.
///
/// Se serializa con `t` como discriminante y nombres punteados (`item.add`)
/// porque son los que se leen en las trazas y en el inspector del prototipo;
/// un `ItemAdd` en camelCase diría lo mismo y se buscaría peor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum AgentDelta {
    #[serde(rename = "turn.start", rename_all = "camelCase")]
    TurnStart { turn: TurnId },

    #[serde(rename = "item.add", rename_all = "camelCase")]
    ItemAdd { turn: TurnId, item: Item },

    /// Texto que se acumula en un item que ya existe.
    ///
    /// La diferencia con el modelo viejo es justamente esta: el trozo **sabe a
    /// qué pertenece**, así que no hace falta un campo de streaming paralelo al
    /// registro para saber dónde ponerlo.
    #[serde(rename = "item.chunk", rename_all = "camelCase")]
    ItemChunk { item: ItemId, text: String },

    #[serde(rename = "item.patch", rename_all = "camelCase")]
    ItemPatch { item: ItemId, patch: ItemPatch },

    #[serde(rename = "thread.patch", rename_all = "camelCase")]
    ThreadPatch { patch: ThreadPatch },

    #[serde(rename = "turn.end", rename_all = "camelCase")]
    TurnEnd {
        turn: TurnId,
        status: TurnStatus,
        cost_usd: Option<f64>,
    },

    /// El backend falló de una forma de la que no se puede seguir.
    #[serde(rename = "failed", rename_all = "camelCase")]
    Failed { message: String },
}

/// Un turno ya materializado. Vive en el store y en la vista, no en el cable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: TurnId,
    pub items: Vec<Item>,
    pub status: TurnStatus,
    pub cost_usd: Option<f64>,
}

/// La conversación completa con un agente, en una carpeta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
    pub provider_session: Option<String>,
    pub cwd: String,
    /// Host SSH; `None` = sesión local.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_host_id: Option<String>,
    pub model: String,
    pub mode: String,
    pub turns: Vec<Turn>,
    /// Momento de la última actividad, en segundos desde epoch. Es el orden en
    /// el que se listan los hilos guardados.
    pub updated_at: i64,
}

impl Thread {
    /// Aplica un delta. Es la MISMA operación que hace la vista.
    ///
    /// Vivir dos veces —una acá y otra en TypeScript— es a propósito y no un
    /// descuido: el store necesita reconstruir el hilo sin frontend, y la vista
    /// necesita aplicarlo sin ida y vuelta al backend. Lo que las mantiene
    /// juntas es que el delta es el mismo tipo, no una copia paralela.
    pub fn apply(&mut self, delta: &AgentDelta) {
        match delta {
            AgentDelta::TurnStart { turn } => self.turns.push(Turn {
                id: turn.clone(),
                items: Vec::new(),
                status: TurnStatus::Running,
                cost_usd: None,
            }),

            AgentDelta::ItemAdd { turn, item } => {
                // Un item cuyo turno no existe NO se descarta: se le abre uno.
                // Perderlo dejaría un hueco en la conversación guardada, y un
                // hueco es peor que un turno de más.
                if !self.turns.iter().any(|t| &t.id == turn) {
                    self.turns.push(Turn {
                        id: turn.clone(),
                        items: Vec::new(),
                        status: TurnStatus::Running,
                        cost_usd: None,
                    });
                }
                if let Some(t) = self.turns.iter_mut().find(|t| &t.id == turn) {
                    t.items.push(item.clone());
                }
            }

            AgentDelta::ItemChunk { item, text } => {
                if let Some(it) = self.item_mut(item) {
                    match &mut it.kind {
                        ItemKind::Message { text: t, .. } | ItemKind::Reasoning { text: t, .. } => {
                            t.push_str(text)
                        }
                        _ => {}
                    }
                }
            }

            AgentDelta::ItemPatch { item, patch } => {
                if let Some(it) = self.item_mut(item) {
                    apply_patch(&mut it.kind, patch);
                }
            }

            AgentDelta::ThreadPatch { patch } => {
                if let Some(v) = &patch.provider_session {
                    self.provider_session = Some(v.clone());
                }
                if let Some(v) = &patch.cwd {
                    self.cwd = v.clone();
                }
                if let Some(v) = &patch.model {
                    self.model = v.clone();
                }
                if let Some(v) = &patch.mode {
                    self.mode = v.clone();
                }
            }

            AgentDelta::TurnEnd {
                turn,
                status,
                cost_usd,
            } => {
                if let Some(t) = self.turns.iter_mut().find(|t| &t.id == turn) {
                    t.status = *status;
                    t.cost_usd = *cost_usd;
                }
            }

            AgentDelta::Failed { .. } => {
                if let Some(t) = self.turns.last_mut() {
                    if t.status == TurnStatus::Running {
                        t.status = TurnStatus::Failed;
                    }
                }
            }
        }
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        // De atrás para adelante: lo que se parchea es casi siempre lo último
        // que llegó, y un hilo largo tiene miles de items.
        self.turns
            .iter_mut()
            .rev()
            .find_map(|t| t.items.iter_mut().rev().find(|i| i.id == id))
    }
}

fn apply_patch(kind: &mut ItemKind, patch: &ItemPatch) {
    match kind {
        ItemKind::Message {
            text, streaming, ..
        }
        | ItemKind::Reasoning { text, streaming } => {
            // El texto del parche REEMPLAZA lo acumulado, no se suma: cuando el
            // backend manda el bloque completo al cerrar, esa versión es la
            // autoritativa y los trozos que fuimos juntando son una
            // aproximación que ya cumplió su función.
            if let Some(v) = &patch.text {
                *text = v.clone();
            }
            if let Some(v) = patch.streaming {
                *streaming = v;
            }
        }
        ItemKind::Tool {
            status,
            output,
            title,
            locations,
            ..
        } => {
            if let Some(v) = &patch.status {
                if let Ok(s) = serde_json::from_value::<ToolStatus>(v.clone()) {
                    *status = s;
                }
            }
            if let Some(v) = &patch.output {
                *output = v.clone();
            }
            if let Some(v) = &patch.title {
                *title = v.clone();
            }
            if let Some(v) = &patch.locations {
                *locations = v.clone();
            }
        }
        ItemKind::Collab {
            status,
            summary,
            title,
            subagent_type,
            ..
        } => {
            if let Some(v) = &patch.status {
                if let Ok(s) = serde_json::from_value::<ToolStatus>(v.clone()) {
                    *status = s;
                }
            }
            if let Some(v) = &patch.summary {
                *summary = v.clone();
            }
            if let Some(v) = &patch.title {
                *title = v.clone();
            }
            if let Some(v) = &patch.subagent_type {
                *subagent_type = v.clone();
            }
        }
        ItemKind::Permission { status, .. } => {
            if let Some(v) = &patch.status {
                if let Ok(s) = serde_json::from_value::<PermissionStatus>(v.clone()) {
                    *status = s;
                }
            }
        }
        ItemKind::Plan { entries } => {
            if let Some(v) = &patch.entries {
                *entries = v.clone();
            }
        }
        ItemKind::Notice { text } => {
            if let Some(v) = &patch.text {
                *text = v.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread() -> Thread {
        Thread {
            id: "h1".into(),
            backend_id: "claude-code".into(),
            backend_name: "Claude Code".into(),
            provider_session: None,
            cwd: String::new(),
            remote_host_id: None,
            model: String::new(),
            mode: String::new(),
            turns: Vec::new(),
            updated_at: 0,
        }
    }

    fn tool(id: &str) -> AgentDelta {
        AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                id,
                ItemKind::Tool {
                    name: "Read".into(),
                    title: "src/main.rs".into(),
                    tool_kind: ToolKind::Read,
                    status: ToolStatus::InProgress,
                    input: Value::Null,
                    output: String::new(),
                    locations: Vec::new(),
                },
            ),
        }
    }

    /// Lo que el registro plano no podía: la misma tarjeta cambia de estado.
    #[test]
    fn una_herramienta_es_un_solo_item_que_muta() {
        let mut h = thread();
        h.apply(&AgentDelta::TurnStart { turn: "t1".into() });
        h.apply(&tool("tu_1"));
        h.apply(&AgentDelta::ItemPatch {
            item: "tu_1".into(),
            patch: ItemPatch {
                output: Some("ok".into()),
                ..ItemPatch::tool_status(ToolStatus::Completed)
            },
        });

        assert_eq!(h.turns[0].items.len(), 1, "no debe duplicarse");
        let ItemKind::Tool { status, output, .. } = &h.turns[0].items[0].kind else {
            panic!("se esperaba una herramienta");
        };
        assert_eq!(*status, ToolStatus::Completed);
        assert_eq!(output, "ok");
    }

    #[test]
    fn un_subagente_se_actualiza_sin_duplicarse() {
        let mut h = thread();
        h.apply(&AgentDelta::TurnStart { turn: "t1".into() });
        h.apply(&AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                "a1",
                ItemKind::Collab {
                    name: "Task".into(),
                    title: "Revisar cambios".into(),
                    subagent_type: "review".into(),
                    status: ToolStatus::InProgress,
                    summary: String::new(),
                    parent_turn_id: Some("t1".into()),
                    creation_source: "provider_native".into(),
                },
            ),
        });
        h.apply(&AgentDelta::ItemPatch {
            item: "a1".into(),
            patch: ItemPatch {
                status: serde_json::to_value(ToolStatus::Completed).ok(),
                summary: Some("No encontró problemas.".into()),
                title: Some("Revisión terminada".into()),
                ..Default::default()
            },
        });

        let ItemKind::Collab {
            status,
            summary,
            title,
            subagent_type,
            ..
        } = &h.turns[0].items[0].kind
        else {
            panic!("se esperaba colaboración");
        };
        assert_eq!(*status, ToolStatus::Completed);
        assert_eq!(summary, "No encontró problemas.");
        assert_eq!(title, "Revisión terminada");
        assert_eq!(subagent_type, "review");
    }

    /// El trozo sabe a qué mensaje pertenece: dos bloques a la vez no se pisan.
    #[test]
    fn dos_bloques_transmiten_sin_mezclarse() {
        let mut h = thread();
        h.apply(&AgentDelta::TurnStart { turn: "t1".into() });
        for (id, kind) in [
            (
                "r1",
                ItemKind::Reasoning {
                    text: String::new(),
                    streaming: true,
                },
            ),
            (
                "m1",
                ItemKind::Message {
                    role: Role::Assistant,
                    text: String::new(),
                    streaming: true,
                },
            ),
        ] {
            h.apply(&AgentDelta::ItemAdd {
                turn: "t1".into(),
                item: Item::new(id, kind),
            });
        }
        h.apply(&AgentDelta::ItemChunk {
            item: "r1".into(),
            text: "pen".into(),
        });
        h.apply(&AgentDelta::ItemChunk {
            item: "m1".into(),
            text: "hola".into(),
        });
        h.apply(&AgentDelta::ItemChunk {
            item: "r1".into(),
            text: "sando".into(),
        });

        let ItemKind::Reasoning { text: r, .. } = &h.turns[0].items[0].kind else {
            panic!("se esperaba razonamiento");
        };
        let ItemKind::Message { text: m, .. } = &h.turns[0].items[1].kind else {
            panic!("se esperaba mensaje");
        };
        assert_eq!(r, "pensando");
        assert_eq!(m, "hola");
    }

    /// Al cerrar el bloque, el texto completo pisa lo acumulado en vez de
    /// sumarse. Sin esto la respuesta saldría escrita dos veces.
    #[test]
    fn el_texto_completo_reemplaza_los_trozos() {
        let mut h = thread();
        h.apply(&AgentDelta::TurnStart { turn: "t1".into() });
        h.apply(&AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                "m1",
                ItemKind::Message {
                    role: Role::Assistant,
                    text: String::new(),
                    streaming: true,
                },
            ),
        });
        h.apply(&AgentDelta::ItemChunk {
            item: "m1".into(),
            text: "hol".into(),
        });
        h.apply(&AgentDelta::ItemPatch {
            item: "m1".into(),
            patch: ItemPatch {
                text: Some("hola mundo".into()),
                streaming: Some(false),
                ..Default::default()
            },
        });

        let ItemKind::Message {
            text, streaming, ..
        } = &h.turns[0].items[0].kind
        else {
            panic!("se esperaba mensaje");
        };
        assert_eq!(text, "hola mundo");
        assert!(!streaming);
    }

    /// Un item sin turno abierto no se pierde: se le abre uno.
    #[test]
    fn un_item_huerfano_abre_su_turno() {
        let mut h = thread();
        h.apply(&tool("tu_1"));
        assert_eq!(h.turns.len(), 1);
        assert_eq!(h.turns[0].items.len(), 1);
    }

    /// El discriminante que ve la interfaz es el que se lee en las trazas.
    #[test]
    fn el_delta_se_serializa_con_nombres_punteados() {
        let d = AgentDelta::ItemChunk {
            item: "m1".into(),
            text: "x".into(),
        };
        let j = serde_json::to_value(&d).unwrap();
        assert_eq!(j["t"], "item.chunk");
        assert_eq!(j["item"], "m1");
    }

    /// Lo ausente en el parche no se toca.
    #[test]
    fn el_parche_solo_cambia_lo_que_trae() {
        let mut h = thread();
        h.apply(&AgentDelta::TurnStart { turn: "t1".into() });
        h.apply(&tool("tu_1"));
        h.apply(&AgentDelta::ItemPatch {
            item: "tu_1".into(),
            patch: ItemPatch {
                locations: Some(vec!["src/main.rs".into()]),
                ..Default::default()
            },
        });

        let ItemKind::Tool {
            status,
            title,
            locations,
            ..
        } = &h.turns[0].items[0].kind
        else {
            panic!("se esperaba una herramienta");
        };
        assert_eq!(*status, ToolStatus::InProgress, "el estado no se tocó");
        assert_eq!(title, "src/main.rs", "el título no se tocó");
        assert_eq!(locations.len(), 1);
    }
}
