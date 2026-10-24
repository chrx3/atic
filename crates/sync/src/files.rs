//! Archivos entre el celular y el PC, cada uno por su propia corriente.
//!
//! La corriente de mensajes lleva cosas chicas; un archivo de cientos de MB la
//! frenaría y tendría que pasar entero por la memoria. Acá cada archivo abre
//! una corriente de un solo sentido: primero un [`FileHeader`] encuadrado como
//! cualquier mensaje y detrás los bytes, que van y vienen del disco por partes.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use iroh::endpoint::{Connection, RecvStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::proto::{read_frame, safe_file_name, write_frame, FileHeader, MAX_FILE_BYTES};

const CHUNK: usize = 64 * 1024;
/// Cada cuánto se avisa el avance: más seguido no se nota y cuesta.
const PROGRESS_EVERY: u64 = 512 * 1024;

/// Manda `path` por una corriente nueva de `conn`. `progress` recibe los bytes
/// enviados y el total.
pub async fn send(conn: &Connection, header: FileHeader, path: &Path, mut progress: impl FnMut(u64, u64)) -> Result<()> {
    if header.size > MAX_FILE_BYTES {
        bail!("el archivo pasa el tope de {} MB", MAX_FILE_BYTES >> 20);
    }
    let mut file = tokio::fs::File::open(path).await.context("no se pudo abrir el archivo")?;
    let mut stream = conn.open_uni().await.context("no se pudo abrir la corriente")?;
    write_frame(&mut stream, &header).await?;
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
pub async fn receive(mut stream: RecvStream, dir: &Path) -> Result<(FileHeader, PathBuf)> {
    let header: FileHeader = read_frame(&mut stream).await?.context("la corriente llegó vacía")?;
    if header.size > MAX_FILE_BYTES {
        bail!("el archivo pasa el tope");
    }
    tokio::fs::create_dir_all(dir).await?;
    // El nombre se reserva creándolo vacío: dos archivos con el mismo nombre
    // que llegan a la vez no se pisan. Los bytes van a un `.part` propio.
    let path = reserve_path(dir, &safe_file_name(&header.name))?;
    let partial = dir.join(format!(".{}.part", crate::ticket::new_token()));
    let result = async {
        let mut file = tokio::fs::File::create(&partial).await?;
        let mut left = header.size;
        let mut buf = vec![0u8; CHUNK];
        while left > 0 {
            let want = buf.len().min(left as usize);
            let n = stream.read(&mut buf[..want]).await?.context("se cortó a medias")?;
            file.write_all(&buf[..n]).await?;
            left -= n as u64;
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
    Ok((header, path))
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
