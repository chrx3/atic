//! Archivos entre el celular y el PC, cada uno por su propia corriente.
//!
//! La corriente de mensajes lleva cosas chicas; un archivo de cientos de MB la
//! frenaría y tendría que pasar entero por la memoria. Acá cada archivo abre
//! una corriente de un solo sentido: primero un [`FileHeader`] encuadrado como
//! cualquier mensaje y detrás los bytes, que van y vienen del disco por partes.
//!
//! Si los dos lados anuncian [`crate::feature::FILE_OFFER`], el archivo se ofrece
//! primero: quien manda escribe solo el encabezado (con [`FileHeader::offer`]) y
//! espera la respuesta, que llega por la corriente de mensajes. Quien recibe decide
//! con el encabezado (el tamaño, si lo pidió, lo que diga el usuario) antes de que
//! viaje un byte del contenido.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use iroh::endpoint::{Connection, RecvStream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};

use crate::lock::MutexExt;

use crate::proto::{read_frame, safe_file_name, write_frame, FileHeader, MAX_FILE_BYTES};

const CHUNK: usize = 64 * 1024;
/// Cada cuánto se avisa el avance: más seguido no se nota y cuesta.
const PROGRESS_EVERY: u64 = 512 * 1024;

/// Cuánto espera quien ofrece un archivo a que el otro lado conteste (allá puede
/// haber una persona decidiendo).
pub const OFFER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Si el otro lado corta la corriente sin contestar, cuánto se espera la respuesta
/// que puede venir atrás por la corriente de mensajes.
const ANSWER_GRACE: Duration = Duration::from_secs(2);
/// Códigos al cortar la corriente de un archivo que no se va a mandar o recibir.
const CODE_DECLINED: u32 = 1;
const CODE_TOO_LARGE: u32 = 2;

/// Por qué no salió un archivo sin que sea una falla: viene dentro del error de
/// quien lo manda (`error.downcast_ref::<SendRefused>()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendRefused {
    /// Quien lo recibía dijo que no.
    #[error("el otro equipo no aceptó el archivo")]
    Declined,
    /// Pasa el tope, acá o según quien lo recibía.
    #[error("el archivo pasa el tope de {} MB", MAX_FILE_BYTES >> 20)]
    TooLarge,
    /// Nadie contestó a tiempo.
    #[error("el otro equipo no contestó si aceptaba el archivo")]
    NoAnswer,
}

/// Lo que contesta quien recibe un archivo ofrecido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileAnswer {
    pub accept: bool,
    /// Se rechazó por el tamaño del encabezado.
    pub too_large: bool,
}

impl FileAnswer {
    fn refusal(self) -> Option<SendRefused> {
        match (self.accept, self.too_large) {
            (true, _) => None,
            (false, true) => Some(SendRefused::TooLarge),
            (false, false) => Some(SendRefused::Declined),
        }
    }
}

/// Las respuestas que espera un lado, por el `id` del archivo ofrecido: la sesión
/// las entrega cuando llegan por la corriente de mensajes.
#[derive(Clone, Default)]
pub(crate) struct Answers(Arc<Mutex<HashMap<String, oneshot::Sender<FileAnswer>>>>);

impl Answers {
    fn wait(&self, id: &str) -> oneshot::Receiver<FileAnswer> {
        let (tx, rx) = oneshot::channel();
        self.0.lock_or_recover().insert(id.to_string(), tx);
        rx
    }

    fn forget(&self, id: &str) {
        self.0.lock_or_recover().remove(id);
    }

    /// Llegó la respuesta para `id`. Una que nadie espera se ignora.
    pub(crate) fn deliver(&self, id: &str, answer: FileAnswer) {
        if let Some(tx) = self.0.lock_or_recover().remove(id) {
            let _ = tx.send(answer);
        }
    }

    /// Se cortó la sesión: quien esperaba deja de esperar.
    pub(crate) fn clear(&self) {
        self.0.lock_or_recover().clear();
    }
}

/// Manda `path` por una corriente nueva de `conn`. `progress` recibe los bytes
/// enviados y el total.
pub async fn send(conn: &Connection, header: FileHeader, path: &Path, progress: impl FnMut(u64, u64)) -> Result<()> {
    send_inner(conn, header, path, None, progress).await
}

/// Como [`send`], ofreciendo primero ([`crate::feature::FILE_OFFER`]): manda el
/// encabezado y no sigue hasta que el otro lado acepte. Si no acepta, el error
/// lleva un [`SendRefused`] y no viajó nada del contenido.
pub(crate) async fn send_offered(
    conn: &Connection,
    mut header: FileHeader,
    path: &Path,
    answers: &Answers,
    progress: impl FnMut(u64, u64),
) -> Result<()> {
    header.offer = true;
    let answer = answers.wait(&header.id);
    // Al soltarlo deja de esperar la respuesta, también si el envío se abandona a medias.
    let _waiting = Waiting { answers, id: header.id.clone() };
    send_inner(conn, header, path, Some(answer), progress).await
}

/// Una respuesta que se espera: al soltarlo, ya no.
struct Waiting<'a> {
    answers: &'a Answers,
    id: String,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.answers.forget(&self.id);
    }
}

