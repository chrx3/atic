# Consola de agentes: minimizar/restaurar sano + detach a ventana propia

**Estado:** implementando
**Fecha:** 2026-09-19
**Ruta:** directa-delegada (un writer; varios archivos no triviales)

## Objetivo

1. Minimizar/restaurar la consola de agentes en la isla SIN que el layout/terminal
   se rompa (reporte del usuario con captura: al volver, canvas en blanco y
   banda negra stale).
2. Poder separar (detach) la consola a su ventana independiente (`agents`) desde
   la isla, y poder volver.

## Diagnóstico (mapeo previo, con evidencia)

- Única vía de minimizar en la isla: `dismissToolFace` (Esc, clic afuera, clic
  en la marca) → `agentsDock.setMinimized(true)` (PillSurface ~:1271-1279);
  `sidePanel` se limpia por tres vías y no queda vivo. El `dockToPill` del
  float es inalcanzable.
- Al RESTAURAR, la cara vuelve al flujo (`.is-stowed` off) mientras la caja
  anima pestaña→consola (`--island-open-dur` 240 ms). `.p-face` es
  `flex: 1 1 auto; min-height: 0` → se aplasta y el `ResizeObserver` del xterm
  (guard 24 px; ConsolePanel ~:2316, ~:2507) refitea en cada paso →
  `consoleResize` con filas/cols intermedios (tormenta de SIGWINCH) y canvas a
  medio pintar ⇒ área blanca + banda stale (lo de la captura).
- Minimizada, la cara viva se estaciona fuera de pantalla
  (`.p-face[data-face="agents"].is-stowed { position: fixed; left: -12000px }`,
  ~:5143) — documentado como lo que congela el canvas de xterm; al volver no
  hay fit/refresh explícito si el tamaño no cambió.
- Detach: ya existe el protocolo de transferencia (`consoleTransfer.ts`,
  `detachTo` en AgentLauncher :259-336, `console_begin_transfer`,
  `agents-transfer`/`agents-transfer-ack`, PTY en vuelo 90 s), la ventana
  `agents` (`agents_window.rs`, App("agents") 1120×760, cerrar = ocultar) y el
  botón dockBack ventana→isla. FALTA: pasar `detachTo`/`detachBusy` a la isla
  (hoy `onDetachRequest={island ? undefined : detachTo}` :655 y el botón exige
  `onDetachRequest` :3529-3544), host de toasts en el overlay, colapsar la cara
  al quedar sin fichas, montar la cara al recibir un transfer a `"overlay"`, y
  bandera `set_agents_console_open` para la consola que vive en la ventana
  (hoy `agents_open()` = OPEN || ISLAND_OPEN y la ventana nunca la setea;
  además la isla la deja true aunque esté minimizada).

## Tareas

- [x] **T1** Minimizar/restaurar sano: fits del `ResizeObserver` coalescidos
      (`scheduleSettledFit`, 96 ms) + fit/refresh al reaparecer (`visible`
      false→true, espera `MOTION.islandOpen`).
- [~] **T2** **REVERTIDO (19-sep, pedido del usuario)**: probó el detach y no
      le gustó → el botón vuelve a no mostrarse en la isla
      (`onDetachRequest={island ? undefined : detachTo}`, igual que HEAD). Se
      conservan el host de toasts del overlay, T4 y T5: sirven a la ventana del
      tray y al dockBack, no al detach desde la isla. Re-activarlo es volver a
      pasar `detachTo` en la isla.
- [x] **T3** Sin fichas tras el detach: verificado por lectura — `setHasConsole(false)+showView("setup")`
      ⇒ `agentsConsoleView=false` ⇒ caja tamaño setup (sin hueco muerto).
- [x] **T4** Vuelta ventana→isla: efecto en PillSurface que monta/abre la cara
      si llega un transfer a `"overlay"` (el lanzador adopta y ackea).
- [x] **T5** Pegado: bandera `WINDOW_OPEN` + `set_agents_window_open` (la
      ventana publica con consola viva y a la vista); la isla conserva su
      contrato (pegado a consola stowed intacto).
- [ ] **T6** Verificación: vitest overlay + check + eslint + `cargo check`
      **hechos** (294 verdes; check limpio; cargo OK 40 warnings preexistentes);
      falta prueba visual del usuario (minimizar/restaurar, detach/dockBack y
      pegado con la consola en la ventana). Commit pendiente de validación
      (árbol con WIP ajeno — no mezclar).

## Criterios de aceptación

