//! Tomar un lock sin que un pánico ajeno se lleve puesto el subsistema.
//!
//! # Qué problema resuelve
//!
//! Si un hilo entra en pánico con un `Mutex` tomado, Rust lo marca
//! **envenenado**, y a partir de ahí *todo* `lock().unwrap()` sobre ese mutex
//! panica también. En una biblioteca eso es prudente; en una app de escritorio
//! es un efecto dominó: un pánico en el hilo del portapapeles no rompe el
//! portapapeles, rompe la pill, el historial y el pegado —todo lo que comparta
//! ese lock— y el usuario no tiene más salida que reiniciar Atic.
//!
//! El veneno no dice que los datos estén mal. Dice que *podrían* estarlo,
//! porque hubo un pánico a mitad de una modificación. Para lo que Atic guarda
//! detrás de un mutex —una lista de sesiones, la posición de una ventana, una
//! caché de modelos— seguir con el estado que haya es mejor que apagarlo todo:
//! lo peor que sale es un ítem raro, y la alternativa es un subsistema muerto
//! hasta reiniciar.
//!
//! Donde un estado a medias sí importaría —lo que se persiste— la protección
//! no es esta: es que la escritura sea atómica (ver [`crate::fs_atomic`]).

use std::sync::{Mutex, MutexGuard};

/// Tomar el lock ignorando el veneno.
pub trait MutexExt<T> {
    /// Como `lock().unwrap()`, pero si el mutex quedó envenenado por un pánico
    /// en otro hilo devuelve igual el dato en vez de propagar el pánico.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            // Vale la pena que quede en el log: significa que hubo un pánico
            // antes, en algún lado, y probablemente nadie lo vio.
            tracing::warn!(
                "mutex envenenado por un pánico anterior; se continúa con el estado que haya"
            );
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn behaves_like_lock_when_healthy() {
        let m = Mutex::new(5);
        assert_eq!(*m.lock_or_recover(), 5);
    }

    #[test]
    fn keeps_working_after_a_panic_poisoned_it() {
        let m = Arc::new(Mutex::new(vec![1, 2, 3]));
        let clone = m.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.lock().unwrap();
            panic!("pánico a propósito");
        })
        .join();

        assert!(m.lock().is_err(), "el mutex tendría que estar envenenado");
        // El punto del trait: esto no panica, y los datos siguen ahí.
        assert_eq!(*m.lock_or_recover(), vec![1, 2, 3]);
    }

    #[test]
    fn still_allows_writing_after_recovery() {
        let m = Arc::new(Mutex::new(0));
        let clone = m.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.lock().unwrap();
            panic!("pánico a propósito");
        })
        .join();

        *m.lock_or_recover() = 42;
        assert_eq!(*m.lock_or_recover(), 42);
    }
}