async fn send_inner(
    conn: &Connection,
    header: FileHeader,
    path: &Path,
    answer: Option<oneshot::Receiver<FileAnswer>>,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    if header.size > MAX_FILE_BYTES {
        return Err(SendRefused::TooLarge.into());
    }
    let mut file = tokio::fs::File::open(path).await.context("no se pudo abrir el archivo")?;
    let mut stream = conn.open_uni().await.context("no se pudo abrir la corriente")?;
    write_frame(&mut stream, &header).await?;
    if let Some(mut answer) = answer {
        let got = tokio::select! {
            got = &mut answer => got.ok(),
            stopped = stream.stopped() => match stopped {
                // El otro lado soltó la corriente: su respuesta puede venir un poco atrás.
                Ok(_) => tokio::time::timeout(ANSWER_GRACE, &mut answer).await.ok().and_then(|got| got.ok()),
                Err(e) => bail!("se cortó la conexión: {e}"),
            },
            _ = tokio::time::sleep(OFFER_TIMEOUT) => None,
        };
        let refused = match got {
            Some(got) => got.refusal(),
            None => Some(SendRefused::NoAnswer),
        };
        if let Some(refused) = refused {
            let code = if refused == SendRefused::TooLarge { CODE_TOO_LARGE } else { CODE_DECLINED };
            let _ = stream.reset(code.into());
            return Err(refused.into());
        }
    }
    let mut buf = vec![0u8; CHUNK];
    let mut sent = 0u64;
    let mut reported = 0u64;
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n]).await?;
        sent += n as u64;
        if sent - reported >= PROGRESS_EVERY {
            reported = sent;
            progress(sent, header.size);
        }
    }
    if sent != header.size {
        bail!("el archivo cambió mientras se mandaba");
    }
    stream.finish()?;
    // Esperar a que el otro lado lo tenga todo: si no, cerrar la conexión ya lo cortaría.
    stream.stopped().await.ok();
    progress(sent, header.size);
    Ok(())
}

/// Recibe un archivo de `stream` en `dir`. Si ya hay uno con ese nombre, se
/// agrega un número. Devuelve el encabezado y dónde quedó.
pub async fn receive(stream: RecvStream, dir: &Path) -> Result<(FileHeader, PathBuf)> {
    receive_with(stream, dir, |_, _| {}).await
}

/// Como [`receive`], avisando el avance: `progress` recibe el encabezado, los
/// bytes que llegaron y el total (al empezar con 0 y cada [`PROGRESS_EVERY`]).
pub async fn receive_with(mut stream: RecvStream, dir: &Path, progress: impl FnMut(&FileHeader, u64)) -> Result<(FileHeader, PathBuf)> {
    let header = read_header(&mut stream).await?;
    if header.size > MAX_FILE_BYTES {
        bail!("el archivo pasa el tope");
    }
    let path = receive_body(stream, &header, dir, progress).await?;
    Ok((header, path))
}

/// Lo primero de la corriente de un archivo: con esto se decide si se recibe.
pub(crate) async fn read_header(stream: &mut RecvStream) -> Result<FileHeader> {
    read_frame(stream).await?.context("la corriente llegó vacía")
}

/// Corta la corriente de un archivo ofrecido que no se va a recibir.
pub(crate) fn refuse(stream: &mut RecvStream, too_large: bool) {
    let _ = stream.stop(if too_large { CODE_TOO_LARGE } else { CODE_DECLINED }.into());
}

/// El contenido de un archivo cuyo encabezado ya se leyó (y se aceptó), a `dir`.
pub(crate) async fn receive_body(mut stream: RecvStream, header: &FileHeader, dir: &Path, mut progress: impl FnMut(&FileHeader, u64)) -> Result<PathBuf> {
    tokio::fs::create_dir_all(dir).await?;
    // El nombre se reserva creándolo vacío: dos archivos con el mismo nombre
    // que llegan a la vez no se pisan. Los bytes van a un `.part` propio.
    let path = reserve_path(dir, &safe_file_name(&header.name))?;
    let partial = dir.join(format!(".{}.part", crate::ticket::new_token()));
    let result = async {
        let mut file = tokio::fs::File::create(&partial).await?;
        let mut left = header.size;
        let mut buf = vec![0u8; CHUNK];
        let mut reported = 0u64;
        progress(header, 0);
        while left > 0 {
            let want = buf.len().min(left as usize);
            let n = stream.read(&mut buf[..want]).await?.context("se cortó a medias")?;
            file.write_all(&buf[..n]).await?;
            left -= n as u64;
            let got = header.size - left;
            if got - reported >= PROGRESS_EVERY {
                reported = got;
                progress(header, got);
            }
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&partial, &path).await?;
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        let _ = tokio::fs::remove_file(&path).await;
        return Err(e);
    }
    Ok(path)
}

/// `nombre.ext`, o `nombre (2).ext` si ya existe, creado vacío para que nadie
/// más lo tome.
fn reserve_path(dir: &Path, name: &str) -> Result<PathBuf> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let candidates = std::iter::once(dir.join(name)).chain((2..10_000).map(|n| dir.join(format!("{stem} ({n}){ext}"))));
    for path in candidates {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).context("no se pudo crear el archivo"),
        }
    }
    bail!("no queda un nombre libre para {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_nombre_repetido_lleva_numero() {
        let dir = std::env::temp_dir().join(format!("atic-files-{}", crate::ticket::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        // Cada reserva deja el archivo creado: la siguiente ya no lo toma.
        assert_eq!(reserve_path(&dir, "a.txt").unwrap(), dir.join("a.txt"));
        assert_eq!(reserve_path(&dir, "a.txt").unwrap(), dir.join("a (2).txt"));
        assert_eq!(reserve_path(&dir, "a.txt").unwrap(), dir.join("a (3).txt"));
        assert_eq!(reserve_path(&dir, "sin-ext").unwrap(), dir.join("sin-ext"));
        assert_eq!(reserve_path(&dir, "sin-ext").unwrap(), dir.join("sin-ext (2)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
