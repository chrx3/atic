//! Las siete herramientas `atic_editor_*`: el agente ve y edita la grabación
//! abierta en el editor de la pill.
//!
//! La pill expone un puente TCP en localhost (ver `docs/EDITOR_AGENTE.md`):
//! `editor-bridge.json` trae puerto y token; cada pedido es una conexión, una
//! línea JSON de ida y una de vuelta. El archivo se lee en CADA llamada porque
//! la pill lo crea al abrir el primer editor y lo borra al cerrar el último.
//! Como en `tools.rs`, los fallos salen como error de HERRAMIENTA (`isError`),
//! nunca como error de protocolo.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use directories::ProjectDirs;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::schemars;
use rmcp::{tool, tool_router, ErrorData as McpError};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::tools::AticServer;

/// Lo que lee el modelo cuando no hay editor al otro lado.
pub const EDITOR_AUSENTE: &str = "Abre una grabación en el editor de Atic para poder editarla.";

/// Tiempo total de un pedido (los hosts cortan las herramientas a los 60 s).
const TIMEOUT_TOTAL: Duration = Duration::from_secs(60);
const TIMEOUT_CONEXION: Duration = Duration::from_secs(2);
/// Tope de una respuesta: ocho cuadros en base64 caben de sobra.
const MAX_RESPUESTA: u64 = 64 * 1024 * 1024;

