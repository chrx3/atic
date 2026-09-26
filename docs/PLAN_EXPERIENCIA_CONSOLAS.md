# Plan — Experiencia de las consolas (mando, contexto y entorno)

> **Fecha:** 2026-09-26
> **Estado:** propuesta. No hay código escrito para esto.
>
> Este plan no reemplaza a ninguno; **se apoya en los que ya existen**:
>
> - [`PLAN_CONSOLAS.md`](PLAN_CONSOLAS.md) — la convergencia modelo ⇄ PTY y la
>   propiedad de los procesos. Eso va primero: acá no se toca.
> - [`PLAN_AGENTES_TUI.md`](PLAN_AGENTES_TUI.md) — el pager de la pill.
>   Este plan trabaja **la ventana**, no la pill.
> - [`PLAN_ORQUESTACION_MCP.md`](PLAN_ORQUESTACION_MCP.md) — el hub y su
>   contrato. Acá solo se le suma la **superficie** para mirarlo y configurarlo.
>
> [`PLAN_AGENTES.md`](PLAN_AGENTES.md) §«Lo que sigue» dejó fuera, a propósito,
> **worktrees, checkpoints y diff review**. Este plan es el lugar donde entran,
> con su fase propia (Fase 2), porque sin eso los agentes en paralelo se pisan.

---

## 1. Tesis

Abrir consolas ya está resuelto: la pizarra, los espacios, los threads
padre→hijo, los estados y los cupos existen. El cuello de botella que queda es
otro: **el mando entre consolas y el contexto que se pierde al copiar y pegar**.

En concreto, tres preguntas no se contestan hoy sin abrir cada TUI:

1. **¿Quién me necesita?** (permiso, pregunta, error, terminé).
2. **¿Qué pasó mientras no miraba?** (resumen del turno, qué archivos tocó).
3. **¿Qué contexto le paso a la otra consola?** (el error, el diff, la decisión).

Este plan ordena eso en seis fases, de «menos datos nuevos» a «más estructura
nueva», y marca ★ las cinco que más rinden con lo que ya está construido.

---

## 2. Lo que NO se hace

Escrito temprano, porque es la mitad del valor.

- **No se scrapea ANSI del PTY** para inventar estados. Los estados salen del
  transcript, del hook (`ping.rs`) o del protocolo; nunca de la pantalla.
- **No se inventan métricas.** Costo, cupo y «archivos tocados» solo si la
  fuente los publica (la lección de `quota.rs`).
- **No se roba el foco por defecto.** Auto-focus es una preferencia, no la
  conducta de fábrica.
- **No se toca el contrato MCP ni el hub**, salvo el punto O4 (tabla de ruteo)
  que es config del usuario, no API.
- **No se rehace la pizarra ni el sistema líquido.** Las superficies nuevas se
  montan como floats/paneles de la ventana, con el vocabulario que ya existe.
- **No hay nube ni móvil.** Todo local, como el resto de Atic.

---

## 3. Lo que existe y se reusa

| Pieza | Dónde | Qué aporta |
|---|---|---|
| Pizarra, tarjetas, cámara, espacios | `AgentsBoard.svelte`, `agentBoard.ts`, `BoardList.svelte`, `BoardTree.svelte` | El mueble donde viven todas las ideas |
| PTY + xterm + bus de salida | `agents/console.rs`, `TerminalView.svelte`, `consoleBus.ts` | Salida por sesión; de ahí salen badges y export |
| Transcripts y fin de turno | `watch_claude.rs`, `watch_codex.rs`, `watch_cursor.rs`, `watch_opencode.rs`, `console_agent.rs` | TurnStart/TurnEnd y texto final ya parseados |
| Hook de permisos (Claude) | `agents/ping.rs` | La señal honesta de «esperando» cuando está |
| Presencia de TUIs externas | `presence.rs` | Snapshot para la pill; sirve igual para la ventana |
| Sesiones estructuradas (chat) | `bridge.rs`, `store.rs`, `turns.rs`, `AgentChatPanel.svelte` | Permisos, preguntas, planes, costo, `EditedFiles` |
| Hub y delegación | `agents/hub/` (`api`, `graph`, `wait`, `server`), `crates/atic-mcp` | `delegate`, `wait`, `permission`, profundidad y ciclos |
| Subagentes en su TUI | `console_agent.rs` | Hijos Claude/Codex con turno reconocible |
| Cupos unificados | `quota.rs` | Una forma para cuatro proveedores, con frescura por agente |
| Archivos de la carpeta | `board_files.rs`, `FileView.svelte`, `SessionFiles.svelte` | Ver/abrir; cambios de carpeta detectados |
| Timbres | `beep.rs` + `sound_*` en config | Voz por acción, parametrizable |
| Sistema | `system_control/snapshot.rs` | CPU/RAM por proceso |
| SSH | `agents/ssh.rs`, `Features/ssh-remote-hosts.md` | Hosts, sesiones remotas |
| Diccionario de agentes | `agentCatalog.ts`, `discover.rs` | CLIs, modelos, esfuerzos, instaladores |

