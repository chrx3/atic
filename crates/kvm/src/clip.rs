//! El portapapeles compartido: lo que se copia en un equipo queda listo para
//! pegar en el otro (texto e imágenes).
//!
//! Un hilo mira el número de cambios del portapapeles (`changeCount` en
//! macOS, `GetClipboardSequenceNumber` en Windows), que no abre el
//! portapapeles; solo cuando cambia lee lo nuevo. No se comparte lo que el
//! sistema marca como sensible (contraseñas de los gestores) ni lo que llegó
//! del otro equipo (no rebota).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use atic_clipboard::{clipboard_is_sensitive, clipboard_sequence, encode_png_rgba, set_system_image, set_system_text, try_clipboard_read};
use base64::{engine::general_purpose::STANDARD, Engine};
use tokio::sync::mpsc::UnboundedSender;

use crate::proto::{Msg, MAX_CLIP_PNG, MAX_CLIP_TEXT};

const POLL: Duration = Duration::from_millis(350);

#[derive(Default)]
pub struct Clip {
    /// El número del portapapeles después de escribir lo que llegó del otro: ese cambio no se manda.
    own: Mutex<Option<u64>>,
}

impl Clip {
    /// Empieza a mirar el portapapeles; lo que se copie aquí sale por `tx`.
    pub fn watch(self: &Arc<Self>, tx: UnboundedSender<Msg>) {
        let clip = self.clone();
        let spawned = std::thread::Builder::new().name("atic-kvm-clip".into()).spawn(move || {
            // Lo que ya estaba copiado al empezar no se manda.
            let mut last = clipboard_sequence();
            loop {
                std::thread::sleep(POLL);
                if tx.is_closed() {
                    return;
                }
                let seq = clipboard_sequence();
                if seq.is_none() || seq == last {
                    continue;
                }
                last = seq;
                if *clip.own.lock().unwrap_or_else(|e| e.into_inner()) == seq || clipboard_is_sensitive() {
                    continue;
                }
                if let Some(msg) = read() {
                    let _ = tx.send(msg);
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("no se pudo mirar el portapapeles: {e}");
        }
    }

    /// Escribe aquí lo que se copió en el otro equipo. Bloquea (decodifica la imagen).
    pub fn apply(&self, msg: &Msg) -> Result<()> {
        match msg {
            Msg::ClipText { text } => set_system_text(text.clone()).map_err(|e| anyhow!(e))?,
            Msg::ClipImage { png } => {
                let bytes = STANDARD.decode(png).context("imagen ilegible")?;
                let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).context("imagen ilegible")?.to_rgba8();
                let (w, h) = (image.width() as usize, image.height() as usize);
                set_system_image(w, h, image.into_raw()).map_err(|e| anyhow!(e))?;
            }
            _ => return Ok(()),
        }
        *self.own.lock().unwrap_or_else(|e| e.into_inner()) = clipboard_sequence();
        Ok(())
    }
}

/// Lo que hay copiado, como mensaje: el texto si hay, si no la imagen.
fn read() -> Option<Msg> {
    try_clipboard_read(|| {
        let mut clipboard = Clipboard::new().ok()?;
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                return (text.len() <= MAX_CLIP_TEXT).then_some(Msg::ClipText { text });
            }
        }
        let image = clipboard.get_image().ok()?;
        let png = encode_png_rgba(&image.bytes, image.width, image.height).ok()?;
        (png.len() <= MAX_CLIP_PNG).then(|| Msg::ClipImage { png: STANDARD.encode(png) })
    })
    .flatten()
}
