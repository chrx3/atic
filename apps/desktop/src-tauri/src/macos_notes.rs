//! Notas de implementación macOS (fase 4).
//!
//! El audio del sistema en macOS no es loopback WASAPI. Opciones:
//! - ScreenCaptureKit (macOS 13+) con filtro de audio de pantalla/app
//! - Core Audio taps (macOS 14.4+)
//!
//! Mientras no esté implementado, `try_start_system_stream` en
//! `crates/audio` falla de forma controlada y la app graba solo micrófono
//! (o falla si el usuario eligió solo «otros» / modo parlantes).
//!
//! ## Checklist de implementación
//!
//! 1. Binding Swift/ObjC o crate `screencapturekit` / `cidre` desde `crates/audio`.
//! 2. Pedir permiso de captura de pantalla/audio antes de abrir el stream.
//! 3. Escribir PCM a WAV con el mismo contrato que WASAPI (`system.wav`).
//! 4. Respetar `CaptureConfig.capture_system` (modo parlantes / solo otros).
//! 5. Firma + notarización para distribución fuera de desarrollo.
//!
//! Permisos TCC (ver `Info.plist`):
//! - NSMicrophoneUsageDescription
//! - NSAudioCaptureUsageDescription / NSScreenCaptureUsageDescription

#![allow(dead_code)]

/// Placeholder: comprobar permisos de micrófono / captura de audio.
pub fn permissions_needed() -> &'static [&'static str] {
    &[
        "micrófono",
        "captura de audio del sistema (ScreenCaptureKit)",
    ]
}

/// Estado documentado de la fase 4 (útil para UI de onboarding futura).
pub fn phase4_status() -> &'static str {
    "andamiaje: Info.plist + stub; falta captura real ScreenCaptureKit"
}
