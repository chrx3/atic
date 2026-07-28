# Grabación de reuniones

**Estado:** `parcial`

## Resumen

Graba audio de reuniones (micrófono y, en Windows, audio del sistema) para
transcribir y resumir después. Incluye detección de llamadas y modo
parlantes.

## Cómo se usa

- Desde la pill: iniciar / detener grabación.
- Detección automática (Meet en navegador, mic en uso, etc.) según ajustes.
- Pistas “yo” / “otros” cuando el backend lo permite.
- En macOS hoy solo micrófono (sin loopback del sistema).

## Código

- [`crates/audio/`](../crates/audio/) — captura WASAPI (Windows); stub macOS
- [`apps/desktop/src-tauri/src/meeting_detection.rs`](../apps/desktop/src-tauri/src/meeting_detection.rs)
- [`apps/desktop/src-tauri/src/commands.rs`](../apps/desktop/src-tauri/src/commands.rs) — comandos de grabación
- [`crates/core/`](../crates/core/) — storage de grabaciones y config

## Pendiente / siguiente

- [ ] Audio del sistema en macOS (ScreenCaptureKit) — ver [macos-audio-sistema.md](macos-audio-sistema.md)
- [ ] Seguir mejorando detección de reuniones en navegador

## Relacionado

- [transcripcion-resumen.md](transcripcion-resumen.md)
- [macos-audio-sistema.md](macos-audio-sistema.md)
- [`docs/MACOS.md`](../docs/MACOS.md)
