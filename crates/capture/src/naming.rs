//! Nombres de archivo de capturas: `capture_<fecha>_<hora>_<id>.png`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::NaiveDateTime;

/// Desambigua capturas creadas dentro del mismo segundo.
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Sufijo corto (6 hex) para el nombre de archivo. Combina los nanosegundos
/// actuales con un contador de proceso para evitar colisiones en ráfaga sin
/// añadir una dependencia de números aleatorios.
pub fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = nanos ^ counter.wrapping_mul(2_654_435_761);
    format!("{:06x}", mixed & 0x00FF_FFFF)
}

/// Formatea el nombre de archivo a partir de una fecha/hora local y un id.
/// Separado de `new_capture_filename` para poder probarlo con una fecha fija.
pub fn format_capture_filename(when: &NaiveDateTime, id: &str) -> String {
    format!("capture_{}_{}.png", when.format("%Y-%m-%d_%H-%M-%S"), id)
}

/// Nombre de archivo para una captura nueva, con la hora local actual.
pub fn new_capture_filename() -> String {
    let now = chrono::Local::now().naive_local();
    format_capture_filename(&now, &new_id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn formats_expected_pattern() {
        let when = NaiveDate::from_ymd_opt(2026, 7, 17)
            .unwrap()
            .and_hms_opt(14, 32, 8)
            .unwrap();
        assert_eq!(
            format_capture_filename(&when, "a1b2c3"),
            "capture_2026-07-17_14-32-08_a1b2c3.png"
        );
    }

    #[test]
    fn new_id_is_six_hex_chars() {
        let id = new_id();
        assert_eq!(id.len(), 6);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn consecutive_ids_differ() {
        // El contador garantiza que dos ids seguidos no colisionen.
        assert_ne!(new_id(), new_id());
    }
}
