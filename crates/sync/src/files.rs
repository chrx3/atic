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
    let path = free_path(dir, &safe_file_name(&header.name));
    let partial = path.with_extension(format!(
        "{}part",
        path.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
    ));
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
        anyhow::Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(e);
    }
    tokio::fs::rename(&partial, &path).await?;
    Ok((header, path))
}

/// `nombre.ext`, o `nombre (2).ext` si ya existe.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("siempre hay un número libre")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_nombre_repetido_lleva_numero() {
        let dir = std::env::temp_dir().join(format!("atic-files-{}", crate::ticket::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::write(dir.join("a (2).txt"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (3).txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