---

## 4. Fases

Cada idea lleva: **para qué**, **cómo** (dónde vive y de qué se alimenta) y
**criterio** (cómo se sabe que quedó bien). Talla: `S` ≤ media jornada,
`M` 1–3 días, `L` más. ★ = top 5.

### Fase 1 — Mando: saber qué pasa y quién me necesita

**M1 ★ Bandeja de pendientes en la ventana**
*Para qué:* dejar de revisar consola por consola. Es «la pill, pero con lugar
para contestar».
*Cómo:* panel/float en la ventana (mismo lenguaje que `BoardList`) alimentado
por las tres fuentes que ya existen: pendientes del chat (permisos, preguntas,
planes), `ping.rs` (hook de permiso de Claude) y watchers de transcript para el
resto. Cada fila: agente, consola, tipo, tiempo esperando y Enter = saltar a la
tarjeta (`reveal` ya existe). Orden por antigüedad. Con el filtro de M6.
*Criterio:* con dos chats y dos TUIs, cualquier pendiente se contesta sin abrir
otra consola. Sin señal fiable, no se muestra «esperando» (regla de
`PLAN_AGENTES_TUI`).
Talla `M` · riego bajo · **reusa todo, no inventa datos**.

**M2 ★ Resumen del turno en la tarjeta**
*Para qué:* saber si avanzó sin abrir el TUI.
*Cómo:* al cerrar un turno, los watchers ya tienen el texto final. Guardar por
consola `{inicio, fin, duración, última frase}` y pintarlo en la tarjeta (una
línea, click = ver el detalle/scrollback). Para TUI, mismo camino que
`console_agent.rs`.
*Criterio:* al volver a la ventana, cada tarjeta dice qué hizo el último turno.
Talla `M` · riesgo bajo (dato que ya se parsea).

**M3 Detección de atasco**
*Para qué:* el agente que lleva 20 min «trabajando» en un error en bucle.
*Cómo:* heurística pura y testeada (patrón del repo, como `pillPlan.ts`):
silencio > umbral, misma herramienta repetida, mismo error repetido, pregunta
sin responder. Aviso en la tarjeta con acciones: cancelar turno, reintentar con
una instrucción, escalar a otro agente (M4/O1).
*Criterio:* un turno trabado se detecta solo; los falsos positivos tienen
umbral configurable.
Talla `M` · riesgo medio (heurística; mitigado con tests y umbral).

**M4 Diario por consola**
*Para qué:* «¿en qué se fue la tarde?» y «¿qué herramienta corrió?».
*Cómo:* lista plegable por consola con los turnos (duración, herramientas,
archivos, costo cuando exista). Reusa `turns.rs`, `UsageModal`/`ChatWork` y
`EditedFiles` para el chat; para PTY, transcript.
*Criterio:* con la ventana abierta, se responde sin mirar el TUI.
Talla `M` · riesgo bajo.

**M5 Búsqueda en scrollback y transcripts**
*Para qué:* «¿dónde vi ese error?» entre todas las consolas y sesiones guardadas.
*Cómo:* en dos capas: (a) índice en RAM por consola que va leyendo `consoleBus`
y el buffer de xterm (`@xterm/addon-search` es el camino para buscar adentro);
(b) búsqueda en transcripts ya guardados con los parsers existentes
(`claude_sessions.rs`, etc.). Resultado con salto a la tarjeta y línea.
*Criterio:* el error se encuentra sin recordar en qué consola estaba.
Talla `L` · riesgo medio (volumen de scrollback; se indexa por consola y con tope).

