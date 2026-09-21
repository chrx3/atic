# Plan — Converger la consola con el modelo canónico

> **Fecha:** 2026-09-20
> **Estado:** Fase 0 en curso — T1, T2 y T3 implementadas el mismo día y
> pendientes de prueba manual; T4 sin empezar. Las fases 1–4, propuesta.
> Arrancó de un bug real (ver §4, D1, y la bitácora en §11).
>
> No reemplaza a [`PLAN_AGENTES.md`](PLAN_AGENTES.md): lo **retoma**. Convive
> con [`PLAN_AGENTES_TUI.md`](PLAN_AGENTES_TUI.md) (el pager) y no toca
> [`PLAN_ORQUESTACION_MCP.md`](PLAN_ORQUESTACION_MCP.md), que ya está
> implementado.

---

## 1. El hallazgo que ordena todo

**Atic ya tiene el camino bueno construido, y la UI dejó de usarlo.**

| Camino | Dónde vive | Estado real hoy |
|---|---|---|
| **Estructurado** — modelo canónico, 3 adaptadores, permisos, persistencia, resume, cuotas | `agents/model.rs`, `claude_code.rs`, `acp.rs`, `codex.rs`, `store.rs`, `turns.rs`; comandos `agent_start` / `agent_send` / `agent_permission` / `agent_interrupt` / `agent_threads` registrados en `lib.rs` | **Vivo y cableado.** Su UI (`AgentsDemo.svelte`, ~2.500 líneas) **no la monta nadie**: solo quedan menciones en comentarios. |
| **PTY** — consola embebida, xterm, traspaso entre ventanas | `agents/console.rs`, `ConsolePanel.svelte`, `AgentLauncher.svelte`, `consoleTransfer.*` | **Es lo que se usa.** `routes/agents` monta `AgentsWindow`, que es la consola. |
| **Pager** — presencia leída del rastro en disco | `watch_claude.rs`, `watch_codex.rs`, `watch_cursor.rs`, `watch_opencode.rs`, `presence.rs` | Vivo, alimentando los chips de la pill. |

O sea: **no hay nada que "portar a ACP"**. ACP ya está —OpenCode y Cursor por
`acp.rs`, Claude Code por su `--input-format stream-json`, Codex por
`app-server`—. Lo que falta es **volver a enchufar la UI al modelo** y dejar la
terminal como un modo de render, no como la fuente de verdad.

La costura ya existe y está probada: `ConsolePanel` **ya pinta fichas de dos
clases** —un xterm con PTY, o una conversación estructurada
(`HubConversation`, para las sesiones que abre otro agente por MCP)— y el rail,
el split, los grupos, el zoom y los atajos son agnósticos de cuál sea. Y
`AgentConversation.svelte` se escribió **a propósito** para montarse fuera del
globo: "dibujar una conversación no depende de estar dentro de la burbuja".

---

## 2. Por qué el chat quedó fuera: fue la UI, no la arquitectura

Dato del dueño del proyecto (2026-09-20): **la UI de esa versión no gustaba.**
Eso es lo que hay que escribir antes que nada, porque cambia el plan entero:

- El motivo del abandono **no** fue el modelo, ni los adaptadores, ni los
  permisos. Todo eso sigue siendo bueno y sigue andando.
- Por lo tanto **`AgentsDemo.svelte` no se resucita.** Se le saca la lógica
  (composer con sus estados, resume, selección de modelo, historial de hilos,
  inserción desde el portapapeles) y **se tira su forma**.
- Y la forma nueva no se inventa: **es la de la consola que ya te gusta.** La
  ficha de chat hereda el mismo mueble —rail de fichas, barra, zoom, split,
  atajos, chrome unificado— y lo único que cambia es qué se pinta en el
  viewport: transcript + composer en vez de xterm.

Consecuencia práctica: el riesgo de la Fase 1 no es técnico, es de diseño, y se
acota solo si la ficha de chat **no trae mueble propio**. Ver §7.

---

## 3. Lo que NO se hace

Escrito temprano, porque es la mitad del valor.

- **No se tira la consola PTY.** Es compatibilidad total con cualquier CLI y es
  lo que se usa a diario. Pasa a ser *un modo de ficha*, no el único.
- **No se reemplaza la TUI por el chat "para tener control".** Un CLI tiene
  cosas que el protocolo no expone: sus propios comandos de barra,
  instaladores, login interactivo. Para eso está la PTY.
- **No se resucita la UI de `AgentsDemo`** (§2).
- **No se toca** la pill, el sistema líquido, las islas, la rueda ni la
  geometría. Este plan cambia *qué datos* pinta la consola, no cómo se mueve.
- **No se toca el contrato MCP ni el hub.** `atic-mcp`, `hub/` y el grafo
  quedan como están.
- **No se persigue ACP para Claude Code.** No lo habla; su adaptador propio ya
  anda y se queda.
