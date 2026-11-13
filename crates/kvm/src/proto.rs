//! Los mensajes entre los dos equipos: frames JSON con su largo delante
//! (`[u32 BE][JSON]`), en un solo bi-stream por sesión.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::layout::{Rect, Side};

/// La sesión de mouse y teclado. Un cambio incompatible cambia la versión.
pub const ALPN: &[u8] = b"atic/kvm/1";
/// El vínculo con código (una conexión corta por intento).
pub const PAIR_ALPN: &[u8] = b"atic/kvm-pair/1";

/// Por qué se cierra un pedido de vínculo sin atenderlo (el código con que se
/// cierra la conexión): nadie abrió el vínculo en este equipo, ya se atiende
/// otro pedido, o hubo demasiados intentos fallidos.
pub const PAIR_CLOSED: u32 = 10;
pub const PAIR_BUSY: u32 = 11;
pub const PAIR_LOCKED: u32 = 12;

/// Lo más grande que cruza es una imagen copiada ([`Msg::ClipImage`]).
const MAX_FRAME: u32 = 32 * 1024 * 1024;
/// Tope del saludo y de todo el vínculo: se leen antes de saber quién es el
/// otro, así que un desconocido no alcanza a hacer reservar los 32 MB.
pub const MAX_GREETING: u32 = 16 * 1024;
/// Lo más largo del nombre de un equipo (se muestra y se guarda en `peers.json`).
pub const MAX_NAME: usize = 60;
/// Un texto copiado que pase de esto no se comparte.
pub const MAX_CLIP_TEXT: usize = 1024 * 1024;
/// Ni una imagen cuyo PNG pase de esto.
pub const MAX_CLIP_PNG: usize = 20 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Windows,
    Macos,
    Other,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    Left,
    Right,
    Middle,
    /// Los botones laterales (atrás y adelante en el navegador).
    Back,
    Forward,
}

/// Lo que va por la sesión. El equipo con el mouse manda casi todo; el otro
/// solo saluda y devuelve el cursor ([`Msg::Back`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    /// Lo primero que manda cada lado. `layout`: de qué lado de quien recibe
    /// está quien saluda, si quien saluda comparte con él (el PC con el Mac a
    /// su derecha manda `left`): así el otro puede devolver el cursor por el
    /// lado correcto sin configurarlo. Solo va al compartir, porque las
    /// versiones anteriores lo leen como «el otro comparte conmigo».
    ///
    /// Desde [`CAP_LAYOUT2`] también van `place` (lo mismo que `layout`, con
    /// la misma convención, pero siempre que haya un lado elegido) y
    /// `layout_rev` (cuándo se eligió, ver [`Msg::Layout`]). Un saludo de
    /// antes llega sin `caps`, `place` ni `layout_rev`.
    Hello {
        name: String,
        os: Os,
        screens: Vec<Rect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<Side>,
        /// Lo que entiende quien saluda ([`CAP_LAYOUT2`], [`CAP_PING`]).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        caps: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        place: Option<Side>,
        #[serde(default, skip_serializing_if = "is_zero")]
        layout_rev: u64,
    },
    /// El cursor entra por `side` de las pantallas de quien recibe, a `along`
    /// (0 a 1) del largo de ese borde.
    Enter { side: Side, along: f64 },
    /// Movimiento relativo, en píxeles de quien manda (ya con su aceleración).
    Move { dx: i32, dy: i32 },
    Button { button: Button, down: bool },
    /// La rueda, en saltos (un clic de rueda es 1). `dy` positivo: hacia
    /// adelante (la página sube); `dx` positivo: a la derecha.
    Wheel { dx: f32, dy: f32 },
    /// Una tecla, como código HID (página 7, [`crate::keys`]).
    Key { hid: u16, down: bool },
    /// Quien manda recuperó el cursor sin que volviera por el borde (el atajo
    /// de emergencia): quien recibe suelta todo lo que tenga apretado.
    Leave,
    /// Quien recibe devuelve el cursor: cruzó de vuelta el borde por el que
    /// entró, a `along` de su largo.
    Back { along: f64 },
    /// Se copió texto en el otro equipo.
    ClipText { text: String },
    /// Se copió una imagen en el otro equipo: PNG en base64.
    ClipImage { png: String },
    /// Quien manda cambió de qué lado está el otro ([`CAP_LAYOUT2`]). Misma
    /// convención que `layout` del saludo: `side` es de qué lado de quien
    /// recibe está quien manda (el PC que puso al Mac a su derecha manda
    /// `left`), así que quien recibe lo guarda tal cual. `rev`: milisegundos
    /// desde 1970 de ese cambio. Gana el `rev` mayor y, si empatan, el lado
    /// del equipo con el id mayor; quien pierde adopta el del otro.
    Layout { side: Side, rev: u64 },
    /// Mientras el cursor está en el otro, cada segundo ([`CAP_PING`]): si no
    /// contesta, el mouse y el teclado vuelven a este equipo.
    Ping,
    Pong,
}