**M6 Filtro y vista «solo lo que espera»**
*Para qué:* una pizarra con 10 tarjetas necesita un modo «pásame solo lo urgente».
*Cómo:* chips de filtro (trabajando / espera / listo / falló / sin leer) y modo
«solo pendientes», alimentados por el mismo estado que las bolitas de
`BoardList`. Con M1 abierto, el filtro también manda en la bandeja.
*Criterio:* con cualquier cantidad de consolas, lo que espera está a la vista en
un clic.
Talla `S` · riesgo bajo.

**M7 Vista foco (una tarjeta grande y legible)**
*Para qué:* leer/demostrar una consola sin pelear con la cámara.
*Cómo:* overlay en la ventana con la tarjeta a pantalla completa y zoom propio
(hoy `ConsolePanel` tiene un solo zoom global, línea 478). Esc vuelve.
*Criterio:* una consola se lee en zoom cómodo sin tocar el zoom de la pizarra.
Talla `M` · riesgo bajo.

### Fase 2 — Contexto y archivos (la fase que desbloquea el paralelismo)

**C1 ★ Pasar contexto entre consolas**
*Para qué:* el dolor más repetido del día: copiar el error/diff/selección y
pegarlo en otra consola «con las palabras correctas».
*Cómo:* acción en el menú de la tarjeta (y atajo) que arma un bloque con
`cwd`, origen y el contenido, y lo inserta con `insertText`/`consoleWrite` en la
consola destino (el composer ya sabe insertar). Fuentes: selección de xterm,
último error del transcript, último diff (C2), o el texto del portapapeles.
*Criterio:* pasar el error de una consola a otra son dos gestos, sin salir de Atic.
Talla `M` · riesgo bajo.

**C2 ★ Diffs por consola y review delegado**
*Para qué:* cerrar el ciclo «¿qué tocó?» → «que lo revise otro» → aplicar.
*Cómo:* para chat ya está `EditedFiles`; para PTY, `git diff --numstat` y
`git status --porcelain` de su `cwd` (proceso `git`, sin crate nuevo) con parseo
puro testeado. Por archivo: ver diff, abrir, pedir review. El review se delega
con el hub (`delegate` ya existe): botón «que lo revise X».
*Criterio:* al terminar un turno, se ve el diff y se lo manda a otro agente sin
copiar nada a mano.
Talla `M` · riesgo medio (repos sin git, diffs enormes; se acota y se degrada
a «no disponible»).

**C3 Aviso de conflicto entre consolas**
*Para qué:* dos agentes tocando el mismo archivo es el origen del merge doloroso.
*Cómo:* con C2 y `board_files.rs` (que ya detecta cambios por carpeta) se
comparan archivos tocados por consola en la misma franja; aviso en ambas
tarjetas y en la bandeja.
*Criterio:* el conflicto se avisa antes de aplicar nada.
Talla `S` · riesgo bajo.

**C4 ★ Worktree por consola**
*Para qué:* paralelizar de verdad en un mismo repo sin que se pisen.
*Cómo:* al abrir una consola sobre un repo (`git rev-parse`), ofrecer «en
worktree»: `git worktree add` con rama propia, la tarjeta muestra el worktree y
su rama, y al cerrar/mergear se ofrece fusionar (`git merge`) o descartar
(`git worktree remove`). Debe ser explícito del usuario: nada de ramas sombra
silenciosas. Lo dejado fuera por `PLAN_AGENTES` y `PLAN_ORQUESTACION_MCP` entra
acá.
*Criterio:* cuatro agentes sobre el mismo repo no comparten archivos por
accidente; merge y descarte son de un clic.
Talla `L` · riesgo medio-alto (estados de git; se testea el parseo y las
decisiones, el proceso se ejercita a mano).

**C5 Checkpoint por consola**
*Para qué:* «aplicó cambios y no me gustaron» sin depender de la memoria del agente.
*Cómo:* antes de un turno que va a tocar disco (o a pedido), snapshot liviano:
`git stash create`/commit temporal en el worktree, o copia de los archivos
detectados por C2. Volver atrás de un clic, con la consola pausada.
*Criterio:* un cambio se revierte sin buscar en el historial de git a mano.
Talla `M` · riesgo medio (interacción con ramas del usuario; siempre con
confirmación y sin tocar trabajo ajeno).

