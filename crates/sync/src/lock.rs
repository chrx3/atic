//! Tomar un lock aunque un pánico anterior lo haya envenenado.
//!
//! Con `lock().unwrap()`, un pánico con el lock tomado mata a todas las tareas
//! del canal que comparten ese mutex, hasta reiniciar. Lo que hay detrás de
//! estos locks (equipos conocidos, conexiones vivas, colas) sirve igual tal
//! como quedó. Es el `lock_or_recover` de `atic-core`, copiado acá porque este
//! crate también se compila para Android y no depende de él.

use std::sync::{Mutex, MutexGuard};

pub(crate) trait MutexExt<T> {
    /// Como `lock().unwrap()`, pero sin propagar el pánico de otro hilo.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("mutex envenenado por un pánico anterior; se continúa con el estado que haya");
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn sigue_andando_despues_de_un_panico_con_el_lock_tomado() {
        let shared = Arc::new(Mutex::new(vec![1, 2, 3]));
        let clone = shared.clone();
        let _ = std::thread::spawn(move || {
            let _guard = clone.lock().unwrap();
            panic!("pánico a propósito");
        })
        .join();

        assert!(shared.lock().is_err(), "el mutex tendría que estar envenenado");
        assert_eq!(*shared.lock_or_recover(), vec![1, 2, 3]);
        shared.lock_or_recover().push(4);
        assert_eq!(shared.lock_or_recover().len(), 4);
    }
}