/// Entiende [`Msg::Layout`] y los campos nuevos del saludo.
pub const CAP_LAYOUT2: &str = "layout2";
/// Contesta [`Msg::Ping`].
pub const CAP_PING: &str = "ping";
/// Lo que entiende esta versión (va en el saludo).
pub fn caps() -> Vec<String> {
    vec![CAP_LAYOUT2.into(), CAP_PING.into()]
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Lo que va por la conexión de vínculo ([`PAIR_ALPN`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PairMsg {
    /// Quien quiere vincularse se presenta; el otro muestra un código.
    Ask { name: String, os: Os },
    /// El código ya está en la pantalla del otro.
    CodeShown,
    /// El código que escribió el usuario.
    Code { code: String },
    /// Vinculados: el nombre y el sistema de quien mostró el código.
    Done { name: String, os: Os },
    Refused { reason: String },
}

pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    let len = u32::try_from(body.len()).context("mensaje demasiado grande")?;
    if len > MAX_FRAME {
        bail!("mensaje de {len} bytes supera el tope");
    }
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// `None` si el otro cerró limpio entre dos mensajes.
pub async fn read_frame<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(r: &mut R) -> Result<Option<T>> {
    read_frame_max(r, MAX_FRAME).await
}

/// Como [`read_frame`], con un tope propio para el largo.
pub async fn read_frame_max<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(r: &mut R, max: u32) -> Result<Option<T>> {
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
    Ok(Some(serde_json::from_slice(&body).context("mensaje ilegible")?))
}

/// Lo que llega por la sesión: un mensaje, o uno que esta versión no conoce.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Msg(Msg),
    /// De una versión más nueva del otro (un tipo que aquí no existe): se
    /// salta sin cortar la sesión. El largo va delante, así que el siguiente
    /// se lee bien.
    Unknown,
}

/// Como [`read_frame`], para la sesión: un mensaje que no se entiende no la corta.
pub async fn read_msg<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Incoming>> {
    let Some(value) = read_frame::<_, serde_json::Value>(r).await? else { return Ok(None) };
    Ok(Some(match serde_json::from_value::<Msg>(value) {
        Ok(msg) => Incoming::Msg(msg),
        Err(e) => {
            tracing::debug!("mensaje desconocido, se salta: {e}");
            Incoming::Unknown
        }
    }))
}

/// Un texto que manda el otro equipo, antes de mostrarlo o guardarlo: sin
/// caracteres de control ni los que cambian el sentido de lectura (con ellos
/// un nombre se lee distinto de lo que es), y a lo más `max` caracteres.
pub fn clean_text(text: &str, max: usize) -> String {
    let hidden = |c: char| c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let clean: String = text.chars().filter(|c| !hidden(*c)).collect();
    clean.trim().chars().take(max).collect()
}

/// El nombre de un equipo como lo manda el otro, listo para mostrar.
pub fn clean_name(name: &str) -> String {
    let name = clean_text(name, MAX_NAME);
    if name.is_empty() {
        "(sin nombre)".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn los_mensajes_van_y_vuelven_iguales() {
        let msgs = vec![
            Msg::Hello { name: "Mac de Chris".into(), os: Os::Macos, screens: vec![Rect { x: 0., y: 0., w: 1512., h: 982. }], layout: Some(Side::Left), caps: vec![], place: None, layout_rev: 0 },
            Msg::Enter { side: Side::Left, along: 0.25 },
            Msg::Move { dx: -3, dy: 7 },
            Msg::Button { button: Button::Forward, down: true },
            Msg::Wheel { dx: 0., dy: -1.5 },
            Msg::Key { hid: 0x04, down: true },
            Msg::Leave,
            Msg::Back { along: 1.0 },
            Msg::ClipText { text: "copiado ñ".into() },
            Msg::Hello { name: "PC".into(), os: Os::Windows, screens: vec![], layout: None, caps: caps(), place: Some(Side::Right), layout_rev: 1_763_000_000_000 },
            Msg::Layout { side: Side::Top, rev: 7 },
            Msg::Ping,
            Msg::Pong,
        ];
        let mut buf = Vec::new();
        for m in &msgs {
            write_frame(&mut buf, m).await.unwrap();
        }
        let mut r = buf.as_slice();
        for m in &msgs {
            assert_eq!(read_frame::<_, Msg>(&mut r).await.unwrap().as_ref(), Some(m));
        }
        assert_eq!(read_frame::<_, Msg>(&mut r).await.unwrap(), None);
    }

    #[test]
    fn un_saludo_sin_layout_se_entiende() {
        let m: Msg = serde_json::from_str(r#"{"t":"hello","name":"PC","os":"windows","screens":[]}"#).unwrap();
        assert!(matches!(m, Msg::Hello { layout: None, .. }));
    }

    /// Un saludo nuevo lo entiende una versión anterior (los campos que no
    /// conoce se ignoran), y uno viejo llega sin capacidades.
    #[test]
    fn el_saludo_nuevo_y_el_viejo_se_entienden() {
        let new = Msg::Hello { name: "PC".into(), os: Os::Windows, screens: vec![], layout: None, caps: caps(), place: Some(Side::Left), layout_rev: 5 };
        let json = serde_json::to_string(&new).unwrap();
        #[derive(Deserialize)]
        #[serde(tag = "t", rename_all = "snake_case")]
        enum Before {
            Hello { name: String, layout: Option<Side> },
        }
        let Before::Hello { name, layout } = serde_json::from_str(&json).unwrap();
        assert_eq!((name.as_str(), layout), ("PC", None));
        let old: Msg = serde_json::from_str(r#"{"t":"hello","name":"PC","os":"windows","screens":[],"layout":"left"}"#).unwrap();
        assert_eq!(old, Msg::Hello { name: "PC".into(), os: Os::Windows, screens: vec![], layout: Some(Side::Left), caps: vec![], place: None, layout_rev: 0 });
    }

    /// Un tipo que esta versión no conoce se salta y la sesión sigue.
    #[tokio::test]
    async fn un_mensaje_desconocido_se_salta() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &serde_json::json!({"t": "algo_del_futuro", "x": 1})).await.unwrap();
        write_frame(&mut buf, &Msg::Ping).await.unwrap();
        let mut r = buf.as_slice();
        assert_eq!(read_msg(&mut r).await.unwrap(), Some(Incoming::Unknown));
        assert_eq!(read_msg(&mut r).await.unwrap(), Some(Incoming::Msg(Msg::Ping)));
        assert_eq!(read_msg(&mut r).await.unwrap(), None);
    }

    #[tokio::test]
    async fn un_frame_enorme_se_rechaza() {
        let mut buf = (MAX_FRAME + 1).to_be_bytes().to_vec();
        buf.extend_from_slice(b"{}");
        assert!(read_frame::<_, Msg>(&mut buf.as_slice()).await.is_err());
    }

    /// El saludo y el vínculo tienen su propio tope, mucho más chico.
    #[tokio::test]
    async fn un_saludo_enorme_se_rechaza() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &PairMsg::Ask { name: "x".repeat(MAX_GREETING as usize), os: Os::Windows }).await.unwrap();
        assert!(read_frame_max::<_, PairMsg>(&mut buf.as_slice(), MAX_GREETING).await.is_err());
        // Basta el largo anunciado: no alcanza a reservar nada.
        let announced = (MAX_GREETING + 1).to_be_bytes();
        assert!(read_frame_max::<_, PairMsg>(&mut announced.as_slice(), MAX_GREETING).await.is_err());
        let mut ok = Vec::new();
        write_frame(&mut ok, &PairMsg::Ask { name: "PC".into(), os: Os::Windows }).await.unwrap();
        assert!(read_frame_max::<_, PairMsg>(&mut ok.as_slice(), MAX_GREETING).await.unwrap().is_some());
    }

    #[test]
    fn el_nombre_del_otro_se_limpia_y_se_corta() {
        assert_eq!(clean_name("  Mac de Chris \n"), "Mac de Chris");
        assert_eq!(clean_name("PC\u{0}\u{1b}[31m\r\nde\tAna"), "PC[31mdeAna");
        // Sin los que dan vuelta el texto.
        assert_eq!(clean_name("PC\u{202e}gnp.exe"), "PCgnp.exe");
        // Por caracteres, no por bytes.
        assert_eq!(clean_name(&"ñ".repeat(500)), "ñ".repeat(MAX_NAME));
        assert_eq!(clean_name("\u{7}\n  "), "(sin nombre)");
    }
}