1. Minimizar y restaurar no deja canvas en blanco ni banda stale; no se
   multiplican los resizes del PTY.
2. Con la consola en la isla, el botón de detach la mueve a la ventana `agents`
   con feedback, y la vuelta la devuelve a la isla.
3. Con la consola en la ventana, pegar desde el historial entra a su sesión;
   minimizada en la isla, el pegado no apunta a una consola invisible.
4. Sin fichas tras el detach, la isla no queda con hueco muerto.

## Archivos

- `apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte`
- `apps/desktop/src/lib/features/agents/ConsolePanel.svelte`
- `apps/desktop/src/lib/features/agents/AgentLauncher.svelte`
- `apps/desktop/src/lib/surfaces/overlay/agents/AgentsFloat.svelte` (toasts)
- `apps/desktop/src/lib/ipc/agents.ts` / `consoleTransfer*.ts`
- (solo si T5 lo exige) `apps/desktop/src-tauri/src/agents/bridge.rs`

- [x] **T7** (usuario, 19-sep) Chrome denso SOLO en la isla: prop `dense`
      (`AgentLauncher` pasa `dense={island}`); riel 8rem→4.25rem icon-only
      (`railCompact` incluye `dense`), cruz absoluta al hover (sin columna
      reservada), sin resizer; header `min-height` 2.25rem, `icon-btn` 1.45rem,
      chip de carpeta icon-only, zoom sin ± (el % queda; Ctrl+rueda/pinch
      siguen), `zoom-cue` ajustado. Ventana/float intactos.

- [x] **T8** (usuario, 19-sep) Borde azul: era el anillo de foco de WebKit
      sobre la cara enfocada por código. `outline: none` en `.p-face`
      (PillSurface :5133-5136); cubre las tres caras; focos reales intactos.
- [x] **T9** (usuario, 19-sep) Riel a barra INFERIOR en la isla (denso):
      `.console.is-dense` `column-reverse`; `.rail` fila full-width con
      hairline arriba; `.rail-tabs` fila con scroll-x; fichas 2.4rem;
      `max-width: none` (el tope de columna capaba la barra); menú del “+”
      abre hacia arriba; radio de esquina del primer chip normalizado.
      Ventana/float conservan el riel lateral.

- [x] **T10** (usuario, 19-sep) Agentes trabajando: texto vivo en el chip
      (stream coalescido 120 ms), fallback “Trabajando…/Contestando…”
      (`answering` = último mensaje assistant con `streaming`), piel que
      respira con `agentWorking` (`liquid.breathe`) y latido del logo/pestaña
      (`p-agent-pulse`); reduced-motion cubierto.
- [x] **T11** (usuario, 19-sep) Dictado: la cara queda sola en la barra (sin
      micrófono) y la soundwave baja a una gota colgante debajo (patrón del
      tail + `liveHang` para flotante+dictando + `trackDrop`), onda vertical
      (`transform-origin:top`), publicada en `skinShapes` (un solo blob);
      entrada “cae desde la pill” con `--island-open-dur`/`--ease-liquid`.

- [~] **T12** (usuario, 19-sep) **CORRECCIÓN DE RUMBO — implementación en
      dirección equivocada.** El pedido real: **mantener la forma del notch/pill
      intacta** y mostrar las **soundwaves DEBAJO** (sin gota/droplet fundida).
      El cuello explícito (`stemBetween`) y el redimensionado de la gota NO eran
      lo pedido. **El usuario se lo pasó a otro agente (19-sep ~15:30): NO tocar
      `PillSurface.svelte` / `pillStage.ts` en esa zona** para no pisarlo.
      Si el otro agente no toca el cuello, revertirlo es una línea.
- [x] **T13** (usuario, 19-sep) Diagnóstico de “¿dónde dice agente
      trabajando?”: la cadena del frontend está sana; el watcher de opencode
      lee el esquema legado y el opencode v2 escribe `session_v2` /
      `session_message` → sin presencia, sin chip. Plan aprobado (T14).
- [x] **T14** Watcher opencode v2 (`watch_opencode.rs`): `schema_of()` por
      `pragma_table_info`; `session_v2` (parent_id NULL, no archivadas, en
      ventana) + `last_message_v2` por `seq` + `classify_v2` (user→Working;
      assistant sin `completed`→Working/contestando; completed→Ready con
      preview; idle→Idle); fallback legado intacto; 6 tests nuevos (11
      verdes). `waiting` queda para el próximo corte (0 filas para validar).
      Nota: se arregló una línea de un test preexistente que no compilaba
      (`agents/hub/server.rs`, `.to_string()` mecánico).

