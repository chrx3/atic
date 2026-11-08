//! El portapapeles compartido: lo que se copia en un equipo queda listo para
//! pegar en el otro (texto e imágenes).
//!
//! Los archivos copiados (en el Finder o el Explorador) también: se mandan
//! aparte ([`crate::files`]) y en el otro quedan en su portapapeles.
//!
//! Un hilo mira el número de cambios del portapapeles (`changeCount` en
//! macOS, `GetClipboardSequenceNumber` en Windows), que no abre el
//! portapapeles; solo cuando cambia lee lo nuevo. No se comparte lo que el
//! sistema marca como sensible (contraseñas de los gestores) ni lo que llegó
//! del otro equipo (no rebota).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use atic_clipboard::{
    clipboard_is_sensitive, clipboard_sequence, encode_png_rgba, set_system_image, set_system_text, try_clipboard_read, with_clipboard_write,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use tokio::sync::mpsc::UnboundedSender;

use crate::proto::{Msg, MAX_CLIP_PNG, MAX_CLIP_TEXT};

const POLL: Duration = Duration::from_millis(350);

/// Lo que se copió en este equipo.
#[derive(Clone, Debug, PartialEq)]
pub enum Copied {
    /// Texto o imagen: va como mensaje.
    Msg(Msg),
    /// Archivos o carpetas: van por su propia corriente.
    Files(Vec<PathBuf>),
}

#[derive(Default)]
pub struct Clip {
    /// El número del portapapeles después de escribir lo que llegó del otro: ese cambio no se manda.
    own: Mutex<Option<u64>>,
}

impl Clip {
    /// Empieza a mirar el portapapeles; lo que se copie aquí sale por `tx`.
    pub fn watch(self: &Arc<Self>, tx: UnboundedSender<Copied>) {
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
        self.mark_own();
        Ok(())
    }

    /// Deja en el portapapeles de aquí los archivos que llegaron del otro.
    pub fn set_files(&self, paths: &[PathBuf]) -> Result<()> {
        with_clipboard_write(|| {
            let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
            clipboard.set().file_list(paths).map_err(|e| e.to_string())
        })
        .map_err(|e| anyhow!(e))?;
        self.mark_own();
        Ok(())
    }

    /// Lo que hay ahora lo escribió este módulo: no se manda de vuelta.
    fn mark_own(&self) {
        *self.own.lock().unwrap_or_else(|e| e.into_inner()) = clipboard_sequence();
    }
}

/// Lo que hay copiado: los archivos si hay (el Finder también pone su nombre
/// como texto), si no el texto, si no la imagen.
fn read() -> Option<Copied> {
    try_clipboard_read(|| {
        let mut clipboard = Clipboard::new().ok()?;
        if let Ok(files) = clipboard.get().file_list() {
            if !files.is_empty() {
                return Some(Copied::Files(files));
            }
        }
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                return (text.len() <= MAX_CLIP_TEXT).then_some(Copied::Msg(Msg::ClipText { text }));
            }
        }
        let image = clipboard.get_image().ok()?;
        let png = encode_png_rgba(&image.bytes, image.width, image.height).ok()?;
        (png.len() <= MAX_CLIP_PNG).then(|| Copied::Msg(Msg::ClipImage { png: STANDARD.encode(png) }))
    })
    .flatten()
}
