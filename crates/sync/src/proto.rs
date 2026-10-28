//! Mensajes del canal y su encuadre.
//!
//! Una sola corriente bidireccional por conexión. Cada mensaje es JSON con un
//! prefijo de 4 bytes (big endian) con su largo. JSON y no binario: el volumen
//! es mínimo (una foto de agentes cada tanto) y se puede leer al depurar.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Versión del protocolo en el ALPN: un cambio incompatible cambia el ALPN.
pub const ALPN: &[u8] = b"atic/sync/1";

/// Tope por mensaje. Una foto de agentes no llega a 100 KB; lo grande es una
/// imagen del portapapeles pedida a propósito (ver [`MAX_IMAGE_BYTES`]).
const MAX_FRAME: u32 = 8 << 20;

/// Tope de una imagen completa del portapapeles (base64 incluido cabe en el frame).
pub const MAX_IMAGE_BYTES: usize = 5 << 20;
/// Tope de una miniatura del historial.
pub const MAX_THUMB_BYTES: usize = 48 * 1024;

/// Lo que el escritorio sabe de un agente. Espeja `AgentPresence` del desktop
/// más el permiso pendiente, que solo traen las sesiones que Atic maneja.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
    /// Último segmento del `cwd`.
    pub project: String,
    pub status: AgentStatus,
    pub activity: Option<AgentActivity>,
    pub preview: Option<String>,
    /// Solo si el celular puede contestarlo.
    pub permission: Option<PermissionAsk>,
    /// El título de la conversación (Atic Code), si lo tiene. Un PC viejo no lo manda.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Terminó un turno y nadie lo ha mirado: va en «Por revisar» hasta
    /// [`AgentAction::Accept`] o hasta que se mire en el PC.
    #[serde(default)]
    pub unread: bool,
    /// El modelo que usa («claude-opus-4-1»…), si se sabe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// La última vez que hizo algo, en milisegundos desde 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_ms: Option<i64>,
    /// Vive en una conversación de Atic Code: recibe imágenes y se puede
    /// detener ([`AgentAction::Stop`]). Uno de consola solo recibe texto.
    #[serde(default)]
    pub chat: bool,
}

/// Lo que el celular le puede hacer a un agente además de escribirle ([`feature::AGENTS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum AgentAction {
    /// Lo que terminó queda revisado, como «Aceptar» en la pill.
    Accept,
    /// Abrir su conversación en el PC, al frente.
    Open,
    /// Detener el turno en curso (pide además [`feature::PROMPT`]).
    Stop,
}

/// Una imagen que va con un mensaje para un agente ([`ToDesktop::Prompt`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PromptImage {
    /// `image/jpeg`, `image/png`, `image/webp` o `image/gif`.
    pub mime: String,
    #[serde(with = "b64")]
    pub data: Vec<u8>,
}

/// Qué es cada parte de lo que se mira en vivo ([`ToPhone::AgentEntries`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// Lo que escribió el usuario (texto plano), con sus imágenes.
    User,
    /// La respuesta del agente, en Markdown.
    Assistant,
    /// Una herramienta que usó: una línea corta («Editando main.rs»).
    Tool,
}

/// Una parte de la conversación que se mira en vivo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AgentEntry {
    pub kind: EntryKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<EntryImage>,
}

/// Una imagen de un mensaje, achicada para el celular (JPEG de unos 720 px).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct EntryImage {
    pub mime: String,
    #[serde(with = "b64")]
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Imágenes por mensaje para un agente.
pub const MAX_PROMPT_IMAGES: usize = 4;
/// Lo que pesan todas juntas (base64 incluido cabe en el frame).
pub const MAX_PROMPT_IMAGE_BYTES: usize = 5 << 20;

