# Audio del sistema en macOS

**Estado:** `parcial` (andamiaje) / objetivo `en curso` cuando haya Mac

## Resumen

En Windows, Atic captura mic + loopback del sistema. En macOS hoy solo el
micrófono. La fase 4 es capturar audio del sistema vía ScreenCaptureKit.

## Cómo se usa

- Hoy: grabar con pistas mic / both cae a mic en Mac (aviso esperado).
- Objetivo: misma experiencia que Windows en reuniones.

## Código

- [`crates/audio/`](../crates/audio/) — stub / notas fase 4
- [`apps/desktop/src-tauri/src/macos_notes.rs`](../apps/desktop/src-tauri/src/macos_notes.rs)
- [`docs/MACOS.md`](../docs/MACOS.md)

## Pendiente / siguiente

- [ ] Implementar captura ScreenCaptureKit en Mac real
- [ ] Permisos TCC / entitlement documentados para el usuario
- [ ] Probar Meet / Zoom / Teams en Mac con loopback

## Relacionado

- [grabacion-reuniones.md](grabacion-reuniones.md)
- [`docs/MACOS.md`](../docs/MACOS.md)