/// Ruta de `editor-bridge.json`. En tests, `ATIC_EDITOR_BRIDGE_JSON` apunta a uno de mentira.
fn puente_json_path() -> Option<std::path::PathBuf> {
    if let Some(var) = std::env::var_os("ATIC_EDITOR_BRIDGE_JSON") {
        let p = std::path::PathBuf::from(var);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    ProjectDirs::from("com", "ciat", "atic").map(|d| d.data_dir().join("editor-bridge.json"))
}

/// Una imagen de la respuesta, con el texto que la acompaña (su tiempo).
#[derive(Debug, Clone, Deserialize)]
pub struct Imagen {
    pub mime: String,
    pub data: String,
    #[serde(default)]
    pub caption: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorPuente {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct RespuestaPuente {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    images: Option<Vec<Imagen>>,
    #[serde(default)]
    error: Option<ErrorPuente>,
}

/// Respuesta buena del puente.
#[derive(Debug, Clone)]
pub struct Resultado {
    pub data: Value,
    pub images: Vec<Imagen>,
}

#[derive(Debug)]
pub enum EditorFallo {
    /// Sin archivo del puente o conexión rechazada: Atic o el editor no están abiertos.
    Ausente,
    /// La pill contestó `ok: false`.
    Puente { code: String, message: String },
    /// Todo lo demás (tiempo agotado, respuesta ilegible…).
    Otro(String),
}

impl EditorFallo {
    pub fn mensaje(&self) -> String {
        match self {
            Self::Ausente => EDITOR_AUSENTE.to_string(),
            Self::Puente { code, message } => {
                let base = if message.trim().is_empty() {
                    "El editor de Atic rechazó el pedido.".to_string()
                } else {
                    message.trim().to_string()
                };
                match code.as_str() {
                    "" => base,
                    "no_editor" => format!("{EDITOR_AUSENTE} ({base})"),
                    "bad_token" => format!(
                        "{base} [bad_token] Vuelve a abrir el editor de Atic para renovar el acceso."
                    ),
                    c => format!("{base} [{c}]"),
                }
            }
            Self::Otro(m) => m.clone(),
        }
    }
}

/// Llama al puente con el archivo de descubrimiento de siempre.
pub async fn llamar(tool: &str, args: Value) -> Result<Resultado, EditorFallo> {
    let ruta = puente_json_path().ok_or(EditorFallo::Ausente)?;
    llamar_en(&ruta, tool, args).await
}

/// Igual que [`llamar`], con el `editor-bridge.json` en `ruta`.
pub async fn llamar_en(
    ruta: &std::path::Path,
    tool: &str,
    args: Value,
) -> Result<Resultado, EditorFallo> {
    static ID: AtomicU64 = AtomicU64::new(1);
    let texto = std::fs::read_to_string(ruta).map_err(|_| EditorFallo::Ausente)?;
    let json: Value = serde_json::from_str(&texto).map_err(|_| EditorFallo::Ausente)?;
    let port = json
        .get("port")
        .and_then(|p| p.as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .ok_or(EditorFallo::Ausente)?;
    let token = json
        .get("token")
        .and_then(|t| t.as_str())
        .ok_or(EditorFallo::Ausente)?
        .to_string();
    let id = ID.fetch_add(1, Ordering::Relaxed);
    let pedido = json!({ "token": token, "id": id, "tool": tool, "args": args });

    let conversar = async {
        let conexion = tokio::time::timeout(
            TIMEOUT_CONEXION,
            TcpStream::connect(("127.0.0.1", port)),
        )
        .await
        .map_err(|_| EditorFallo::Ausente)?
        .map_err(|_| EditorFallo::Ausente)?;
        let (lectura, mut escritura) = conexion.into_split();
        let mut linea = pedido.to_string();
        linea.push('\n');
        escritura
            .write_all(linea.as_bytes())
            .await
            .map_err(|e| EditorFallo::Otro(format!("No pude hablarle al editor de Atic: {e}.")))?;
        escritura.flush().await.ok();
        let mut lector = BufReader::new(lectura.take(MAX_RESPUESTA));
        let mut respuesta = String::new();
        let leidos = lector
            .read_line(&mut respuesta)
            .await
            .map_err(|e| EditorFallo::Otro(format!("El editor de Atic cortó la respuesta: {e}.")))?;
        if leidos == 0 || respuesta.trim().is_empty() {
            return Err(EditorFallo::Otro(
                "El editor de Atic cerró la conexión sin responder. Vuelve a intentarlo.".into(),
            ));
        }
        Ok::<String, EditorFallo>(respuesta)
    };
    let respuesta = tokio::time::timeout(TIMEOUT_TOTAL, conversar)
        .await
        .map_err(|_| {
            EditorFallo::Otro(
                "El editor de Atic no respondió en 60 s. Consulta el estado con atic_editor_state y vuelve a intentarlo."
                    .into(),
            )
        })??;

    let r: RespuestaPuente = serde_json::from_str(respuesta.trim()).map_err(|e| {
        EditorFallo::Otro(format!("La respuesta del editor de Atic no se entiende: {e}."))
    })?;
    if !r.ok {
        let e = r.error.unwrap_or(ErrorPuente {
            code: "internal".into(),
            message: String::new(),
        });
        return Err(EditorFallo::Puente {
            code: e.code,
            message: e.message,
        });
    }
    Ok(Resultado {
        data: r.data.unwrap_or_else(|| json!({})),
        images: r.images.unwrap_or_default(),
    })
}

/// De la respuesta del puente al resultado MCP: `data` como JSON compacto y,
/// por cada imagen, el bloque imagen seguido de su `caption` como texto.
pub fn a_resultado(r: Result<Resultado, EditorFallo>) -> CallToolResult {
    match r {
        Err(f) => CallToolResult::error(vec![ContentBlock::text(f.mensaje())]),
        Ok(r) => {
            let mut bloques = vec![ContentBlock::text(r.data.to_string())];
            for img in r.images {
                bloques.push(ContentBlock::image(img.data, img.mime));
                if let Some(c) = img.caption.filter(|c| !c.trim().is_empty()) {
                    bloques.push(ContentBlock::text(c));
                }
            }
            CallToolResult::success(bloques)
        }
    }
}

/// Pasa los args tipados al pedido del puente, sin los campos vacíos.
fn args_json<T: serde::Serialize>(a: &T) -> Value {
    let mut v = serde_json::to_value(a).unwrap_or_else(|_| json!({}));
    if let Value::Object(m) = &mut v {
        m.retain(|_, x| !x.is_null());
    }
    v
}

async fn pedir<T: serde::Serialize>(tool: &str, a: &T) -> Result<CallToolResult, McpError> {
    Ok(a_resultado(llamar(tool, args_json(a)).await))
}

// --- Parámetros ---

/// Solo el editor al que apuntar.
#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct SoloVideo {
    /// Ruta de la grabación (la que da atic_editor_list). Sin ella, el editor que tuvo el foco al último.
    #[serde(default)]
    pub video: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct VerCuadros {
    /// Ruta de la grabación. Sin ella, el editor que tuvo el foco al último.
    #[serde(default)]
    pub video: Option<String>,
    /// Instantes a mirar, en segundos del video original (hasta 8). Alternativa a start/end/count.
    #[serde(default)]
    pub times: Option<Vec<f64>>,
    /// Inicio del tramo a repartir en cuadros (s del original), junto con `end` y `count`.
    #[serde(default)]
    pub start: Option<f64>,
    /// Final del tramo (s del original).
    #[serde(default)]
    pub end: Option<f64>,
    /// Cuántos cuadros repartir entre start y end (hasta 8).
    #[serde(default)]
    pub count: Option<u32>,
    /// Alto de cada imagen en px (240–720, por defecto 480).
    #[serde(default)]
    pub height: Option<u32>,
    /// true: el cuadro como queda con zoom, fondo y textos (por defecto false: el video crudo).
    #[serde(default)]
    pub composed: Option<bool>,
}

#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct Editar {
    /// Ruta de la grabación. Sin ella, el editor que tuvo el foco al último.
    #[serde(default)]
    pub video: Option<String>,
    /// Operaciones a aplicar, en orden; cada una es un objeto con `op` (catálogo en la descripción de la herramienta).
    pub ops: Vec<serde_json::Value>,
    /// Nombre corto del paso de deshacer (por defecto «Cambios del asistente»).
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct Exportar {
    /// Ruta de la grabación. Sin ella, el editor que tuvo el foco al último.
    #[serde(default)]
    pub video: Option<String>,
    /// Segundos a esperar a que termine (hasta 40). Si no alcanza vuelve con status running y progress: consulta de nuevo.
    #[serde(default)]
    pub wait: Option<u64>,
}

// --- Herramientas ---

#[tool_router(router = editor_router, vis = "pub(crate)")]
impl AticServer {
    #[tool(
        name = "atic_editor_list",
        description = "Lista las grabaciones abiertas en el editor de Atic: video (ruta), title, duration (s) y focused. Llámala primero para saber qué se puede editar; el resto de atic_editor_* acepta `video` con una de esas rutas y, sin él, usa el editor que tuvo el foco al último."
    )]
    async fn atic_editor_list(
        &self,
        params: Parameters<SoloVideo>,
    ) -> Result<CallToolResult, McpError> {
        pedir("list", &params.0).await
    }

    #[tool(
        name = "atic_editor_state",
        description = "Estado del proyecto en el editor: duración, tamaño, cabezal, selección y qué contiene (recorte, cortes, zooms, textos, velocidades, difuminados, estilo, cursor, exportación) con ÍNDICES —los que usan las operaciones update y remove de atic_editor_edit—, el tiempo resultante tras cortes y velocidades, can_undo/can_redo y el estado de la exportación y de la transcripción. Todos los tiempos son segundos del video original. Léelo antes de editar y después para comprobar el resultado."
    )]
    async fn atic_editor_state(
        &self,
        params: Parameters<SoloVideo>,
    ) -> Result<CallToolResult, McpError> {
        pedir("state", &params.0).await
    }

    #[tool(
        name = "atic_editor_frames",
        description = "VER el video: devuelve cuadros como imágenes, cada una con su tiempo. Pide `times` (hasta 8 instantes) o bien `start`, `end` y `count` (hasta 8 cuadros repartidos en ese tramo). `height` en px (240–720, por defecto 480). Con `composed: true` el cuadro sale como queda con zoom, fondo y textos; por defecto es el video crudo. Los tiempos son segundos del video original. Úsala antes de decidir cortes, zooms o textos, y después de editar para comprobar."
    )]
    async fn atic_editor_frames(
        &self,
        params: Parameters<VerCuadros>,
    ) -> Result<CallToolResult, McpError> {
        pedir("frames", &params.0).await
    }

    #[tool(
        name = "atic_editor_transcript",
        description = "Lo que se dice en la grabación, con tiempos: segments [{start, end, text}] en segundos del video original. La primera vez lanza la transcripción local y vuelve con status running y progress: vuelve a llamarla hasta que traiga los segmentos. Sirve para cortar muletillas, poner títulos o subtítulos y ubicar un tema sin ver todo el video."
    )]
    async fn atic_editor_transcript(
        &self,
        params: Parameters<SoloVideo>,
    ) -> Result<CallToolResult, McpError> {
        pedir("transcript", &params.0).await
    }

    #[tool(
        name = "atic_editor_activity",
        description = "Lo que pasó en pantalla durante la grabación: clics (t en segundos del original; x, y de 0 a 1), rachas de escritura, tramos quietos, silencios del audio y picos de volumen. Úsala para decidir dónde acercar con zoom o qué tramos cortar o acelerar sin mirar todo el video."
    )]
    async fn atic_editor_activity(
        &self,
        params: Parameters<SoloVideo>,
    ) -> Result<CallToolResult, McpError> {
        pedir("activity", &params.0).await
    }

    // `description` solo acepta literales: el catálogo entra por `#[doc]`, que
    // la macro lee como descripción (con `include_str!`).
    #[doc = include_str!("editor_ops.md")]
    #[tool(name = "atic_editor_edit")]
    async fn atic_editor_edit(
        &self,
        params: Parameters<Editar>,
    ) -> Result<CallToolResult, McpError> {
        pedir("edit", &params.0).await
    }

    #[tool(
        name = "atic_editor_export",
        description = "Exporta la grabación con la configuración actual del proyecto. Con `wait` (s, hasta 40) espera a que termine; si no alcanza vuelve con status running y progress, y se consulta de nuevo con otra llamada. Al terminar trae status done y la ruta del archivo; si falla, status failed. Exporta cuando el usuario lo pida o cuando la edición esté terminada y revisada."
    )]
    async fn atic_editor_export(
        &self,
        params: Parameters<Exportar>,
    ) -> Result<CallToolResult, McpError> {
        pedir("export", &params.0).await
    }
}