/// Si las imágenes de un mensaje caben: cuántas, cuánto pesan y de qué tipo.
pub fn prompt_images_ok(images: &[PromptImage]) -> bool {
    images.len() <= MAX_PROMPT_IMAGES
        && images.iter().map(|i| i.data.len()).sum::<usize>() <= MAX_PROMPT_IMAGE_BYTES
        && images.iter().all(|i| !i.data.is_empty() && matches!(i.mime.as_str(), "image/jpeg" | "image/png" | "image/webp" | "image/gif"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Working,
    Waiting,
    Ready,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    Thinking,
    Writing,
    Editing,
    Reading,
    Searching,
    Running,
    Delegating,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct AgentActivity {
    pub kind: ActivityKind,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PermissionAsk {
    pub id: String,
    pub title: String,
    pub detail: Option<String>,
    pub can_allow_always: bool,
    /// Si no está vacío, el permiso es una pregunta del agente
    /// (`AskUserQuestion`): se contesta con [`ToDesktop::Answer`], no con
    /// permitir o rechazar.
    #[serde(default)]
    pub questions: Vec<AgentQuestion>,
}

/// Una pregunta con opciones que hace el agente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct AgentQuestion {
    pub question: String,
    /// Un rótulo corto («BD», «Tests»).
    pub header: String,
    pub options: Vec<QuestionOption>,
    /// Se puede elegir más de una opción.
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// Igual que `PermissionDecision` en `agents/mod.rs` del desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Allow,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum ClipKind {
    #[default]
    Text,
    Image,
}

/// Algo que se copió en un equipo y viaja al otro.
///
/// Las imágenes del historial viajan como miniatura; la imagen entera se pide
/// aparte ([`ToDesktop::FetchImage`]) solo si el usuario la toca.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct ClipItem {
    pub id: String,
    /// Vacío en las imágenes.
    pub text: String,
    #[serde(default)]
    pub kind: ClipKind,
    /// Miniatura JPEG de una imagen.
    #[serde(default, with = "b64_opt", skip_serializing_if = "Option::is_none")]
    pub thumb: Option<Vec<u8>>,
    /// Tamaño de la imagen original, en píxeles.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Quién lo copió, para mostrarlo ("PC de Carlos", "Galaxy S24").
    pub source_name: String,
    /// Epoch en milisegundos, según el reloj de quien lo copió.
    pub copied_at_ms: i64,
    /// Fijado en el historial del PC.
    #[serde(default)]
    pub pinned: bool,
}

/// Tope de cada texto del historial que viaja al celular: el historial entero
/// va en un mensaje y no puede pasar el tope de [`MAX_FRAME`].
pub const MAX_HISTORY_ITEM_BYTES: usize = 16 * 1024;
/// Cuántos textos del historial viajan.
pub const MAX_HISTORY_ITEMS: usize = 30;

/// Lo que pasa en el PC fuera de los agentes: lo que suena y si está grabando.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PcState {
    pub media: Option<MediaState>,
    pub recording: Option<RecordingState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    /// De qué app viene, legible («Spotify»).
    pub app: String,
    pub playing: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_prev: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct RecordingState {
    /// Epoch en milisegundos: el celular cuenta el cronómetro solo.
    pub started_at_ms: i64,
}

/// Lo que el celular le puede pedir al PC fuera de los permisos.
///
/// No hay «empezar a grabar»: prender el micrófono del PC a distancia es otra
/// conversación. Detener sí, que es lo que se olvida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum PcCommand {
    MediaToggle,
    MediaNext,
    MediaPrev,
    StopRecording,
    // Desde acá, solo a un PC con [`feature::CONTROL`].
    VolumeUp,
    VolumeDown,
    Mute,
    LockScreen,
    SlideNext,
    SlidePrev,
    // Desde acá, solo a un PC con [`feature::REMOTE`].
    /// Suspender el PC.
    Sleep,
}

impl PcCommand {
    /// La función que tiene que anunciar el PC para recibirlo: uno viejo no
    /// conoce el comando y cortaría la sesión. `None`: lo conocen todos.
    pub fn feature(self) -> Option<&'static str> {
        match self {
            Self::MediaToggle | Self::MediaNext | Self::MediaPrev | Self::StopRecording => None,
            Self::VolumeUp | Self::VolumeDown | Self::Mute | Self::LockScreen | Self::SlideNext | Self::SlidePrev => {
                Some(feature::CONTROL)
            }
            Self::Sleep => Some(feature::REMOTE),
        }
    }
}