- **No se migra `atic.db3`** más allá de lo que ya hay.

---

## 4. Los tres defectos de control (con evidencia, no con opinión)

### D1 — El dueño del proceso es un componente de UI

Despegar la consola de la isla **mataba la sesión**. Traza del 2026-09-20
(instrumentación temporal sobre `pill_trace`, ya retirada):

```
clearTransferred after=[] n=0        la isla suelta la ficha (correcto)
settleAfterDetach remaining=0
onDestroy ids=["b0e9…"] tabs=1       6 ms después el panel ve la lista VIEJA
disconnect key=t1 id=b0e9…           → console_close → PTY muerta (exit 129)
```

Dos causas encadenadas: `ConsolePanel.onDestroy` cerraba **dos veces** cada
sesión (el bucle + `disconnectAll`), y `console_close` consume la marca de
traspaso, así que el primer cierre la gastaba y el segundo mataba de verdad.
Encima el receptor levanta la protección (`console_end_transfer`) **antes** de
que el emisor suelte sus fichas.

Mitigado el mismo día con un registro de sesiones entregadas (`handedOver`,
`Set` fuera del estado reactivo) que ni `disconnect` ni `onDestroy` pueden
cerrar. **Eso es una venda, no la cura.** La cura es que el ciclo de vida de un
proceso no dependa de un `{#if}` de Svelte.

### D2 — `console_gc` mata PTYs de otras vistas · **vivo hoy**

`reapOrphanConsoles()` llama `console_gc(knownSessionIds())` con las fichas **de
ese panel**, y Rust mata todo lo que no esté en `keep` y no esté protegido por
una marca de traspaso —que caduca a los 90 s—. Con dos vistas vivas (isla y
float, o la ventana dedicada), abrir una consola nueva en una **reapea las de la
otra**. Se dispara en cada `connect()`.

### D3 — El estado de la consola se adivina

Para saber si un agente trabaja o espera permiso, Atic lee el JSONL de Claude
Code y el **SQLite privado de OpenCode** (y ahora su esquema v2).
`watch_opencode.rs` incluso documenta que no puede emitir `waiting` porque no
logró verificar qué fila significa "esperando" — mientras el adaptador ACP
recibe `session/request_permission` y lo sabe con certeza.

---

## 5. Fases

Cada fase deja el árbol usable y se puede parar ahí.

### Fase 0 — Dueño único: el proceso es de Rust (≈1 día)

La regla, y de ella sale todo lo demás:

> **Una sesión se cierra por acción explícita del usuario o por el GC. Nunca
> porque una vista se desmontó.**

- **T1 · hecho (2026-09-20)** — lo entregado no se cierra (`handedOver`), el
  desmontaje cierra una sola vez por sesión y `adoptSessions` vuelve a hacer
  propia una sesión readoptada. Archivos: `ConsolePanel.svelte`.
- **T2 · hecho (2026-09-20)** — `console_gc` ya no recibe `keep` del cliente.
  Rust lleva el registro de vistas (`console_attach` / `console_detach` /
  `console_heartbeat`) y barre lo que no reclama ninguna vista viva. Tres
  relojes, cada uno por una razón distinta: gracia de 30 s sin dueño (el hueco
  del despegue), caducidad de 45 s del reclamo sin latido (la vista que muere
  sin desmontarse: recarga del overlay) y barrido cada 20 s en un hilo que
  arranca con la primera consola (para no depender de que la UI llame). Y
  `ConsolePanel.onDestroy` **suelta en vez de cerrar**. Arregla D2. Archivos:
  `agents/console.rs`, `lib.rs`, `ipc/agents.ts`, `core/types.ts`,
  `ConsolePanel.svelte`. Tests: `el_registro_de_vistas_manda_sobre_el_barrido`,
  `un_reclamo_sin_latido_caduca`.
- **T3 · hecho (2026-09-20)** — El traspaso isla ⇄ float ya no pasa por Rust.
  Mismo webview, mismo contexto JS: el bus local devuelve una promesa y la
  receptora la resuelve en el acto. Se fueron, en ese camino,
  `console_begin_transfer`, `console_end_transfer`, el ack por
  `agents-transfer-ack` y la espera de 4 s; `receiveTransfer` quedó partido en
  `adoptPayload` (común) + la confirmación de cada camino. La receptora
  reclama **antes** de que la emisora suelte, así que la sesión nunca queda
  sin dueño. Archivos: `consoleTransfer.svelte.ts`, `AgentLauncher.svelte`.
