# Audio del sistema en macOS

**Estado:** `implementado` (ScreenCaptureKit, macOS 13+)

## Resumen

En Windows, Atic captura mic + loopback del sistema (WASAPI). En macOS usa
**ScreenCaptureKit** (`SCStream` con `capturesAudio`) sobre un display y sólo
consume la salida de audio. Misma experiencia que Windows: pistas mic / both /
sistema y medidor de "Otros".

SCK pide **Grabación de pantalla** y enciende el indicador de captura del
sistema mientras dura la grabación. En macOS 11/12 la pista de sistema no está
disponible y la UI muestra ese mensaje.

## Código

- [`crates/audio/src/screencapturekit.rs`](../crates/audio/src/screencapturekit.rs) — stream, callback `SCStreamOutput`, conversión `CMSampleBuffer` → f32 y writer con línea de tiempo (rellena silencio para no desalinear la pista del mic).
- [`crates/audio/src/lib.rs`](../crates/audio/src/lib.rs) — `try_start_system_stream` elige WASAPI o SCK; `SystemStream` encapsula el guard.
- [`apps/desktop/src-tauri/Info.plist`](../apps/desktop/src-tauri/Info.plist) — `NSScreenCaptureUsageDescription`.
- [`docs/MACOS.md`](../docs/MACOS.md) — permisos y desarrollo.

## Pendiente / siguiente

- [x] Implementar captura ScreenCaptureKit
- [x] Permisos TCC documentados para el usuario
- [ ] Probar Meet / Zoom / Teams en Mac con loopback
- [ ] Evaluar CoreAudio Process Tap (macOS 14.2+) para capturar sólo la reunión

## Relacionado

- [grabacion-reuniones.md](grabacion-reuniones.md)
- [`docs/MACOS.md`](../docs/MACOS.md)
