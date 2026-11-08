//! Archivos y carpetas entre los dos equipos, cada envío por su propia
//! corriente de un solo sentido: un archivo grande no frena el mouse ni el
//! teclado, que van por la corriente de mensajes.
//!
//! La corriente lleva un [`Offer`] encuadrado como cualquier mensaje y detrás
//! los bytes de cada archivo, en el orden de la lista. Las rutas viajan
//! relativas y con `/`; quien recibe las revisa (nada de `..`, de rutas
//! absolutas ni de nombres que su sistema no acepte) antes de escribir.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use iroh::endpoint::{Connection, RecvStream};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::proto::{read_frame, write_frame};

/// Para qué se mandan: decide dónde quedan en el otro equipo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    /// Se copiaron: quedan en una carpeta de Atic y en el portapapeles del otro, listos para pegar.
    Clipboard,
    /// Se arrastraron al borde (o se mandaron): quedan en Descargas.
    Drop,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Relativa a la carpeta de destino, con `/`. La primera parte es el nombre de lo que se mandó.
    pub path: String,
    /// Las carpetas van solas (también las vacías); sus archivos, aparte.
    pub dir: bool,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub purpose: Purpose,
    pub entries: Vec<Entry>,
}

impl Offer {
    pub fn total(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }
}

/// Lo más que se manda de una vez.
pub const MAX_TOTAL: u64 = 20 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const CHUNK: usize = 256 * 1024;

/// La lista de lo que se manda: cada raíz con su nombre y, si es carpeta, todo lo de dentro.
pub fn collect(roots: &[PathBuf], purpose: Purpose) -> Result<(Offer, Vec<PathBuf>)> {
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    for root in roots {
        let name = root.file_name().and_then(|n| n.to_str()).context("un archivo sin nombre")?.to_string();
        walk(root, &name, &mut entries, &mut sources)?;
    }
    let offer = Offer { purpose, entries };
    if offer.entries.len() > MAX_ENTRIES {
        bail!("son más de {MAX_ENTRIES} archivos");
    }
    if offer.total() > MAX_TOTAL {
        bail!("pasa de {} GB", MAX_TOTAL >> 30);
    }
    Ok((offer, sources))
}

fn walk(path: &Path, rel: &str, entries: &mut Vec<Entry>, sources: &mut Vec<PathBuf>) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).with_context(|| format!("no se pudo leer {}", path.display()))?;
    if meta.file_type().is_symlink() {
        // Los enlaces no se siguen: podrían apuntar fuera o en círculo.
        return Ok(());
    }
    if meta.is_dir() {
        entries.push(Entry { path: rel.to_string(), dir: true, size: 0 });
        let mut children: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let Some(name) = child.file_name().to_str().map(str::to_string) else { continue };
            // Lo que el sistema deja de recuerdo (.DS_Store, Thumbs.db) no vale la pena.
            if name == ".DS_Store" || name.eq_ignore_ascii_case("Thumbs.db") {
                continue;
            }
            walk(&child.path(), &format!("{rel}/{name}"), entries, sources)?;
            if entries.len() > MAX_ENTRIES {
                bail!("son más de {MAX_ENTRIES} archivos");
            }
        }
    } else {
        entries.push(Entry { path: rel.to_string(), dir: false, size: meta.len() });
        sources.push(path.to_path_buf());
    }
    Ok(())
}

/// Manda lo de [`collect`] por una corriente nueva de `conn`.
pub async fn send(conn: &Connection, offer: &Offer, sources: &[PathBuf]) -> Result<()> {
    let mut stream = conn.open_uni().await.context("no se pudo abrir la corriente")?;
    write_frame(&mut stream, offer).await?;
    let mut buf = vec![0u8; CHUNK];
    let files = offer.entries.iter().filter(|e| !e.dir);
    for (entry, source) in files.zip(sources) {
        let mut file = tokio::fs::File::open(source).await.with_context(|| format!("no se pudo abrir {}", source.display()))?;
        // Se manda lo que se anunció, ni más ni menos, aunque el archivo cambie mientras tanto.
        let mut left = entry.size;
        while left > 0 {
            let want = (left as usize).min(CHUNK);
            let n = file.read(&mut buf[..want]).await?;
            if n == 0 {
                buf[..want].fill(0);
                stream.write_all(&buf[..want]).await?;
                left -= want as u64;
                continue;
            }
            stream.write_all(&buf[..n]).await?;
            left -= n as u64;
        }
    }
    stream.finish().context("no se pudo cerrar la corriente")?;
    let _ = stream.stopped().await;
    Ok(())
}