## Bitácora

- 19-sep 12:55 — Mapeo (explore): diagnóstico de A y B (ver arriba). T1-T5
  definidas; el usuario espera probar minimizar/restaurar y detach.
- 19-sep 13:10 — T1 (ConsolePanel): `scheduleSettledFit` (96 ms) para el
  `ResizeObserver` + fit/refresh al reaparecer (`visible` false→true, espera
  `MOTION.islandOpen`). Suite 294 verdes; check limpio.
- 19-sep 13:20 — T2-T5: detach desde la isla (`onDetachRequest={detachTo}`) +
  host de toasts en el overlay; T3 verificado por lectura; T4 efecto que monta
  la cara al recibir transfer a `"overlay"`; T5 `WINDOW_OPEN` +
  `set_agents_window_open` (isla intacta). Rust `cargo check` OK; suite 294
  verdes; check/eslint/prettier/fmt limpios.
  Límite anotado: pill oculta ⇒ dockBack al overlay sin consumidor (aborta con
  toast). Doble insert posible en traspasos parciales (forma preexistente).
- 19-sep 13:35 — Detach REVERTIDO a pedido del usuario (“no me gustó, quítalo
  por ahora”): `onDetachRequest={island ? undefined : detachTo}` restaurado en
  AgentLauncher (el botón ya no aparece en la isla). T1 queda; toasts/T4/T5 se
  conservan para la ventana del tray. Suite 294 verdes; check 0 errores.
- 19-sep 13:45 — T7: modo denso de la consola en la isla (riel 4.25rem
  icon-only con × al hover y sin resizer; header 2.25rem, controles 1.45rem,
  carpeta icon-only, zoom sin ±; `zoom-cue` ajustado). El puntito `.live` del
  riel baja al vértice inferior (la × ocupa el superior) y el label de sesión
  (`.where`) queda visible a propósito. Suite 294 verdes; check/eslint/prettier
  limpios.
- 19-sep 13:52 — T8: anillo de foco de WebKit sobre la cara enfocada por
  código → `outline: none` en `.p-face`. T9: riel a barra inferior en la isla
  (column-reverse, fila con scroll-x, fichas 2.4rem, `max-width: none`, menú
  del “+” hacia arriba). Ventana/float intactos. Suite 294 verdes;
  check/eslint/prettier limpios.
- 19-sep 14:00 — T10: agentes trabajando (chip con texto vivo coalescido
  120 ms, “Trabajando…/Contestando…”, breathe de la piel con `agentWorking`,
  latido del logo y de la pestaña; `answering` acotado a la sesión del chip).
  Suite 295 verdes.
- 19-sep 14:12 — T11: dictado con la cara quieta y la onda colgando en una
  gota vertical debajo; sin micrófono; stop en la cara. Suite 296 verdes.
- 19-sep 13:57 — (aparte) Filtro de clipboard/textos: se quitó el fallback de
  subsecuencia de `fuzzyMatch` (devolvía ítems sin la palabra buscada);
  regresión en `core/clipboardSearch.test.ts` (6 tests). Cross-cutting:
  aplica también a textos, capturas y reuniones.
- 19-sep 14:45 — T12: la gota del dictado no se fundía por `SKIN_BLEND = 0`;
  cuello explícito con `stemBetween` + gota al ancho del disco. Suite 296
  verdes; check limpio.
- 19-sep 14:48 — T13: diagnóstico con evidencia (watcher legacy vs opencode
  v2: `session_v2`/`session_message`). T14 (watcher v2) en curso; toca Rust y
  `tauri dev` recompila solo.
- 19-sep 15:14 — T14 hecha: watcher v2 (`schema_of`/`session_v2`/
  `classify_v2`, fallback legado) con 11 tests verdes; `waiting` documentado
  para el próximo corte; una línea de un test preexistente en
  `hub/server.rs` que no compilaba (mecánica). La app recompiló y volvió.
- 19-sep 15:30 — Corrección del usuario: el chip mostró bien al otro agente
  trabajando (T14 ✓). Pero el diseño del dictado era otro: **forma del notch
  intacta + soundwaves DEBAJO**, sin gota fundida → T12 va en dirección
  equivocada y el usuario se lo pasó a OTRO AGENTE. Manos fuera de
  `PillSurface.svelte`/`pillStage.ts` en esa zona.
