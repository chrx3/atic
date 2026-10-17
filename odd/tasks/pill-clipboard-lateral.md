# Pill: clipboard al costado (isla expandida a la derecha)

**Estado:** en validación visual (código completo y verificado; falta prueba del usuario y commit)
**Fecha:** 2026-09-19
**Ruta:** directa-delegada (un writer; toca 2+ archivos no triviales)

## Objetivo

Con la consola de agentes abierta en la isla del notch, abrir el historial
(clipboard) o los textos (snippets) ya NO los superpone tapando la consola:
la isla se expande HACIA LA DERECHA y el panel aparece AL LADO de la consola,
dentro del mismo blob, con la coreografía de apertura existente (misma
transición de caja, misma familia de motion, marca/robot intactos).

## Problema / por qué

Hoy `sidePanel` monta `.p-side` en `position:absolute; right:8; bottom:8`
encima de la consola (captura del usuario, 11:46). Pedido textual: “como un
desplegable hacia la derecha, o sea que se expanda el tamaño de la pil y
aparezca el clipboard al lado, con la misma animación de que se abre y el
robot y todo”.

## Alcance

- Entra: cara `agents` con consola viva (`consoleFaceShowing()`), en cualquier
  canto acoplado; `clipboard` y `snippets` como panel lateral.
- No entra: el camino float (`ClipboardFloat` / `showClipboardWindow` /
  `executeToolAction`) queda inerte como está. Sin consola, clipboard/textos
  siguen abriendo como cara, sin cambios.
- No reescribir la coreografía de apertura: se reutilizan los tokens
  (`--island-open-dur`, `--ease-island`, `--ease-liquid`, `--island-stagger`).

## Decisiones

1. **La consola no se mueve.** El borde de la caja del lado acoplado queda
   fijo y la caja crece hacia el otro lado (top → crece a la derecha con borde
   izquierdo fijo). Aplica el mismo criterio en bottom (bottomLeft) y en
   left/right (ya anclan su canto).
2. **Tamaño combinado (puro, en `pillPlan.contentFor`)** con parámetro `side`:
   - axis y (top/bottom): `w = max(long, w + islandGap + islandClipW)`,
     `h = thick + max(h, islandClipH)` (496 > 252 ⇒ alto sin cambio).
   - axis x (left/right): `w = thick + w + islandGap + islandClipW`,
     `h = max(long, h, islandClipH)`.
3. **Layout**: `.p-island-body` (columna con banda de pestaña) pasa a envolver
   un `.p-row` = [cara agents (ancho `--face-agents-w`)] + [panel
   (`--face-clip-w`, estirado a la altura)]. El panel deja de ser absoluto:
   ya no tapa nada.
4. **Marca anclada**: con panel abierto, `.p-island-along` se restringe al
   ancho de la consola; la marca (AticMark) queda exactamente donde estaba.
5. **Anclaje fino**: `pivotFor` con `side` (top→`topLeft`, bottom→
   `bottomLeft`; left/right sin cambio) y el recentrado del notch
   (`shouldRecenterTopNotch`) usa “unidad de recentrado” = ancho de consola
   cuando el layout lleva panel (si no, la caja entera). El flag `side` de
   `plan()` debe cubrir también la transición de cierre (layout aplicado con
   panel), para que abrir y cerrar sean simétricos y sin salto.
6. **Entrada del panel**: usar la familia de motion de la casa (fade +
   desplazamiento corto con `--island-open-dur`/`--ease-liquid`); nada de
   coreografía nueva.

## Tareas

- [x] **T1** `pillPlan.contentFor`/`targetFor`: parámetro `side` +
      `notchRecenterSize` + `sideExitX`; `pillPlan.test.ts` actualizado.
- [x] **T2** `PillSurface.svelte`: `target` con `sideInBox`; markup `.p-row`
      (`display:contents` sin panel); CSS del panel en flujo; along anclado
      (`has-side`); entrada animada con tokens.
- [x] **T3** Anclaje: `pivotFor` con `side` (top→topLeft, bottom→bottomLeft),
      recentrado por unidad, anclaje de salida (`appliedSide` + `sideExitX`).
- [ ] **T4** Validación: vitest + check **hechos**; prueba visual del usuario
      **en curso**. Commit pendiente de validación (árbol con WIP ajeno — no
      mezclar).
- [x] **T5** (usuario, 19-sep) Panel sin chrome propio: sin fondo, sin
      filete/borde, sin sombra, sin radio; sin separación con la consola
      (`sidePanelSpan()` = `islandClipW`, `.p-row.is-side { gap: 0 }`).
- [x] **T6** (usuario, 19-sep) Arrastrar un ítem del historial ya no mueve la
      isla ni cierra la consola: `.p-side` hace `stopPropagation` del
      pointerdown (mismo patrón que las caras).