**C6 Abrir archivo en el editor con línea**
*Para qué:* de la salida al editor real, sin buscar la carpeta.
*Cómo:* rutas clicables en las tarjetas de archivo (`FileView`,
`SessionFiles`) y en lo detectado del transcript; `opener` con `vscode://file/…`
o `code -g ruta:línea`, con fallback al visor propio.
*Criterio:* clic en `src/foo.ts:42` abre el editor en esa línea.
Talla `S` · riesgo bajo.

### Fase 3 — Entrada más rápida

**E1 Plantillas de prompt con variables**
*Para qué:* no reescribir el mismo encargo.
*Cómo:* reusar `snippets` (ya tiene UI y persistencia) más un juego de
variables del contexto: `{cwd}`, `{rama}`, `{archivo}`, `{seleccion}`. El
composer ofrece las plantillas al escribir `/` o un prefijo.
*Criterio:* «revisa el diff de {rama}» sale con dos toques.
Talla `M` · riesgo bajo.

**E2 Broadcast con confirmación**
*Para qué:* pedirle lo mismo a varios agentes a la vez (planes, comparar).
*Cómo:* multi-selección en la pizarra → «mandar a N» con confirmación que
enumera destinos. Reusa el envío del composer por sesión.
*Criterio:* un texto llega a las consolas elegidas; ninguna recibe doble.
Talla `S` · riesgo bajo.

**E3 «Repetir con otro agente»**
*Para qué:* comparar respuestas o aprovechar cupo.
*Cómo:* desde el historial del composer (`BoardComposer` ya guarda) y de los
turnos del chat: «repetir en…» con el mismo texto.
*Criterio:* el último prompt se reenvía sin copiarlo.
Talla `S` · riesgo bajo.

**E4 Cola por consola**
*Para qué:* seguir escribiendo mientras trabaja, sin perder el mensaje.
*Cómo:* si la sesión está en turno, el envío entra a una cola por sesión
(visible «1 en cola») y sale al TurnEnd. La cola vive donde vive el estado
(TS para chat, Rust para PTY), con el orden garantizado.
*Criterio:* dos mensajes enviados seguidos llegan en orden y ninguno se pierde.
Talla `M` · riesgo medio (orden y turnos; test de la cola pura).

**E5 `@` rutas con autocompletado**
*Para qué:* referenciar archivos sin tipear la ruta completa.
*Cómo:* el composer completa con `fs_browse.rs`/`FolderBrowser` ya existentes,
con tope de resultados y respetando los `SKIP_DIRS` de `board_files.rs`.
*Criterio:* `@board` ofrece los archivos que empiezan así en el cwd.
Talla `M` · riesgo bajo.

**E6 Dictado → composer**
*Para qué:* ya está en pendientes de `agentes.md`; acá se especifica el destino.
*Cómo:* el dictado existente pega en el composer de la ventana cuando es la
ventana activa (mismo mecanismo de foco/pegado que el historial del
portapapeles).
*Criterio:* dictar con la ventana de agentes al frente escribe en el composer.
Talla `S` · riesgo bajo.

### Fase 4 — Orquestación (la superficie del hub)

**O1 Panel de orquestación**
*Para qué:* los threads de la pizarra cuentan la historia, pero no se puede
leer «quién espera a quién» con 8 nodos.
*Cómo:* panel que dibuja el árbol real del hub (`hub/graph.rs` ya valida
profundidad y ciclos; `api` ya conoce hijos) con estado por nodo, tiempo de
espera y acción «ir». Los hilos de `BoardThreads` quedan como el mismo dato en
la pizarra.
*Criterio:* una cadena padre → hijo → nieto se entiende de un vistazo.
Talla `M` · riesgo bajo.

**O2 Review automático como gesto**
*Para qué:* que el diff de C2 no se revise a ojo cuando hay otro agente gratis.
*Cómo:* acción post-turno «que lo revise <agente>» que arma el encargo con el
diff y delega por el hub (o `console_agent` para un hijo con TUI). El resultado
vuelve como turno del hijo y queda en su tarjeta.
*Criterio:* un turno termina y se pide review en un gesto; el veredicto queda
visible.
Talla `M` · riesgo bajo (el camino ya existe; es UI + armado del texto).

