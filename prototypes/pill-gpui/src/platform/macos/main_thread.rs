//! Llevar trabajo al hilo principal (el de AppKit y GPUI) desde cualquier
//! hilo, con la cola principal de libdispatch.

use std::ffi::c_void;

extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

type Work = Box<dyn FnOnce() + Send>;

extern "C" fn trampoline(context: *mut c_void) {
    // SAFETY: `context` es el `Box` armado en `run`, usado una sola vez.
    let work = unsafe { Box::from_raw(context.cast::<Work>()) };
    work();
}

/// Corre `work` en el hilo principal, en la próxima vuelta de su loop.
pub fn run(work: impl FnOnce() + Send + 'static) {
    let context = Box::into_raw(Box::new(Box::new(work) as Work));
    // SAFETY: la cola principal existe todo el proceso; `trampoline` libera
    // el contexto.
    unsafe { dispatch_async_f(&raw const _dispatch_main_q, context.cast(), trampoline) };
}

/// Como `run`, pero espera el resultado. No llamar desde el hilo principal.
pub fn sync<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    if objc2::MainThreadMarker::new().is_some() {
        return Some(work());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    run(move || {
        let _ = tx.send(work());
    });
    rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
}
