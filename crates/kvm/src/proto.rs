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

/// Ningún mensaje legítimo se acerca a esto.
const MAX_FRAME: u32 = 64 * 1024;

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
    /// Lo primero que manda cada lado.
    Hello { name: String, os: Os, screens: Vec<Rect> },
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
    Ok(Some(serde_json::from_slice(&body).context("mensaje ilegible")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn los_mensajes_van_y_vuelven_iguales() {
        let msgs = vec![
            Msg::Hello { name: "Mac de Chris".into(), os: Os::Macos, screens: vec![Rect { x: 0., y: 0., w: 1512., h: 982. }] },
            Msg::Enter { side: Side::Left, along: 0.25 },
            Msg::Move { dx: -3, dy: 7 },
            Msg::Button { button: Button::Forward, down: true },
            Msg::Wheel { dx: 0., dy: -1.5 },
            Msg::Key { hid: 0x04, down: true },
            Msg::Leave,
            Msg::Back { along: 1.0 },
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

    #[tokio::test]
    async fn un_frame_enorme_se_rechaza() {
        let mut buf = (MAX_FRAME + 1).to_be_bytes().to_vec();
        buf.extend_from_slice(b"{}");
        assert!(read_frame::<_, Msg>(&mut buf.as_slice()).await.is_err());
    }
}
