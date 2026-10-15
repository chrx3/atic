//! Arrastrar texto y archivos hacia otras apps. Pendiente en macOS (irá con
//! `NSDraggingSession`): por ahora el arrastre falla y quien llama usa su
//! respaldo (copiar o pegar).

/// Cómo terminó el arrastre (ver `win32/drag.rs`).
pub struct DragOutcome {
    pub dropped: bool,
    pub effect: u32,
}

pub fn drag_text(_text: &str) -> Result<DragOutcome, String> {
    Err("arrastrar todavía no está en macOS".into())
}

pub fn drag_files(_paths: &[String]) -> Result<DragOutcome, String> {
    Err("arrastrar todavía no está en macOS".into())
}