- **T4 · pendiente** — La ventana dedicada usa el mismo camino
  (`console_attach` desde el otro webview). Ahí sí hace falta cruzar procesos,
  pero ya no para proteger una PTY: solo para decirle a la otra ventana qué
  mostrar. Con eso se van `console_begin_transfer`, `console_end_transfer`, el
  mapa `TRANSFERS` con su TTL de 90 s, `consoleTransferDeliver`,
  `onAgentsTransferAck`, `waitTransferAck` y `consoleTransfer.*` completo.
  Archivos: `agents/console.rs`, `AgentsWindow.svelte`, `AgentLauncher.svelte`.

**Hecho cuando:** despegar y acoplar 20 veces no pierde una sesión; con una
consola despegada, abrir otra en la isla no mata nada; `consoleTransfer.ts` y
sus tests se borran sin que nada se rompa.

### Fase 1 — La ficha de chat, de primera clase (≈2–3 días)

- **T1** — Generalizar la ficha. Hoy `Tab` tiene `sessionId` (PTY) y
  `hubSession` (conversación de sólo lectura); pasa a `source: "pty" | "chat" |
  "hub"` con un id. El rail, el split, los grupos, el zoom y los atajos no se
  tocan. Archivos: `ConsolePanel.svelte`, `lib/types.ts`.
- **T2** — `AgentChatPanel.svelte`: **composición, no código nuevo**. Ya
  existen `AgentConversation.svelte` (transcript), `AgentToolCard`,
  `AgentMessage`, `AgentCollabCard` y `PermissionBar.svelte`. Falta el composer
  y los controles de sesión, que se **extraen** de `AgentsDemo.svelte`.
- **T3** — El `+` del lanzador ofrece, por agente, **Chat** o **Terminal**. El
  chat llama `agent_start(backend, {cwd, model, …})`; la terminal,
  `console_open` como hoy. Archivos: `AgentLauncher.svelte`.
- **T4** — `HubConversation` pasa a ser el modo sólo-lectura del mismo panel
  (hoy ya lo es de hecho, pero con su propio markup).
- **T5** — `AgentsDemo.svelte` queda deprecado y se borra **cuando** T2 cubra
  lo suyo (historial, modelos, resume, inserción desde el portapapeles). No
  antes, y no se copia su forma (§2).

**Hecho cuando:** abrir Claude Code como chat dentro de la isla, mandar un
prompt, ver las tool-calls con su estado, contestar un permiso desde la barra,
minimizar y restaurar sin perder nada, y que el hilo quede en el historial —
todo con el mismo mueble que la consola de hoy.

### Fase 2 — Una sola lista de sesiones (≈1–2 días)

- **T1** — Un normalizador puro (TS testeable, patrón `pillPlan.ts`) que une
  tres fuentes en una lista: canónicas (`agents.sessions`), PTY vivas y
  presencias externas. Nuevo: `surfaces/overlay/pill/agentRoster.ts` + test.
- **T2** — El chip de la pill y la cara `live` leen **esa** lista. Las reglas
  de prioridad y de no-duplicar ya están escritas y justificadas en
  `PLAN_AGENTES_TUI.md` §5.4: se aplican tal cual. Archivos:
  `pillAgentChip.ts`, `PillSurface.svelte`.

**Hecho cuando:** el mismo agente nunca aparece dos veces y el clic siempre
lleva a donde esa sesión vive.

### Fase 3 — El pager, sólo para lo de afuera (≈1 día)

- **T1** — Los watchers ignoran lo que es nuestro. Ya hay filtro por
  `providerSession`; se extiende a las PTY que lanzamos nosotros (conocemos cwd
  + CLI). Archivos: `agents/watch_*.rs`, `agents/presence.rs`.
- **T2** — Para una sesión de chat, el estado sale del modelo, no del disco.
  Los watchers quedan para agentes que corren en su propia terminal, que es
  exactamente la tesis del plan TUI.

### Fase 4 — El MCP de Atic a un clic (≈1 día)

- **T1** — `hub_snippet` y `agent_mcp_status` / `agent_mcp_toggle` ya existen:
  se exponen **en la ficha** (indicador + acción), no en un modal aparte.
- **T2** — En una ficha de chat, ACP y Codex ya reciben `atic-mcp` por
  `mcp_config`: mostrarlo conectado en vez de que el usuario adivine.

### Fase 5 — Lo que ya estaba pendiente (sin fecha)

De `PLAN_AGENTES.md` §"Lo que sigue": interrupt suave sin matar la sesión,
resume de ACP cuando OpenCode/Cursor lo expongan fiable, y los puentes de Atic
(dictado, OCR, captura) como tools del harness.

---

## 6. Qué gana el usuario, en concreto

- **Varios agentes a la vez**, cada uno una ficha, mezclando chat y terminal en
  el mismo rail.
- **Minimizar / agrandar / mover sin miedo**: si el proceso vive en Rust, la
  vista es descartable (Fase 0).
- **Permisos como botones de verdad**, no texto que hay que leer en un TUI.
- **Estado honesto** en la pill: trabajando / te necesita / listo, dicho por el
  agente y no inferido de un SQLite ajeno (Fase 3).
