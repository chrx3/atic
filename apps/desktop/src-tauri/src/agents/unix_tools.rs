//! Los comandos Unix que Atic pone en sus consolas.
//!
//! # Qué resuelve
//!
//! Una consola en Windows deja al usuario en `cmd`, donde `ls` no existe. La
//! consola la abre Atic, así que las herramientas las pone Atic: con el
//! sidecar `atic-unix` al lado, `ls`, `cat`, `head` y compañía funcionan sin
//! que nadie instale nada.
//!
//! # Cómo
//!
//! `atic-unix` es un binario multiuso que mira su `argv[0]`. Acá se le crea
//! **un enlace duro por comando** en el directorio de datos —`ls.exe`,
//! `cat.exe`…— y esa carpeta se antepone al PATH de la consola.
//!
//! Enlaces duros y no copias: son el mismo archivo en disco, así que veinte
//! comandos ocupan lo que uno. Si el sistema de archivos no los admite —una
//! unidad de red, por ejemplo— se copia, que funciona igual y solo cuesta
//! espacio. Y no shims `.cmd`: un guion se lleva mal con las comillas y
//! `CreateProcess` no lo lanza directo.

use std::path::{Path, PathBuf};

/// Los nombres que se enlazan. Gemelo de la tabla de `atic-unix`: si se suma
/// uno allá, va acá; si sobra uno acá, el enlace existe y no hace nada útil.
///
/// No están `grep`, `sed`, `awk` ni `find`: no son coreutils y uutils no los
/// trae.
pub const COMANDOS: &[&str] = &[
    "basename", "cat", "cp", "cut", "dirname", "du", "env", "head", "ls", "mkdir", "mv", "nl",
    "realpath", "rm", "rmdir", "seq", "sort", "tac", "tail", "tee", "touch", "tr", "uniq", "wc",
];

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Dónde está `atic-unix`: al lado del ejecutable instalado, o en `target/`
/// cuando se corre en dev. Misma regla que el sidecar del hub.
pub fn sidecar() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let junto = dir.join(exe_name("atic-unix"));
            if junto.is_file() {
                return Some(junto);
            }
        }
    }
    if cfg!(debug_assertions) {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut mejor: Option<(PathBuf, std::time::SystemTime)> = None;
        for perfil in ["debug", "release"] {
            let p = manifest
                .join("..")
                .join("..")
                .join("..")
                .join("target")
                .join(perfil)
                .join(exe_name("atic-unix"));
            if let Ok(meta) = std::fs::metadata(&p) {
                let t = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                if mejor.as_ref().is_none_or(|(_, best)| t > *best) {
                    mejor = Some((p, t));
                }
            }
        }
        if let Some((p, _)) = mejor {
            return Some(p);
        }
    }
    None
}

/// Crea (o repara) los enlaces y devuelve la carpeta que va al PATH.
///
/// Idempotente y barato: si el enlace ya apunta al binario vigente no se toca.
/// Se rehace cuando el sidecar cambió —una actualización de Atic deja el
/// binario nuevo y los enlaces viejos apuntando al archivo anterior—.
pub fn preparar(dir_datos: &Path) -> Result<PathBuf, String> {
    let origen = sidecar().ok_or_else(|| {
        "No se encontró atic-unix. En dev, ejecuta `pnpm unix:build` primero.".to_string()
    })?;
    let destino = dir_datos.join("unix-bin");
    std::fs::create_dir_all(&destino)
        .map_err(|e| format!("no se pudo crear {}: {e}", destino.display()))?;

    let referencia = std::fs::metadata(&origen)
        .and_then(|m| m.modified())
        .ok();

    for nombre in COMANDOS {
        let enlace = destino.join(exe_name(nombre));
        if vigente(&enlace, referencia) {
            continue;
        }
        // Un enlace viejo hay que quitarlo: `hard_link` no pisa lo que existe.
        let _ = std::fs::remove_file(&enlace);
        if std::fs::hard_link(&origen, &enlace).is_err() {
            std::fs::copy(&origen, &enlace)
                .map_err(|e| format!("no se pudo poner {}: {e}", enlace.display()))?;
        }
    }
    Ok(destino)
}

/// ¿Este enlace ya es del binario vigente? Sin fecha de referencia no se
/// arriesga: se rehace.
fn vigente(enlace: &Path, referencia: Option<std::time::SystemTime>) -> bool {
    let (Some(referencia), Ok(meta)) = (referencia, std::fs::metadata(enlace)) else {
        return false;
    };
    meta.modified().map(|t| t >= referencia).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_lista_no_trae_lo_que_uutils_no_tiene() {
        // Estos cuatro son los que la gente busca primero y NO son coreutils:
        // dejarlos en la lista crearía un `grep.exe` que no sabe hacer grep.
        for ausente in ["grep", "sed", "awk", "find"] {
            assert!(!COMANDOS.contains(&ausente), "«{ausente}» no lo trae uutils");
        }
        assert!(COMANDOS.contains(&"ls"));
    }

    #[test]
    fn los_nombres_no_se_repiten() {
        let mut vistos = std::collections::HashSet::new();
        for c in COMANDOS {
            assert!(vistos.insert(*c), "«{c}» está dos veces");
        }
    }

    #[test]
    fn en_windows_los_enlaces_llevan_extension() {
        if cfg!(windows) {
            assert_eq!(exe_name("ls"), "ls.exe");
        } else {
            assert_eq!(exe_name("ls"), "ls");
        }
    }

    #[test]
    fn un_enlace_que_no_existe_nunca_esta_vigente() {
        let ninguno = std::env::temp_dir().join("atic-unix-no-existe.exe");
        assert!(!vigente(&ninguno, Some(std::time::SystemTime::now())));
    }

    #[test]
    fn sin_fecha_de_referencia_se_rehace() {
        // Mejor rehacer de más que dejar un enlace apuntando al binario viejo.
        let temp = std::env::temp_dir().join(format!("atic-unix-{}.tmp", std::process::id()));
        std::fs::write(&temp, b"x").unwrap();
        assert!(!vigente(&temp, None));
        let _ = std::fs::remove_file(&temp);
    }

    #[test]
    fn preparar_crea_un_enlace_por_comando() {
        let Some(_) = sidecar() else {
            // Sin el sidecar compilado no hay nada que enlazar; el test que
            // importa es el de arriba y este se salta en un checkout limpio.
            return;
        };
        let dir = std::env::temp_dir().join(format!("atic-unix-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = preparar(&dir).expect("se pudo preparar");
        for c in COMANDOS {
            assert!(bin.join(exe_name(c)).is_file(), "falta {c}");
        }
        // Repetirlo no falla ni duplica: es idempotente.
        preparar(&dir).expect("segunda vez");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
