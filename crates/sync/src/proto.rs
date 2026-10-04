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
    Pair { token: String, device_name: String },
    /// Primer mensaje de un equipo ya pareado.
    #[serde(rename_all = "camelCase")]
    Hello { device_name: String },
    #[serde(rename_all = "camelCase")]
    Decide { agent_id: String, permission_id: String, decision: Decision },
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
}

/// Del escritorio al celular.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToPhone {
    #[serde(rename_all = "camelCase")]
    Welcome { desktop_name: String },
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
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME {
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