**O3 Presupuestos por cadena**
*Para qué:* que una delegación no se coma la cuota sin avisar.
*Cómo:* tope de profundidad (ya validada), de costo acumulado (turnos con
`costUsd`) y de tiempo; al exceder, corte con `atic_cancel` y aviso. Los topes
son config del usuario; el grafo ya da el alcance.
*Criterio:* una cadena se corta sola al pasar el tope y dice por qué.
Talla `M` · riesgo medio (costos no siempre publicados; se aplica donde exista).

**O4 Perfiles de ruteo**
*Para qué:* «review → Codex, refactor → Claude» sin repetirlo cada vez.
*Cómo:* el `auto` por `kind` de `delegate` (`plan`, `patch`, `review`, `apply`)
hoy es fijo en el sidecar. Se vuelve una tabla del usuario en config, expuesta
por el hub (`default_backend(kind)`), manteniendo el contrato actual.
*Criterio:* cambiar la tabla cambia a quién va cada `auto`, sin tocar código.
Talla `M` · riesgo medio (contrato compartido con `atic-mcp`: solo se agrega
lectura, no se rompe nada).

**O5 Tablilla de contexto compartido**
*Para qué:* el contexto que hoy se repite en cada prompt («la carpeta es X, la
rama es Y, el ticket es Z»).
*Cómo:* una nota de la pizarra (nueva, chica) cuyo texto se adjunta al prompt
con un gesto, más el `AGENTS.md`/`CLAUDE.md` del cwd detectado y ofrecido. Nada
se inyecta sin que el usuario lo vea.
*Criterio:* adjuntar el contexto base es un clic y el prompt lo muestra antes
de enviarse.
Talla `S` · riesgo bajo.

### Fase 5 — Entorno y arranque

**T1 Perfiles de consola**
*Para qué:* abrir «mi stack» (Claude plan + Codex apply + shell) sin repetir
cada elección.
*Cómo:* presets guardados en config/localStorage con CLI, modelo, esfuerzo,
modo de permisos, cwd y env; el menú «Nueva consola» los ofrece y puede abrir
varios de una vez. Reusa `agentCatalog.ts` y el lanzamiento actual.
*Criterio:* un perfil abre la consola lista para trabajar en un gesto.
Talla `M` · riesgo bajo.

**T2 Detección de dev server / puerto**
*Para qué:* el agente levanta algo y no hay que buscarlo.
*Cómo:* sobre `consoleBus`, detectar URLs (`http://localhost:5173`) y avisos de
«listening on» con regex pura testeada; badge en la tarjeta con abrir en el
navegador y copiar. Descartable por consola si molesta.
*Criterio:* el puerto aparece en la tarjeta cuando existe.
Talla `S` · riesgo bajo (falsos positivos acotados por patrón).

**T3 SSH de primera**
*Para qué:* que lo remoto se sienta como lo local.
*Cómo:* retomar/reconectar sesión, cwd remoto como carpeta de la tarjeta y
`spaces` que recuerden el host (los hosts ya están en config; ver
[`ssh-remote-hosts.md`](../Features/ssh-remote-hosts.md)).
*Criterio:* cerrar y reabrir la ventana reata la sesión remota donde estaba.
Talla `M` · riesgo medio (redes); se degrada con aviso, nunca en silencio.

**T4 Semáforo de cupo al crear**
*Para qué:* no abrir una consola con el agente que está al límite.
*Cómo:* `quota.rs` ya unifica; el menú «Nueva consola» muestra el cupo por
agente con su frescura (el dato viejo se dice).
*Criterio:* elegir agente mira el cupo antes, no después del 429.
Talla `S` · riesgo bajo.

**T5 Entorno en la tarjeta**
*Para qué:* saber dónde corre: rama, node/python, venv.
*Cómo:* del `cwd` de la consola, `git rev-parse --abbrev-ref HEAD` y detección
de manifests (`package.json`, `pyproject.toml`, `.venv`) con caché; se pinta
chico en la tarjeta.
*Criterio:* dos consolas en carpetas distintas se distinguen de un vistazo.
Talla `S` · riesgo bajo.

