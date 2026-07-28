# Capturas de pantalla

**Estado:** `hecho`

## Resumen

Captura ventana, región o monitor desde Atic, con overlay, historial de
capturas y uso desde clipboard / agentes (adjuntar imagen).

## Cómo se usa

- Desde la pill o el flujo de captura: elegir región / ventana / monitor.
- Overlay para delimitar.
- Las capturas viven bajo el directorio de datos de Atic y aparecen en
  historial / clipboard según el flujo.
- Se pueden adjuntar al compositor de agentes.

## Código

- [`crates/capture/`](../crates/capture/) — motor de captura (Windows)
- [`apps/desktop/src-tauri/src/capture.rs`](../apps/desktop/src-tauri/src/capture.rs)
- [`apps/desktop/src-tauri/src/capture_session.rs`](../apps/desktop/src-tauri/src/capture_session.rs)
- [`apps/desktop/src-tauri/src/capture_shelf.rs`](../apps/desktop/src-tauri/src/capture_shelf.rs)
- [`apps/desktop/src/routes/capture-overlay/+page.svelte`](../apps/desktop/src/routes/capture-overlay/+page.svelte)

## Pendiente / siguiente

- [ ] Upgrade futuro a Windows Graphics Capture (ver plan §23)
- [ ] Paridad / stub macOS si se prioriza

## Relacionado

- [clipboard-historial.md](clipboard-historial.md)
- [agentes.md](agentes.md)
- [`docs/PLAN_HERRAMIENTA_CAPTURAS.md`](../docs/PLAN_HERRAMIENTA_CAPTURAS.md)