/// El celular como panel táctil y teclado del PC ([`feature::REMOTE`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InputEvent {
    /// Mover el puntero, en puntos de pantalla.
    Move { dx: f32, dy: f32 },
    /// Desplazar, en puntos; `dy` positivo baja por la página.
    Scroll { dx: f32, dy: f32 },
    Click { button: MouseButton },
    /// Apretar o soltar un botón, para arrastrar.
    Button { button: MouseButton, down: bool },
    /// Texto tal cual, como si se tecleara.
    Text { text: String },
    Key { key: PcKey },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Teclas sueltas y atajos que cambian según el sistema: cada PC los traduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum PcKey {
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    Up,
    Down,
    Left,
    Right,
    /// Cambiar a la app anterior (⌘Tab, Alt+Tab).
    AppSwitch,
    /// Pantalla completa de la ventana del frente (⌃⌘F, F11).
    Fullscreen,
    /// Ver el escritorio (F11 en Mac, Win+D).
    ShowDesktop,
    /// Pantalla en negro en una presentación (B en PowerPoint, Keynote y Slides).
    BlackScreen,
}

/// Tope de un [`InputEvent::Text`]: lo que se teclea de una vez.
pub const MAX_INPUT_TEXT: usize = 1024;

/// Lo que cada lado anuncia al saludar ([`ToDesktop::Hello`], [`ToPhone::Welcome`]).
///
/// Un mensaje nuevo solo se manda a quien anunció su función: un equipo viejo
/// no conoce el mensaje, falla al leerlo y corta la sesión. Así cada función
/// se prende sola cuando los dos lados la tienen.
pub mod feature {
    /// Archivos en los dos sentidos, por una corriente aparte ([`super::FileHeader`]).
    pub const FILES: &str = "files";
    /// Las notificaciones del celular en el PC, y contestarlas desde ahí.
    pub const NOTICES: &str = "notices";
    /// Las llamadas del celular en el PC: quién llama, contestar o colgar.
    pub const CALLS: &str = "calls";
    /// Escribirle a un agente del PC desde el celular.
    pub const PROMPT: &str = "prompt";
    /// Ver en vivo lo que escribe un agente.
    pub const WATCH: &str = "watch";
    /// Volumen, bloquear y diapositivas ([`super::PcCommand`] más allá de la música).
    pub const CONTROL: &str = "control";
    /// Panel táctil, teclado, atajos y suspender ([`super::InputEvent`], [`super::PcCommand::Sleep`]).
    pub const REMOTE: &str = "remote";
    /// El PC hace sonar el celular para encontrarlo ([`super::ToPhone::Ring`]).
    pub const FIND: &str = "find";
    /// Abrir un enlace en el otro equipo ([`super::ToDesktop::OpenUrl`], [`super::ToPhone::OpenUrl`]).
    pub const LINKS: &str = "links";
    /// El PC pide una foto con la cámara del celular ([`super::ToPhone::TakePhoto`]).
    pub const PHOTO: &str = "photo";
    /// Tarjetas con título, «por revisar» y modelo, las acciones de
    /// [`super::AgentAction`] y mensajes con imágenes ([`super::ToDesktop::Prompt`]).
    pub const AGENTS: &str = "agents";
    /// Los permisos cambian sin reconectar ([`super::ToPhone::Features`]) y el
    /// celular puede pedir los que le faltan ([`super::ToDesktop::RequestAccess`]).
    pub const ACCESS: &str = "access";
    /// La pantalla del celular en el PC, por la conexión de video aparte
    /// ([`crate::video`], ALPN `atic/video/1`).
    pub const SCREEN: &str = "screen";

    /// Todo lo que sabe hacer este lado (el del celular las tiene todas).
    pub fn all() -> Vec<String> {
        [FILES, NOTICES, CALLS, PROMPT, WATCH, CONTROL, REMOTE, FIND, LINKS, PHOTO, ACCESS, AGENTS, SCREEN].iter().map(|f| f.to_string()).collect()
    }

    /// Lo que el celular puede pedir con [`super::ToDesktop::RequestAccess`]:
    /// lo que el PC deja apagado de entrada porque teclea en él.
    pub fn requestable(f: &str) -> bool {
        matches!(f, PROMPT | CONTROL | REMOTE)
    }
}

/// Una notificación del celular, para mostrarla en el PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct PhoneNotice {
    /// La clave de Android: con ella se contesta o se descarta.
    pub key: String,
    /// El paquete de la app («com.whatsapp»).
    pub app: String,
    pub app_name: String,
    pub title: String,
    pub text: String,
    pub posted_at_ms: i64,
    /// La app deja contestar desde la notificación (respuesta rápida).
    pub can_reply: bool,
    /// Ícono chico en PNG.
    #[serde(default, with = "b64_opt", skip_serializing_if = "Option::is_none")]
    pub icon: Option<Vec<u8>>,
}

