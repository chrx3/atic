//! Escribir un archivo entero, o no escribirlo.
//!
//! # Por qué no alcanza `fs::write`
//!
//! `fs::write` trunca el archivo y **después** escribe. Entre esas dos cosas el
//! archivo existe con cero bytes, y si el proceso muere ahí —cierre forzado,
//! batería, un pánico en otro hilo— eso es lo que queda en disco para siempre.
//!
//! No es teórico para Atic: `config.json` se reescribe con cada cambio en
//! Ajustes, y `Config::load` está escrito para *no fallar* ante un JSON roto
//! —devuelve los valores por defecto—, así que el usuario no vería un error:
//! vería su configuración en blanco, sin ninguna pista de qué pasó. Lo mismo
//! vale para una transcripción o un resumen, que además no se pueden rehacer
//! sin volver a gastar el modelo.
//!
//! # Cómo se resuelve
//!
//! Se escribe un `.tmp` al lado, se lo baja a disco, y recién ahí se lo mueve
//! encima del destino. `rename` sobre el mismo volumen es atómico tanto en
//! NTFS como en APFS: cualquier lector ve el archivo viejo entero o el nuevo
//! entero, nunca uno a medias. El `.tmp` va en el **mismo directorio** por eso:
//! desde el temporal del sistema, `rename` cruzaría volúmenes y dejaría de ser
//! atómico para pasar a ser copiar y borrar.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Escribe `bytes` en `path` de forma atómica.
///
/// El `sync_all` antes del `rename` no es de más: sin él el sistema puede tener
/// el rename aplicado y el contenido todavía en caché, y un corte de luz deja
/// el archivo nuevo lleno de ceros. Es el modo de fallo clásico de este patrón
/// cuando se lo escribe apurado.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;

    // El nombre del temporal cuelga del destino para que dos escrituras a
    // archivos distintos del mismo directorio no se pisen.
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("atic-tmp");
    let tmp = dir.join(format!(".{name}.tmp"));

    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    // En Windows `rename` reemplaza el destino (`MOVEFILE_REPLACE_EXISTING`),
    // igual que en Unix. Si falla, el original sigue intacto: es justamente lo
    // que se quería.
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // No dejar basura si el movimiento no salió.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Igual que [`write_atomic`], para texto.
pub fn write_atomic_str(path: &Path, text: &str) -> std::io::Result<()> {
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_a_new_file() {
        let dir = temp_dir();
        let path = dir.join("nuevo.json");
        write_atomic_str(&path, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replaces_an_existing_file() {
        let dir = temp_dir();
        let path = dir.join("existente.json");
        std::fs::write(&path, "viejo").unwrap();
        write_atomic_str(&path, "nuevo").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "nuevo");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leaves_no_temporary_behind() {
        let dir = temp_dir();
        write_atomic_str(&dir.join("a.json"), "1").unwrap();
        let sobrantes: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(sobrantes.is_empty(), "quedó basura: {sobrantes:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn creates_the_parent_directory() {
        let dir = temp_dir();
        let path = dir.join("sub").join("hondo").join("x.json");
        write_atomic_str(&path, "ok").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ok");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Dos archivos del mismo directorio no comparten temporal: si lo
    /// compartieran, escribirlos a la vez dejaría uno con el contenido del otro.
    #[test]
    fn different_targets_use_different_temporaries() {
        let dir = temp_dir();
        write_atomic_str(&dir.join("uno.json"), "1").unwrap();
        write_atomic_str(&dir.join("dos.json"), "2").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("uno.json")).unwrap(), "1");
        assert_eq!(std::fs::read_to_string(dir.join("dos.json")).unwrap(), "2");
        std::fs::remove_dir_all(&dir).ok();
    }
}