**T6 Exportar consola a Markdown**
*Para qué:* pegar la sesión en un PR/issue, o archivarla.
*Cómo:* el chat ya tiene turnos en `store.rs`; el PTY, buffer de xterm
(`translateToString`) o transcript. Un formato, sin secretos por defecto (los
tokens pegados no se exportan solos: se avisa) .
*Criterio:* «exportar» produce un `.md` legible con lo que se veía.
Talla `S` · riesgo bajo (privacidad: filtro y aviso).

### Fase 6 — Avisos y robustez

**A1 Notificaciones con acción**
*Para qué:* contestar sin llegar a la ventana.
*Cómo:* hoy existe la notificación del sistema para chats («terminó fuera de
vista»). Se extiende a consolas (estado por watchers) y, donde el SO lo permita
(plugin de notificación), con acciones «Ver», «Aprobar», «Cancelar»; el clic
siempre lleva a la tarjeta (`focus.rs` ya sabe enfocar).
*Criterio:* un aviso lleva a la consola correcta; en Windows, aprobar desde el
toast cuando aplique.
Talla `M` · riesgo medio (soporte desparejo de acciones por SO).

**A2 Sonido por agente y evento**
*Para qué:* distinguir quién terminó sin mirar.
*Cómo:* los timbres ya son parametrizables por acción (`sound_*`); se agrega
«agente» como dimensión (o al menos terminó/necesita/falló) reusando `beep.rs`.
*Criterio:* se reconoce el evento por oído, y se puede apagar.
Talla `S` · riesgo bajo.

**A3 Modo concentración / auto-focus**
*Para qué:* que avise sin robar foco, y que al volver te deje donde importa.
*Cómo:* dos preferencias: «no robar foco» (default apagado = no roba) y «al
activar la ventana, ir a lo que espera». La decisión va en un módulo puro
testeado, como `pillPlan.ts`.
*Criterio:* con la preferencia activa, volver a la ventana aterriza en el
pendiente; con la apagada, en la última consola.
Talla `S` · riesgo bajo.

**A4 Aviso de compatibilidad de CLIs**
*Para qué:* los watchers dependen del formato del transcript; si una versión lo
cambia, hoy se degrada en silencio.
*Cómo:* contador de líneas no reconocidas por watcher; si supera un umbral, un
aviso en Diagnóstico y en la tarjeta («no puedo leer el estado de este agente»),
con la versión del CLI a mano (`--version`).
*Criterio:* el estado deja de mentir cuando el CLI cambia.
Talla `S` · riesgo bajo.

**A5 Consumo por consola**
*Para qué:* detectar la consola que se está comiendo la máquina.
*Cómo:* el PID del hijo PTY ya existe en `console.rs`; `system_control/snapshot.rs`
ya resume CPU/RAM por proceso. Se pinta en la tarjeta y se ofrece cerrar
(`console_close`).
*Criterio:* una consola colgada al 100 % se identifica y se cierra sin buscarla
en el administrador de tareas.
Talla `S` · riesgo bajo.

---

## 5. Resumen de todas las ideas