- **El MCP de Atic visible y conectado** en cada sesión (Fase 4).
- **Costo y contexto** por sesión, que el modelo ya recibe (`usage_update`).

---

## 7. Riesgos y trampas

- **El riesgo real es de diseño, no técnico.** Mitigación: la ficha de chat no
  trae mueble propio (§2). Si en la revisión visual aparece chrome nuevo, es
  señal de que se está repitiendo el error de la versión anterior.
- **El chat no cubre todo el CLI.** Login interactivo, instaladores y comandos
  propios del TUI siguen siendo de la PTY. Por eso la PTY no se va.
- **`xterm` no se mueve, se re-monta.** Ya está resuelto con `console_tail` +
  `fitAndResize`; no inventar nada nuevo.
- **La bandera de pegado.** `set_agents_console_open` / `agents_open()` deciden
  si el historial inserta en la consola o manda Ctrl+V afuera. Con fichas de
  chat tiene que seguir significando "hay dónde insertar", incluido el
  composer.
- **Estado fantasma en el desmontaje.** El `onDestroy` vio una lista de fichas
  que ya se había vaciado (§4 D1). Mientras exista esa ventana, ninguna
  decisión destructiva puede depender de estado reactivo leído en un teardown.
- **ACP resume es parcial.** No ofrecer "Continuar" donde no es fiable.

---

## 8. Mapa de archivos

**Fase 0:** `agents/console.rs` (registro de vistas, gc), `ConsolePanel.svelte`,
`AgentLauncher.svelte`, borrar `consoleTransfer.ts` /
`consoleTransfer.svelte.ts` / `consoleTransfer.test.ts`, `PillSurface.svelte`,
`AgentsFloat.svelte`, `AgentsWindow.svelte`.

**Fase 1:** nuevo `features/agents/AgentChatPanel.svelte`;
`ConsolePanel.svelte` (modelo de ficha); `AgentLauncher.svelte` (menú `+`);
reusar `lib/AgentConversation.svelte`, `AgentToolCard`, `AgentMessage`,
`AgentCollabCard`, `features/agents/PermissionBar.svelte`; extraer de
`AgentsDemo.svelte`.

**Fase 2:** nuevo `surfaces/overlay/pill/agentRoster.ts` (+ test);
`pillAgentChip.ts`, `PillSurface.svelte`.

**Fase 3:** `agents/watch_*.rs`, `agents/presence.rs`.

**Fase 4:** `AgentLauncher.svelte`, `McpServersModal.svelte`, `agents/hub/`.

---

## 9. Verificación

- Rust: `cargo check --locked -p atic-desktop` y
  `cargo test --locked -p atic-desktop --lib` por fase.
- Front: `pnpm --dir apps/desktop exec vitest run <ruta>` de lo tocado y
  `pnpm --dir apps/desktop check` al cerrar cada fase.
- Manual, con la app abierta (esto no lo cubre ningún test): despegar/acoplar
  con sesión viva, dos vistas a la vez, permiso contestado desde la barra,
  minimizar/restaurar, y el pegado del historial en los dos tipos de ficha.

---

## 10. Lo que este plan NO resuelve

Worktrees, checkpoints, revisión de diffs, committee/advisor. Siguen fuera,
igual que en `PLAN_AGENTES.md`. La orquestación entre agentes no cambia: es de
`PLAN_ORQUESTACION_MCP.md`, ya implementado.

---

## 11. Bitácora

- **2026-09-20** — Revisión del re-acople de herramientas flotantes. Dos
  arreglos aplicados fuera de este plan, en el árbol de la isla:
  1. El imán del re-acople se unificó en `surfaces/overlay/retachMagnet.ts`
     (puro, con test): historial y textos volvían a pegarse solos porque les
     faltaba el armado del gesto que agentes sí tenía.
  2. El float de agentes sin consola ya puede re-acoplarse (antes
     `detachToLocal` salía en silencio por `!panel`).
- **2026-09-20** — Diagnóstico de D1 con trazas temporales sobre `pill_trace`
  (retiradas al cerrar). Fase 0 · T1 aplicada como mitigación.
- **2026-09-20** — Este plan, escrito tras leer `acp.rs`, `PLAN_AGENTES.md`,
  `PLAN_AGENTES_TUI.md` y el árbol de agentes. Corrección del dueño del
  proyecto: el chat se abandonó por su UI, no por su arquitectura (§2).
- **2026-09-20** — Fase 0 · T2 y T3 implementadas (detalle en §5). Verificado:
  `cargo check` limpio, 10 tests en `agents::console`, `pnpm check` 0 errores,
  376 tests del front, eslint limpio. **Pendiente de prueba manual**: despegue
  con sesión viva, dos vistas a la vez y recarga del overlay con consola
  abierta.