- [x] **T7** (usuario, 19-sep) Drop OLE sobre la consola abierta: publicar el
      hit-rect `agents` también con la cara de agentes a la vista (antes solo
      con la consola minimizada) → durante el arrastre el overlay queda armado
      sobre la consola y el drop inserta; `agentsExpanded` excluye la cara de
      la isla para conservar los avisos de la banda.

## Bitácora

- 19-sep 12:20 — Implementación (writer): 81 verdes en `pillPlan.test.ts`,
  290 en `surfaces/overlay`.
- 19-sep 12:22 — Verificación independiente: suite verde; hallazgos H1 (cierre
  total desde panel en canto inferior dejaba la pestaña corrida), H2
  (`sidePanel` colgado al abrir rueda/desacoplar → panel reaparecía solo), H3
  (`spatialIntent` colgado en el toggle); nota visual del panel (fondo/sombra
  propios).
- 19-sep 12:26 — Correcciones: `appliedSide` + `sideExitX` (H1), limpieza de
  `sidePanel` al salir del canto (H2), espejo de `spatialIntent` (H3).
  Suite completa: 294 verdes; `check` 0 errores (3 warnings preexistentes);
  eslint limpio en los archivos tocados.
- 19-sep 12:40 — Feedback visual del usuario: (1) el panel se ve como tarjeta
  con borde/sombra/separación → debe leerse como una sola superficie; (2) bug:
  arrastrar un ítem del historial movía la isla entera y cerraba la consola.
  T5/T6 en curso.
- 19-sep 12:38 — T5/T6 hechas: `.p-side` sin fondo/anillo/sombra/radio y sin
  gap (helper `sidePanelSpan()` compartido por `contentFor`, `sideExitX` y
  `notchRecenterSize`); `.p-side` frena el pointerdown (el arrastre de ítems ya
  no siembra el de la isla; bonus: seleccionar en el scratchpad tampoco).
  Suite 294 verdes; check limpio.
- 19-sep 12:48 — T7: el drop de ítems caía en la app de atrás porque el único
  drop-target OLE (`agents`) solo existía con la consola minimizada; con la
  cara abierta el overlay quedaba click-through sobre la consola (el rojo es
  el watch JS del cursor, no dependía del rect). Fix: registrar la cara como
  `agents` (`{@attach}` + efecto) y excluir la cara de la isla en
  `agentsExpanded` (banda/avisos como antes). Suite 294 verdes; check limpio.

## Criterios de aceptación

1. Con consola abierta, abrir el historial lo muestra AL LADO (derecha), la
   consola no se mueve ni se tapa.
2. La caja crece con la transición existente (240 ms, ease de la casa) y al
   cerrar (Esc / clic afuera / pegar) vuelve exactamente al estado previo.
3. Sin consola, el comportamiento actual no cambia (cara clipboard como hoy).
4. Pegar desde el panel sigue insertando en la consola y vuelve a la cara de
   consola (`backToConsoleAfterPaste`).
5. La marca (AticMark) y los avisos no se corren de lugar con el panel abierto.

## Verificación

- `pnpm --dir apps/desktop exec vitest run src/lib/surfaces/overlay/pill/pillPlan.test.ts`
- `pnpm --dir apps/desktop check` (tipos/svelte) al cierre.
- Visual: app corriendo (`tauri dev`, PID 19028) con HMR — sin reinicios.

## Archivos

- `apps/desktop/src/lib/surfaces/overlay/pill/pillPlan.ts`
- `apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte`
- `apps/desktop/src/lib/surfaces/overlay/pill/pillPlan.test.ts`
- (solo si hace falta) `apps/desktop/src/lib/surfaces/overlay/pillCssStage.ts`

## Contexto de código (referencias)

- Rama actual: `PillSurface.svelte:2799-2811` (`runActivateAtSlot` →
  `sidePanel = id`) y `:1162-1189` (`sidePanel`, `consoleFaceShowing`,
  `islandConsoleAlive`).
- Panel actual: markup `:3888-3951`, CSS `.p-side` `:4967-4985`
  (absolute right/bottom, encima).
- Caja: `target` `:1250-1268`; `contentFor` `pillPlan.ts:111-202`;
  `pivotFor` `pillPlan.ts:530-552`; `shouldRecenterTopNotch` `:316-326`;
  `reconcile` `PillSurface.svelte:1574-1619`; pivotes `pillCssStage.ts:167-253`.
- Layout isla: `.p-island` `:4543-4575`, `.p-island-body` `:4577-4585`,
  `.is-face` `:4888-4926`, `.p-face` `:4928-4965`, `.p-island-along`
  `:4617-4628`, transición caja `:4430-4440`.
- Constantes `PILL` en `pillStage.ts` (`islandAgentsW/H 440/496`,
  `islandClipW/H 280/252`, `islandGap`).