/// Una llamada en el celular, de cualquier app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct CallState {
    /// Quién llama o con quién se habla.
    pub name: String,
    /// Sonando, todavía sin contestar.
    pub incoming: bool,
    /// Desde cuándo se habla (epoch en ms); `None` sonando.
    pub since_ms: Option<i64>,
    pub can_answer: bool,
    pub can_hang_up: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum CallAction {
    Answer,
    HangUp,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    #[default]
    File,
    /// Una reunión: el audio o su transcripción; `meta` trae el JSON de la reunión.
    Meeting,
}

/// Lo primero de una corriente de archivo; detrás vienen `size` bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct FileHeader {
    pub id: String,
    /// Nombre para guardarlo (sin carpetas: quien recibe lo limpia igual).
    pub name: String,
    pub mime: String,
    pub size: u64,
    #[serde(default)]
    pub kind: FileKind,
    #[serde(default)]
    pub meta: Option<String>,
}

/// Tope de un archivo. Va por su propia corriente, sin pasar por la memoria.
pub const MAX_FILE_BYTES: u64 = 2 << 30;

/// El nombre de un archivo recibido, sin carpetas ni caracteres raros: lo
/// manda el otro lado y no puede escribir fuera de la carpeta de entrada.
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && !is_format_char(*c) && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    // Windows no deja terminar en punto o espacio (los come y el nombre cambia).
    let clean = clean.trim().trim_start_matches('.').trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() {
        return "archivo".to_string();
    }
    let clean: String = clean.chars().take(120).collect();
    // CON, NUL.txt, COM1… son dispositivos en Windows, no archivos.
    let stem = clean.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("_{clean}")
    } else {
        clean
    }
}

/// Caracteres invisibles que cambian cómo se lee un nombre: `foo\u{202E}gpj.exe`
/// se ve como `fooexe.jpg`.
fn is_format_char(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// Tope de un enlace para abrir en el otro equipo.
pub const MAX_URL_BYTES: usize = 8 * 1024;

/// Si `url` es un enlace web que se puede abrir en el otro equipo: solo
/// `http` y `https`, sin espacios ni caracteres de control. `file:`,
/// `javascript:` o el esquema de una app no pasan: el otro lado lo abriría
/// sin preguntar.
pub fn is_web_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return false;
    };
    url.len() <= MAX_URL_BYTES
        && !rest.is_empty()
        && !rest.starts_with('/')
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || is_format_char(c))
}

/// Tope de un texto copiado. Más que esto no es un «copiar y pegar» y no vale
/// la pena empujarlo sin que nadie lo pida.
pub const MAX_CLIP_BYTES: usize = 64 * 1024;

