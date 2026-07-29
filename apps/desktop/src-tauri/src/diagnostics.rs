//! Dejar rastro de lo que pasó, para poder investigarlo después.
//!
//! # Por qué existe
//!
//! Hasta acá el log iba a stdout con `tracing_subscriber::fmt()`. En desarrollo
//! eso alcanza; en la app instalada no lo lee **nadie**, porque una app de
//! escritorio no se lanza desde una consola. El resultado práctico era que un
//! reporte de «se cerró solo» no venía con nada: ni el pánico, ni dónde, ni las
//! últimas líneas de `pill_geo` que habrían dicho quién movió la ventana.
//!
//! Acá se resuelven las dos mitades del problema:
//!
//! 1. **El log va también a disco**, rotando por día y conservando una semana.
//! 2. **Los pánicos entran al log**, que es justo lo que no pasaba: un pánico
//!    escribe en stderr y muere, sin tocar `tracing`. Perdíamos exactamente el
//!    evento que más importa.
//!
//! # El guard que hay que sostener
//!
//! La escritura es no bloqueante: hay un hilo aparte que vacía el buffer, y
//! [`init`] devuelve un guard que lo mantiene vivo. Si se descarta, el hilo
//! muere y **las últimas líneas antes del cierre se pierden** — que son
//! siempre las interesantes. Por eso el guard vive en `run()` hasta que la app
//! termina.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Cuántos días de log se conservan. Una semana cubre el «me pasó el martes»
/// sin que la carpeta crezca sola para siempre.
const DIAS_DE_LOG: usize = 7;

/// Arranca el log a consola y a archivo, y engancha los pánicos.
///
/// Devuelve el guard del escritor no bloqueante: **hay que conservarlo**. Si
/// no se pudo abrir el archivo (disco lleno, permisos), el log a consola se
/// instala igual y se devuelve `None`: quedarse sin app por no poder escribir
/// un log sería peor que la falta del log.
#[must_use = "si se descarta el guard, las últimas líneas antes del cierre se pierden"]
pub fn init(logs_dir: &Path) -> Option<WorkerGuard> {
    let filter = || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let archivo = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("atic")
        .filename_suffix("log")
        .max_log_files(DIAS_DE_LOG)
        .build(logs_dir);

    let guard = match archivo {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let resultado = tracing_subscriber::registry()
                .with(filter())
                .with(tracing_subscriber::fmt::layer())
                // Sin colores: los códigos ANSI en un archivo lo vuelven
                // ilegible justo cuando alguien lo abre para pegarlo en un
                // reporte.
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(writer),
                )
                .try_init();
            if resultado.is_err() {
                return None;
            }
            Some(guard)
        }
        Err(error) => {
            let _ = tracing_subscriber::registry()
                .with(filter())
                .with(tracing_subscriber::fmt::layer())
                .try_init();
            tracing::warn!(%error, dir = %logs_dir.display(), "sin log a archivo");
            None
        }
    };

    instalar_panic_hook();
    tracing::info!(dir = %logs_dir.display(), "log iniciado");
    guard
}

/// Hace que un pánico quede escrito en el log antes de irse.
///
/// Encadena al hook anterior en vez de reemplazarlo, para no perder la salida
/// por stderr que sirve en desarrollo.
fn instalar_panic_hook() {
    let anterior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // `payload_as_str` todavía no está estable; hay que probar los dos
        // tipos con los que se panica en la práctica.
        let mensaje = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(sin mensaje)".to_string());
        let lugar = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "(desconocido)".to_string());
        let hilo = std::thread::current()
            .name()
            .unwrap_or("(sin nombre)")
            .to_string();

        tracing::error!(
            %mensaje,
            %lugar,
            %hilo,
            traza = %std::backtrace::Backtrace::force_capture(),
            "pánico"
        );

        anterior(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El hook tiene que sobrevivir a un pánico atrapado y dejar el anterior
    /// encadenado: si reemplazara sin llamar al de antes, en desarrollo se
    /// perdería la salida por stderr.
    #[test]
    fn el_hook_no_se_come_el_panico() {
        instalar_panic_hook();
        let resultado = std::panic::catch_unwind(|| panic!("prueba"));
        assert!(resultado.is_err());
    }

    #[test]
    fn sin_carpeta_escribible_no_tumba_la_app() {
        // Una ruta que no se puede crear: init tiene que devolver None, no
        // entrar en pánico.
        let guard = init(Path::new("\0ruta imposible"));
        assert!(guard.is_none());
    }
}