/// Recibe un envío. `dest` decide la carpeta según el propósito. Devuelve el
/// propósito y las raíces escritas (lo que se mandó, ya en este equipo).
pub async fn receive(mut stream: RecvStream, dest: impl FnOnce(Purpose) -> Result<PathBuf>) -> Result<(Purpose, Vec<PathBuf>)> {
    let offer: Offer = read_frame(&mut stream).await?.context("envío vacío")?;
    if offer.entries.len() > MAX_ENTRIES || offer.total() > MAX_TOTAL {
        bail!("envío demasiado grande");
    }
    let base = dest(offer.purpose)?;
    std::fs::create_dir_all(&base)?;
    // Cada raíz con un nombre libre en el destino («foto (2).png» si ya hay una).
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut roots = Vec::new();
    let mut buf = vec![0u8; CHUNK];
    for entry in &offer.entries {
        let parts = safe_parts(&entry.path).with_context(|| format!("ruta inválida: {}", entry.path))?;
        let first = &parts[0];
        let root_name = match renames.iter().find(|(from, _)| from == first) {
            Some((_, to)) => to.clone(),
            None => {
                let free = free_name(&base, first);
                renames.push((first.clone(), free.clone()));
                roots.push(base.join(&free));
                free
            }
        };
        let mut path = base.join(root_name);
        for part in &parts[1..] {
            path.push(part);
        }
        if entry.dir {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = tokio::fs::File::create(&path).await.with_context(|| format!("no se pudo crear {}", path.display()))?;
        let mut left = entry.size;
        while left > 0 {
            let want = (left as usize).min(CHUNK);
            stream.read_exact(&mut buf[..want]).await.context("el envío se cortó")?;
            file.write_all(&buf[..want]).await?;
            left -= want as u64;
        }
        file.flush().await?;
    }
    Ok((offer.purpose, roots))
}

/// Las partes de una ruta relativa, si todas son nombres normales y válidos aquí.
fn safe_parts(path: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = path.split('/').map(str::to_string).collect();
    let ok = !parts.is_empty()
        && parts.iter().all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.contains(['\\', ':', '\0'])
                && !(cfg!(windows) && p.contains(['<', '>', '"', '|', '?', '*']))
                && matches!(Path::new(p).components().next(), Some(Component::Normal(_)))
        });
    ok.then_some(parts)
}

/// `name`, o `name (2)`, `name (3)`… el primero que no existe en `dir`.
fn free_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..).map(|n| format!("{stem} ({n}){ext}")).find(|candidate| !dir.join(candidate).exists()).expect("siempre hay un nombre libre")
}

/// Un nombre de carpeta único para cada envío que llega para pegar.
pub fn stamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}-{}", now.as_millis(), std::process::id())
}

/// La carpeta Descargas de este usuario (`ATIC_KVM_DOWNLOADS` la cambia:
/// para probar dos instancias en el mismo equipo y en los tests).
pub fn downloads_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("ATIC_KVM_DOWNLOADS") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).context("sin carpeta de usuario")?;
    Ok(PathBuf::from(home).join("Downloads"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_rutas_raras_se_rechazan() {
        assert!(safe_parts("foto.png").is_some());
        assert!(safe_parts("carpeta/sub/a.txt").is_some());
        for bad in ["", "../x", "a/../../x", "/etc/passwd", "a//b", "C:/x", "a\\b", "./a"] {
            assert!(safe_parts(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn un_nombre_ocupado_pasa_a_ser_con_numero() {
        let dir = std::env::temp_dir().join(format!("atic-kvm-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("foto.png"), b"x").unwrap();
        std::fs::write(dir.join("foto (2).png"), b"x").unwrap();
        assert_eq!(free_name(&dir, "foto.png"), "foto (3).png");
        assert_eq!(free_name(&dir, "otra.png"), "otra.png");
        std::fs::create_dir_all(dir.join("Carpeta")).unwrap();
        assert_eq!(free_name(&dir, "Carpeta"), "Carpeta (2)");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn una_carpeta_se_lista_entera() {
        let dir = std::env::temp_dir().join(format!("atic-kvm-walk-{}", std::process::id()));
        let root = dir.join("Proyecto");
        std::fs::create_dir_all(root.join("vacia")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(root.join(".DS_Store"), b"basura").unwrap();
        let (offer, sources) = collect(std::slice::from_ref(&root), Purpose::Drop).unwrap();
        let paths: Vec<_> = offer.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["Proyecto", "Proyecto/src", "Proyecto/src/main.rs", "Proyecto/vacia"]);
        assert_eq!(sources, [root.join("src/main.rs")]);
        assert_eq!(offer.total(), 12);
        let _ = std::fs::remove_dir_all(dir);
    }
}