/// Del celular al escritorio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToDesktop {
    /// Primer mensaje de un equipo nuevo: el token del ticket que escaneó.
    #[serde(rename_all = "camelCase")]
    Pair {
        token: String,
        device_name: String,
        #[serde(default)]
        features: Vec<String>,
    },
    /// Primer mensaje de un equipo ya pareado.
    #[serde(rename_all = "camelCase")]
    Hello {
        device_name: String,
        #[serde(default)]
        features: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Decide { agent_id: String, permission_id: String, decision: Decision },
    /// Respuestas a una pregunta del agente: una por pregunta, en orden (las
    /// opciones elegidas o lo que se escribió). Una vacía queda sin contestar.
    Answer { agent_id: String, permission_id: String, answers: Vec<String> },
    /// Lo que el usuario mandó desde el celular al portapapeles del PC.
    Clip { item: ClipItem },
    Command { command: PcCommand },
    /// Pide la imagen entera de un ítem del historial.
    FetchImage { id: String },
    /// Una imagen para el portapapeles del PC (PNG o JPEG).
    Image {
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    /// El historial que guarda el celular, al conectarse. El celular es el
    /// centro: lo que el PC no tiene se suma a su historial (sin tocar su
    /// portapapeles) y lo borrado en otro lado se borra acá. Las imágenes
    /// van sin bytes; el PC pide las que le faltan con [`ToPhone::ClipNeed`].
    ClipSync { items: Vec<ClipItem>, deleted: Vec<String> },
    /// Una imagen del historial del celular que el PC pidió.
    ClipImage {
        id: String,
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    /// El usuario borró ítems en el celular.
    ClipDelete { ids: Vec<String> },
    /// Lo que suena en el celular, para la pill del PC. `None`: nada.
    PhoneMedia {
        media: Option<MediaState>,
        /// Carátula chica en JPEG.
        #[serde(default, with = "b64_opt", skip_serializing_if = "Option::is_none")]
        art: Option<Vec<u8>>,
    },
    /// Una notificación nueva (o actualizada) del celular ([`feature::NOTICES`]).
    Notice { notice: PhoneNotice },
    /// Se fue la notificación en el celular.
    NoticeRemoved { key: String },
    /// La llamada del celular; `None`: ya no hay ([`feature::CALLS`]).
    Call { call: Option<CallState> },
    /// Texto para un agente, como si se escribiera en su consola ([`feature::PROMPT`]).
    ///
    /// Con [`feature::AGENTS`] puede llevar imágenes del celular y del historial
    /// del portapapeles del PC (`clip_ids`, los de [`ToPhone::ClipHistory`]).
    #[serde(rename_all = "camelCase")]
    Prompt {
        agent_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<PromptImage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        clip_ids: Vec<String>,
    },
    /// Aceptar, abrir o detener un agente ([`feature::AGENTS`]).
    #[serde(rename_all = "camelCase")]
    AgentAction { agent_id: String, action: AgentAction },
    /// Mirar en vivo lo que escribe un agente; `None`: dejar de mirar ([`feature::WATCH`]).
    #[serde(rename_all = "camelCase")]
    Watch { agent_id: Option<String> },
    /// El puntero o el teclado del PC ([`feature::REMOTE`]).
    Input { event: InputEvent },
    /// Abrir este enlace en el navegador del PC ([`feature::LINKS`]).
    OpenUrl { url: String },
    /// Pedirle al PC que prenda estas funciones ([`feature::requestable`]); quien
    /// está en el PC acepta o rechaza y vuelve [`ToPhone::AccessAnswer`]
    /// ([`feature::ACCESS`]).
    RequestAccess { features: Vec<String> },
    /// La foto que pidió el PC con [`ToPhone::TakePhoto`] (JPEG o PNG, [`feature::PHOTO`]).
    Photo {
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
}

/// Del escritorio al celular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToPhone {
    #[serde(rename_all = "camelCase")]
    Welcome {
        desktop_name: String,
        /// Lo que este PC sabe hacer (ver [`feature`]). Un PC viejo no manda nada.
        #[serde(default)]
        features: Vec<String>,
    },
    /// El token era bueno; falta que alguien acepte en el PC.
    AwaitingApproval,
    Rejected { reason: String },
    /// Foto completa: reemplaza la anterior.
    Agents { agents: Vec<AgentCard> },
    /// Algo que se copió en el PC.
    Clip { item: ClipItem },
    /// Estado completo de medios y grabación: reemplaza el anterior.
    Pc { pc: PcState },
    /// Los últimos textos del historial del PC, fijados primero. Vacío si el
    /// portapapeles compartido está apagado.
    ClipHistory { items: Vec<ClipItem> },
    /// Controlar la música del celular desde la pill del PC.
    MediaCommand { command: PcCommand },
    /// Imágenes del historial del celular que el PC no tiene y quiere.
    ClipNeed { ids: Vec<String> },
    /// Ítems borrados en el PC: el celular los borra y lo recuerda.
    ClipDeleted { ids: Vec<String> },
    /// Contestar una notificación del celular desde el PC.
    NoticeReply { key: String, text: String },
    /// Descartarla desde el PC.
    NoticeDismiss { key: String },
    /// Contestar o colgar la llamada del celular.
    CallAction { action: CallAction },
    /// Lo que escribió el agente que se mira, por partes: lo del usuario, la
    /// respuesta en Markdown y las herramientas, con imágenes. Reemplaza a
    /// [`ToPhone::AgentOutput`] con los celulares que tienen [`feature::AGENTS`].
    #[serde(rename_all = "camelCase")]
    AgentEntries {
        agent_id: String,
        entries: Vec<AgentEntry>,
        #[serde(default)]
        reset: bool,
    },
    /// Lo que escribió el agente que se mira: líneas nuevas, o todas si `reset`.
    #[serde(rename_all = "camelCase")]
    AgentOutput {
        agent_id: String,
        lines: Vec<String>,
        #[serde(default)]
        reset: bool,
    },
    /// Hacer sonar el celular para encontrarlo; `false`: que se calle ([`feature::FIND`]).
    Ring { on: bool },
    /// Abrir este enlace en el navegador del celular ([`feature::LINKS`]).
    OpenUrl { url: String },
    /// Lo que sabe hacer el PC cambió (se prendió o apagó algo en sus ajustes):
    /// reemplaza lo del [`ToPhone::Welcome`] ([`feature::ACCESS`]).
    Features { features: Vec<String> },
    /// La respuesta a [`ToDesktop::RequestAccess`]. Si se aceptó, antes llega
    /// [`ToPhone::Features`] con lo nuevo ([`feature::ACCESS`]).
    AccessAnswer { features: Vec<String>, granted: bool },
    /// Sacar una foto con la cámara del celular y mandarla con
    /// [`ToDesktop::Photo`] ([`feature::PHOTO`]). Si se cancela, no vuelve nada.
    TakePhoto,
    /// Respuesta a [`ToDesktop::FetchImage`]. Sin `data`: ya no existe.
    Image {
        id: String,
        mime: String,
        #[serde(default, with = "b64_opt", skip_serializing_if = "Option::is_none")]
        data: Option<Vec<u8>>,
    },
}

/// Bytes como base64 dentro del JSON (un arreglo de números pesaría 3–4 veces más).
mod b64 {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

mod b64_opt {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_some(&STANDARD.encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let text = Option::<String>::deserialize(d)?;
        text.map(|t| STANDARD.decode(t).map_err(serde::de::Error::custom)).transpose()
    }
}

pub async fn write_frame<W, T>(w: &mut W, msg: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = serde_json::to_vec(msg)?;
    let len = u32::try_from(body.len()).context("mensaje demasiado grande")?;
    if len > MAX_FRAME {
        bail!("mensaje de {len} bytes supera el tope");
    }
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

/// Lee mensajes en una tarea propia y los entrega por un canal.
///
/// [`read_frame`] no se puede cancelar a medias: si se descarta cuando ya leyó
/// el largo (o parte del cuerpo), esos bytes se pierden y la lectura siguiente
/// arranca en medio de otro mensaje. Dentro de un `select!` eso pasa cada vez
/// que otra rama gana mientras llega algo grande (una imagen). El canal sí se
/// puede cancelar sin perder nada.
pub fn spawn_reader<R, T>(mut reader: R) -> (tokio::sync::mpsc::Receiver<Result<Option<T>>>, ReaderTask)
where
    R: AsyncRead + Unpin + Send + 'static,
    T: for<'de> Deserialize<'de> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        loop {
            let frame = read_frame::<_, T>(&mut reader).await;
            let last = !matches!(frame, Ok(Some(_)));
            if tx.send(frame).await.is_err() || last {
                return;
            }
        }
    });
    (rx, ReaderTask(task))
}

/// La tarea de [`spawn_reader`]; se corta al soltarla.
pub struct ReaderTask(tokio::task::JoinHandle<()>);

impl Drop for ReaderTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// `None` si el otro lado cerró la corriente limpio entre mensajes.
pub async fn read_frame<R, T>(r: &mut R) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    read_frame_max(r, MAX_FRAME).await
}

