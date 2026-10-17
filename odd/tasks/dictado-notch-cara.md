# Dictado en notch: cara expandida, sin desacoplar

**Estado:** implementado (pendiente prueba visual del usuario)
**Fecha:** 2026-09-19
**Ruta:** directa inline (edición dirigida en 4 archivos ya mapeados)

## Objetivo

Al dictar en modo notch NO desacoplar la pill en dos bolas: mantener la forma
del notch, que se expanda hacia abajo y mostrar ahí las sound waves. Lo mismo
al transcribir/pegar: el notch se agranda para que quepa el estado
("Transcribiendo…", "Pegado", error) en vez de verse una cápsula suelta al lado.

## Cambios

- `pillStage.ts`: constantes `islandDictW: 232` / `islandDictH: 72` (tarjeta fija,
  sin re-medir por frame, igual que las otras caras).
- `pillPlan.ts`: `"dictation"` en `IslandFace`; predicado puro
  `islandFaceDictation` (solo acoplada y dictando); rama en `contentFor` que
  crece pestaña + tarjeta hacia el escritorio en cualquier canto.
- `PillSurface.svelte`:
  - `islandFace` prioriza: permiso de agente > dictado > caras de herramienta.
  - ELIMINADO el efecto que desacoplaba al dictar (`dictDockedEdge` era código
    muerto ahora). La onda vive en `.p-face[data-face="dictation"]`; la gota
    `.p-skin-drop` queda solo para la pill flotante.
  - Cara en template: Waveform voice live (escuchando) o icono + label
    (transcribiendo/pegado/error); parar sigue siendo el clic en la marca.

## Verificación

- `vitest run pillPlan.test.ts`: 88 passed (2 regresiones nuevas).
- `svelte-check --threshold error`: 0 errores (3 warnings previos en otros archivos).
- Pendiente: prueba visual del usuario vía HMR (sesión `pnpm tauri dev` activa).

## Pendiente / notas

- Commits diferidos: hay WIP sin confirmar de otra sesión (consola-agentes,
  ~1000 líneas) intercalado en los mismos archivos; no se mezcló en commits.
  Confirmar ese feature primero o dejar que el usuario decida el corte.

## Extensión: resumen ambiental de agentes

- `pillAgentChip.ts`: las presencias JSONL frescas ya no se descartan por no
  coincidir con otra consola viva; solo se poda `working` sin ventana después
  de 90 s cuando se entrega un reloj explícito.
- `pillPlan.ts` / `pillStage.ts`: la cara `live` mide una fila de 32 px por
  agente y crece hacia abajo o hacia adentro según el canto.
- `PillSurface.svelte`: el notch prioriza la cara live después de permiso,
  dictado y herramientas, y muestra una fila compacta por chip con logo y
  estado. La banda superior conserva la marca y no duplica los logos. Un chip
  real solo usa su propio logo; el agregado de marcas queda reservado al dock
  minimizado sin chip.

### Checks

- `vitest run pillAgentChip.test.ts pillPlan.test.ts`: 2 archivos, 121 tests
  pasaron.
- `svelte-check --threshold error`: 0 errores; quedaron 3 warnings.
- Detector Impeccable sobre los archivos cambiados: sin hallazgos.
- Pendiente: prueba visual del usuario vía HMR.

## Extensión: el turno terminado sigue abajo

- `watch_opencode.rs`: en v2, la fila `idle` es el CIERRE del turno, no «sin
  respuesta»: clasifica como `Ready` y el preview (primera línea de la última
  respuesta) sale del último `assistant` (`last_assistant_preview_v2`). El
  watcher ya poda Ready a los 30 min; la ventana viva de 15 min marca el final.
- `pillAgentChip.ts`: el chip `ready` muestra el preview SIEMPRE (se haya
  leído o no) — es el relato de que terminó. Sin preview cae al «Listo» del
  fallback de la fila.
- Resultado: al terminar un turno la cara live queda abierta en verde con el
  inicio de la respuesta, y se cierra sola cuando la sesión envejece.
- Checks: 121 tests frontend; `cargo test --locked -p atic-desktop
  watch_opencode` 11 passed. Pendiente prueba visual (exige recompilación de
  la sesión `tauri dev`).