| # | Idea | Fase | Valor | Talla | Riesgo | Se apoya en |
|---|---|---|---|---|---|---|
| M1 ★ | Bandeja de pendientes en la ventana | 1 | alto | M | bajo | watchers, `ping.rs`, turns |
| M2 ★ | Resumen del turno en la tarjeta | 1 | alto | M | bajo | watchers, `console_agent` |
| M3 | Detección de atasco | 1 | medio | M | medio | transcript, decisiones puras |
| M4 | Diario por consola | 1 | medio | M | bajo | `turns.rs`, usage, `EditedFiles` |
| M5 | Búsqueda en scrollback y transcripts | 1 | medio | L | medio | `consoleBus`, xterm, parsers |
| M6 | Filtro «solo lo que espera» | 1 | medio | S | bajo | estados de `BoardList` |
| M7 | Vista foco | 1 | medio | M | bajo | `AgentsBoard`, zoom |
| C1 ★ | Pasar contexto entre consolas | 2 | alto | M | bajo | composer, transcript, xterm |
| C2 ★ | Diffs por consola + review delegado | 2 | alto | M | medio | `EditedFiles`, git, hub |
| C3 | Aviso de conflicto entre consolas | 2 | medio | S | bajo | C2, `board_files.rs` |
| C4 ★ | Worktree por consola | 2 | alto | L | medio-alto | git, `AgentsBoard` |
| C5 | Checkpoint por consola | 2 | medio | M | medio | C2/C4 |
| C6 | Abrir archivo en el editor con línea | 2 | medio | S | bajo | `FileView`, opener |
| E1 | Plantillas de prompt con variables | 3 | medio | M | bajo | `snippets` |
| E2 | Broadcast con confirmación | 3 | medio | S | bajo | composer |
| E3 | Repetir con otro agente | 3 | medio | S | bajo | historial, turnos |
| E4 | Cola por consola | 3 | medio | M | medio | console/bridge |
| E5 | `@` rutas con autocompletado | 3 | bajo | M | bajo | `fs_browse.rs` |
| E6 | Dictado → composer | 3 | medio | S | bajo | dictado |
| O1 | Panel de orquestación | 4 | alto | M | bajo | `hub/graph`, `api` |
| O2 | Review automático como gesto | 4 | medio | M | bajo | hub, C2 |
| O3 | Presupuestos por cadena | 4 | medio | M | medio | grafo, costos |
| O4 | Perfiles de ruteo | 4 | medio | M | medio | `delegate`, hub |
| O5 | Tablilla de contexto compartido | 4 | bajo | S | bajo | pizarra |
| T1 | Perfiles de consola | 5 | alto | M | bajo | `agentCatalog.ts` |
| T2 | Detección de dev server / puerto | 5 | medio | S | bajo | `consoleBus` |
| T3 | SSH de primera | 5 | medio | M | medio | `ssh.rs` |
| T4 | Semáforo de cupo al crear | 5 | bajo | S | bajo | `quota.rs` |
| T5 | Entorno en la tarjeta | 5 | bajo | S | bajo | cwd, git |
| T6 | Exportar consola a Markdown | 5 | bajo | S | bajo | store/xterm |
| A1 | Notificaciones con acción | 6 | medio | M | medio | notificación + `focus.rs` |
| A2 | Sonido por agente y evento | 6 | bajo | S | bajo | `beep.rs` |
| A3 | Modo concentración / auto-focus | 6 | bajo | S | bajo | decisión pura |
| A4 | Aviso de compatibilidad de CLIs | 6 | medio | S | bajo | watchers |
| A5 | Consumo por consola | 6 | bajo | S | bajo | `snapshot.rs`, PTY |

**Tallas:** 15 `S` · 18 `M` · 2 `L`. Nada de esto toca el modelo de sesiones ni
el contrato MCP.

---

## 6. Orden recomendado

Las ★ en este orden. Cada una habilita la siguiente:

1. **M1 — Bandeja de pendientes.** No necesita datos nuevos y cambia el día a
   día. Habilita M6 (el filtro manda en la bandeja) y A1.
2. **C1 — Pasar contexto.** Dos gestos en vez de copy/paste. Base de O2 y C3.
3. **M2 — Resumen del turno.** Deja de abrir TUIs para saber si avanzó. Base de
   M3 y M4.
4. **C4 — Worktree por consola.** Desbloquea el paralelismo real; es la más
   grande y por eso va con la base ya probada (C1/C2 ayudan en el merge).
5. **C2 — Diffs + review delegado.** Cierra el ciclo sin salir de Atic.

Después: O1 (panel) cuando haya cadenas de verdad; T1 (perfiles) cuando el
arranque duela; el resto por dolor, no por orden.

---

## 7. Cómo se prueba

- **Decisiones puras con tests** (patrón del repo): filtros y umbrales (M3, M6),
  parseo de git (C2), parseo de URLs (T2), cola (E4), auto-focus (A3), frescura
  de datos de cupo (T4). En TS donde ya vive la decisión (`pillPlan.ts`,
  `agentBoard.ts`), en Rust donde el dato es del sistema (`agents/`).
- **Manual con la app abierta** para todo lo que es gesto: pasar contexto,
  bandeja, worktree, review. Se anota en la bitácora con fecha y escenario.
- **Sin red y sin git** deben degradar con aviso, nunca con un número inventado
  ni un silencio.
- Validación de cierre por zona: `pnpm --dir apps/desktop check` +
  `cargo test --locked -p atic-desktop agents::` para Rust.

---

## 8. Bitácora

- **2026-09-26** — Plan escrito (propuesta). Origen: lluvia de ideas sobre la
  ventana de consolas. Sin código.