/// Tope del saludo (`Hello`/`Pair`): se lee antes de saber quién es el otro,
/// así que un desconocido no alcanza a hacer reservar los 8 MB de un mensaje.
pub const MAX_GREETING: u32 = 16 * 1024;

/// Como [`read_frame`], con un tope propio para el largo.
pub async fn read_frame_max<R, T>(r: &mut R, max: u32) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len);
    if len > max {
        bail!("mensaje de {len} bytes supera el tope");
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn un_mensaje_va_y_vuelve_igual() {
        let msg = ToDesktop::Decide {
            agent_id: "a".into(),
            permission_id: "p".into(),
            decision: Decision::AllowAlways,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).await.unwrap();
        let back: Option<ToDesktop> = read_frame(&mut buf.as_slice()).await.unwrap();
        assert_eq!(back, Some(msg));
    }

    #[test]
    fn solo_se_abren_enlaces_web() {
        assert!(is_web_url("https://atic.app/precios?x=1#y"));
        assert!(is_web_url("HTTP://192.168.1.4:8080"));
        for url in [
            "",
            "https://",
            "http:///etc/passwd",
            "file:///C:/Windows",
            "javascript:alert(1)",
            "intent://scan/#Intent;scheme=zxing;end",
            "https://a.com/ con espacio",
            "https://a.com/\u{202E}gpj",
            "www.atic.app",
        ] {
            assert!(!is_web_url(url), "{url}");
        }
        assert!(!is_web_url(&format!("https://a.com/{}", "a".repeat(MAX_URL_BYTES))));
    }

    #[tokio::test]
    async fn la_foto_y_el_enlace_van_y_vuelven() {
        for msg in [ToDesktop::OpenUrl { url: "https://atic.app".into() }, ToDesktop::Photo { mime: "image/jpeg".into(), data: vec![1, 2, 3] }] {
            let mut buf = Vec::new();
            write_frame(&mut buf, &msg).await.unwrap();
            assert_eq!(read_frame::<_, ToDesktop>(&mut buf.as_slice()).await.unwrap(), Some(msg));
        }
        for msg in [
            ToPhone::TakePhoto,
            ToPhone::OpenUrl { url: "https://atic.app".into() },
            ToPhone::Features { features: vec!["control".into()] },
            ToPhone::AccessAnswer { features: vec!["control".into()], granted: false },
        ] {
            let mut buf = Vec::new();
            write_frame(&mut buf, &msg).await.unwrap();
            assert_eq!(read_frame::<_, ToPhone>(&mut buf.as_slice()).await.unwrap(), Some(msg));
        }
    }

    #[tokio::test]
    async fn cerrar_entre_mensajes_no_es_error() {
        let back: Option<ToPhone> = read_frame(&mut [].as_slice()).await.unwrap();
        assert_eq!(back, None);
    }

    #[tokio::test]
    async fn un_largo_absurdo_se_rechaza_sin_reservar_memoria() {
        let bytes = u32::MAX.to_be_bytes();
        let back: Result<Option<ToPhone>> = read_frame(&mut bytes.as_slice()).await;
        assert!(back.is_err());
    }

    #[test]
    fn un_pc_viejo_entiende_el_saludo_nuevo_y_al_reves() {
        // El PC de antes ignora las funciones que anuncia el celular...
        let hello: ToDesktop =
            serde_json::from_str(r#"{"type":"hello","deviceName":"S24","features":["files"]}"#).unwrap();
        assert_eq!(hello, ToDesktop::Hello { device_name: "S24".into(), features: vec!["files".into()] });
        // ...y un saludo sin funciones es un PC que no sabe ninguna.
        let welcome: ToPhone = serde_json::from_str(r#"{"type":"welcome","desktopName":"PC"}"#).unwrap();
        assert_eq!(welcome, ToPhone::Welcome { desktop_name: "PC".into(), features: vec![] });
    }

    #[test]
    fn el_nombre_de_un_archivo_no_sale_de_su_carpeta() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\x\\foto.png"), "foto.png");
        assert_eq!(safe_file_name(".."), "archivo");
        assert_eq!(safe_file_name("informe: final?.pdf"), "informe final.pdf");
        assert_eq!(safe_file_name("foto\u{202E}gpj.exe"), "fotogpj.exe");
        assert_eq!(safe_file_name("NUL.txt"), "_NUL.txt");
        assert_eq!(safe_file_name("com3"), "_com3");
        assert_eq!(safe_file_name("COMET.txt"), "COMET.txt");
        assert_eq!(safe_file_name("nota. "), "nota");
    }

    #[tokio::test]
    async fn un_saludo_grande_se_rechaza() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &ToDesktop::Hello { device_name: "x".repeat(MAX_GREETING as usize), features: vec![] }).await.unwrap();
        let back: Result<Option<ToDesktop>> = read_frame_max(&mut buf.as_slice(), MAX_GREETING).await;
        assert!(back.is_err());
    }

    #[test]
    fn el_json_usa_los_nombres_del_desktop() {
        let json = serde_json::to_string(&ToDesktop::Decide {
            agent_id: "a".into(),
            permission_id: "p".into(),
            decision: Decision::AllowAlways,
        })
        .unwrap();
        assert_eq!(json, r#"{"type":"decide","agentId":"a","permissionId":"p","decision":"allowAlways"}"#);
    }
}
