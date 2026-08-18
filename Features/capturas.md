# Capturas de pantalla

**Estado:** `hecho`

## Resumen

Captura ventana, región o monitor desde Atic, con overlay, historial de
capturas, **editor para dibujar encima** y uso desde clipboard / agentes
(adjuntar imagen).

## Cómo se usa

- Desde la pill o el flujo de captura: elegir región / ventana / monitor.
- Overlay para delimitar.
- Las capturas viven bajo el directorio de datos de Atic y aparecen en
  historial / clipboard según el flujo.
- Se pueden adjuntar al compositor de agentes.

### Dibujar encima

Botón **Dibujar** en el estante, o clic en la miniatura si en Ajustes →
Capturas está elegido «Dibujar encima» en vez de la vista previa.

- Lápiz, flecha, círculo, rectángulo y resaltador (teclas `1`–`5`).
- Seis colores y tres grosores. El grosor **escala con el tamaño de la
  imagen**: un trazo fijo se ve como un pelo en una captura 4K.
- `Ctrl+Z` / `Ctrl+Shift+Z`, `Enter` copia, `Ctrl+Enter` guarda, `Esc` cierra
  (pide confirmar si hay trazos). Guardar no es `Ctrl+S` porque
  [`desktopChrome.ts`](../apps/desktop/src/lib/desktopChrome.ts) se lo traga
  en fase de captura: es el «guardar página» del navegador.
- Guardar **no pisa el original**: escribe una captura nueva y la muestra en el
  estante, igual que cualquier captura recién tomada.

## Código

- [`crates/capture/`](../crates/capture/) — motor de captura (Windows)
- [`apps/desktop/src-tauri/src/capture.rs`](../apps/desktop/src-tauri/src/capture.rs)
- [`apps/desktop/src-tauri/src/capture_session.rs`](../apps/desktop/src-tauri/src/capture_session.rs)
- [`apps/desktop/src-tauri/src/capture_shelf.rs`](../apps/desktop/src-tauri/src/capture_shelf.rs)
- [`apps/desktop/src-tauri/src/annotate.rs`](../apps/desktop/src-tauri/src/annotate.rs)
  — ventana del editor, tamaño y salidas (guardar / portapapeles)
- [`apps/desktop/src/routes/capture-overlay/+page.svelte`](../apps/desktop/src/routes/capture-overlay/+page.svelte)
- [`apps/desktop/src/lib/surfaces/annotate/`](../apps/desktop/src/lib/surfaces/annotate/)
  — lienzo (`AnnotateSurface`), modelo puro y dibujo, con tests

## Pendiente / siguiente

- [ ] Pizarra sobre la pantalla (dibujar fuera de una captura), fase 2:
      mismo motor sobre el congelado de pantalla completa
- [ ] Texto y desenfoque (tapar datos) en el editor
- [ ] Upgrade futuro a Windows Graphics Capture (ver plan §23)
- [ ] Paridad / stub macOS si se prioriza

## Relacionado

- [clipboard-historial.md](clipboard-historial.md)
- [agentes.md](agentes.md)
- [`docs/PLAN_HERRAMIENTA_CAPTURAS.md`](../docs/PLAN_HERRAMIENTA_CAPTURAS.md)
