//! `PILL_DEV=1`: una pill de desarrollo al lado de la de siempre.
//!
//! Sin esto la segunda pill ni arranca (`single_instance`). Con esto convive:
//! - Su propio candado y su propio lugar guardado (`home-dev.txt`), que por
//!   omisión es un cuarto del borde y no el centro, donde está la otra.
//! - Sin atajos: ni los registrados ni la rueda ni el dictado (se pelearían
//!   con los de la otra o dispararían dos veces). Se prueba con el mouse.
//! - No guarda lo que copias ni sincroniza el celular: lo hace la otra, y
//!   saldría todo dos veces. Sí lee los mismos datos y el mismo `config.json`.
//! - No se actualiza sola ni toca el inicio con Windows.
//! - Un punto naranjo junto a la cara, para no confundirlas.

use std::sync::OnceLock;

/// Dónde va el notch a lo largo del borde de arriba (0 a 1): al centro, o a
/// un cuarto la de desarrollo, para no quedar encima de la otra. Vale para
/// su lugar por omisión y para adonde vuela al abrir una herramienta.
pub fn notch_share() -> f32 {
    if on() {
        0.25
    } else {
        0.5
    }
}

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("PILL_DEV").is_ok_and(|v| !v.is_empty() && v != "0"))
}
