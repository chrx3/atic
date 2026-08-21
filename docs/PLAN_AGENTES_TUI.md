# Plan — El agente vive en su TUI, la pill es el pager

> **Estado: propuesta.** No hay una línea de código escrita para esto.
> Este documento es para aprobar o rechazar antes de tocar el árbol.
>
> No es la continuación de [`PLAN_AGENTES.md`](PLAN_AGENTES.md). Ese plan
> construye el **chat de Atic** (ACP, transcript, permisos, historial) y está
> hecho. Este construye otra cosa, encima del mismo vocabulario.

---

## 1. Tesis

Los agentes siguen corriendo donde ya corren: Windows Terminal, WezTerm, o el
PTY que Atic ya embebe. Atic no los spawnea, no les habla, no les lee la
pantalla. Lo único que hace es **mirar el rastro que el CLI ya escribe en
disco** y traducirlo a tres estados en la pill: trabajando, te necesita, listo.
El clic del chip **enfoca la ventana del agente** y baja el contador; no abre el
chat de Atic. Fase 1 es Claude Code solo, en Windows, leyendo su JSONL vivo.
Fase 2 agrega el hook, que es lo único que puede decir «permiso» con honestidad.
Fase 3 mira si los otros tres CLIs dejan un rastro equivalente. Queda afuera
todo lo que implique que Atic sea la interfaz del agente.

En una frase: **el TUI es el trabajo; la pill es el pager**, y el pager no
inventa lo que no puede saber.

---

## 2. Lo que NO se hace

Escrito primero, porque es la mitad del valor del plan.

- **No se scrapea ANSI del PTY.** Ni del embebido ni de ninguno.
- **No se spawnea TUI y protocolo a la vez sobre el mismo proceso.** Una sesión
  es de la TUI o es del chat de Atic; nunca las dos.
- **No se pinta transcript, tools ni markdown en la pill.** El chip tiene
  ícono + una línea de ≤28 caracteres, que es lo que ya tiene hoy.
- **No se reemplaza la TUI por el chat de Atic «para que reporte».**
- **No se muestra `waiting` sin señal fiable.** Sin hook, el estado más fuerte
  que existe es «trabajando hace rato». Ver §5.4.
- **No se abre `AgentAuthCard` para una sesión de TUI.** Esa tarjeta tiene
  botones que contestan por un canal que en modo sidecar no existe. Duplicar el
  diálogo del CLI con botones muertos es peor que no tener nada.
- **No se toca** el morph del launcher, la rueda, el sistema líquido, el
  historial de hilos ni nada de `PLAN_AGENTES.md`.
- **No se reabre la herramienta de agentes en la rueda del instalador.** Eso es
  una decisión de producto aparte (ver §4.3, no la resuelve este plan).
- **No hay migración de `atic.db3`.** La presencia es memoria + archivos
  ajenos. Si algún día hay que persistirla, lo decide un humano en otro plan.

---

## 3. Lo que ya existe y se usa tal cual

| Pieza | Dónde | Qué aporta acá |
|---|---|---|
| Codificación del cwd y localización del proyecto | `agents/claude_sessions.rs` (`encode_project_key`, `project_dir_for`, `absolute_cwd`, `strip_verbatim`) | Encontrar los `.jsonl` de una carpeta. Ya resuelve el `\\?\` y el `C--`/`c--` de Windows. |
| Parser best-effort del JSONL | `agents/claude_sessions.rs` (`extract_user_text`, `apply_assistant_line`) | La forma de leer las líneas ya está escrita y probada contra archivos reales. El watcher necesita **mucho menos** que eso. |
| Traer una ventana al frente | `clipboard_history::force_foreground` (`#[cfg(windows)]`, ya `pub`) | `AllowSetForegroundWindow` + `AttachThreadInput` + `BringWindowToTop`. Es el conocimiento caro del repo; no se reescribe. |
| Saber si un HWND es nuestro | `clipboard_history::is_own_app_hwnd` | Evita que «enfocar el agente» traiga a Atic al frente. |
| Chip de la pill | `PillSurface.svelte` (`.p-agent`, `agentAlert`/`agentWorking`/`agentReady`/`agentReadyLabel`) | El vocabulario visual ya existe, con su `prefers-reduced-motion` y su pulso. Se le cambia la **fuente** y el **destino del clic**, no la forma. |
| Decisiones puras + tests | `pill/pillPlan.ts` + `pillPlan.test.ts` | El patrón del repo: la decisión es TS puro y testeado, el componente solo la ejecuta. Lo nuevo entra ahí. |
| Reenvío de argv de una segunda instancia | `lib.rs:134`, `tauri_plugin_single_instance::init(|app, _args, _cwd| …)` | Hoy ignora `_args`. Es el candidato #1 para el transporte del hook (§6). |

---

## 4. El hallazgo que ordena este plan

### 4.1 El JSONL de Claude Code está vivo, y tiene un marcador de fin de turno

Verificado **en esta máquina** contra `claude 2.1.233`, sobre 70 archivos de
`~/.claude/projects/`, incluido el de la sesión que escribió este documento
mientras la escribía:

| Hecho | Cómo se verificó |
|---|---|
| Las líneas se **anexan durante el turno**, no al final | El `.jsonl` de la sesión en curso tenía 126 líneas con el turno todavía abierto |
| Un turno arranca con `type:"user"` + `promptSource` (`"typed"`, `"system"`) | 25 líneas `user` en la sesión, de las cuales **1** traía `promptSource`; las otras 24 traían `toolUseResult` + `sourceToolAssistantUUID` (resultados de tool) |
| Un turno termina con `assistant` cuyo `message.stop_reason` **no** es `"tool_use"` | En tres sesiones cerradas: `{tool_use: 600, end_turn: 71}`, `{tool_use: 298, end_turn: 7}`, `{tool_use: 472, end_turn: 64, stop_sequence: 3}` |
| Los subagentes ensucian la cuenta | 71 `end_turn` contra 41 prompts tipeados en la misma sesión: las líneas con `isSidechain: true` también cierran turno. `claude_sessions.rs` ya las saltea; el watcher hace lo mismo |
| Cada línea trae `cwd`, `sessionId`, `version`, `gitBranch` | Presentes en 79 de 126 líneas del archivo vivo |
| Hay líneas de control que no son diálogo | `mode`, `permission-mode`, `last-prompt`, `ai-title`, `attachment`, `file-history-snapshot`, `agent-name` |
| Hay archivos degenerados | 2 de los 5 revisados no tenían ninguna línea `assistant` (sesiones abortadas) |
| **No hay ninguna línea que diga «estoy esperando permiso»** | Ningún tipo observado lo expresa. `permission-mode` es un cambio de modo, no un pedido |

**Lo que esto significa:** `working` y `ready` salen del archivo con precisión
razonable. `waiting` **no sale del archivo**. Es exactamente el reparto que
justifica que la Fase 2 exista.

> ⚠️ Esto es formato interno, no contrato público. Está verificado en una
> máquina y una versión. **Verificar en docs/CHANGELOG del CLI** antes de
> codear, y tratarlo como en `claude_sessions.rs`: si el shape cambia, la
> feature se degrada (a «vivo / no vivo») y no se rompe.

### 4.2 Dónde persiste cada CLI (Fase 3)

Verificado por inspección del disco de esta máquina, no por documentación:

| CLI | Estado local | Forma | Sirve para el pager |
|---|---|---|---|
| Claude Code | `~/.claude/projects/<cwd>/<uuid>.jsonl` | JSONL append-only, vivo | **Sí** — es el MVP |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` | JSONL, un archivo por sesión, carpetas por fecha | Probablemente. Falta inspeccionar el shape de las líneas |
| Cursor | `~/.cursor/chats/<hash>/<uuid>/` y `~/.cursor/acp-sessions/<uuid>/` | Carpeta por sesión | Falta inspeccionar qué hay dentro |
| OpenCode | `~/.local/share/opencode/opencode.db` (+ `-wal`, `-shm`) | **SQLite** | Distinto problema: no se tailea, se consulta. Solo lectura, y el WAL de otro proceso obliga a abrir en modo read-only |

Nota al margen: `cursor-agent` y `codex` **sí** están instalados en esta
máquina, al revés de lo que dice `PLAN_AGENTES.md`.

### 4.3 La feature está apagada

`AGENTS_ENABLED = false` (`core/tools.ts:16`) y su gemelo `UI_ENABLED = false`
(`agents/mod.rs:47`). El chip `.p-agent` es hoy código muerto: la condición
`agentAlert` empieza con `AGENTS_ENABLED &&`. En cambio `agents.init({notify:true})`
**sí** corre siempre (`PillSurface.svelte:1690`), así que el store del chat está
vivo aunque nadie lo mire.

**Decisión (justificada, no pregunta):** el pager necesita su propio
interruptor, `AGENT_PAGER_ENABLED`, gemelo en Rust. El motivo no es
administrativo: la consola se ocultó porque expone a Atic como interfaz de
chat, y el pager **no expone nada de eso** — no hay compositor, no hay
transcript, no hay permisos. Son dos features distintas y merecen dos
interruptores. `agentAlert` pasa a ser la unión de las dos fuentes, cada una
con su flag.

---

## 5. Fase 0 — El contrato de presencia

**Tamaño: S.**

### 5.1 Por qué NO se reusa `AgentDelta`

La opción 1 del encargo era reusar `AgentDelta` reducido. No sobrevive al
examen, por tres razones concretas:

1. **Para encender `unread` habría que fabricar un item de mensaje.**
   `agentSessions.svelte.ts` sube `unread` en `item.add` de un
   `message/assistant` no-streaming, o en un `item.patch` con
   `streaming:false`. Es decir: habría que inventar un `Item` con texto — un
   pedazo de transcript falso — para mover un contador.
2. **Para encender `waiting` habría que fabricar un `ItemKind::Permission`,
   y eso dispara una UI que no podemos sostener.** `primaryPending` alimenta
   `showAuthCard` → `AgentAuthCard` con botones allow/deny. En modo sidecar no
   hay canal para contestar: el permiso lo tiene el teclado de la TUI. Sería
   exactamente el anti-goal «duplicar el diálogo de permisos sin canal fiable»,
   construido a propósito.
3. **`AgentDelta` no tiene dónde poner lo único nuevo que hay:** pid, hwnd y
   «esto es una TUI, no un hilo de Atic».

O sea: `AgentDelta` obliga a inventar turnos, items y permisos falsos, y uno de
esos inventos enciende una tarjeta con botones muertos. Se va por la opción 2.

### 5.2 `AgentPresence`

Tipo nuevo, chico, en `agents/presence.rs`. Deliberadamente **no** es un delta:

```rust
/// Un agente corriendo en SU terminal. Atic solo mira.
pub struct AgentPresence {
    /// Clave estable. Claude Code: el id de sesión del CLI (nombre del .jsonl).
    pub id: String,
    pub backend_id: String,   // "claude-code" | "codex" | …
    pub backend_name: String, // "Claude Code"
    pub cwd: String,
    pub status: PresenceStatus,
    /// Primera línea del último mensaje del agente, cruda y con tope ~120.
    /// El recorte a 28 lo hace la vista, con la MISMA función que `readyLabel`.
    pub preview: Option<String>,
    /// Última señal, epoch secs. Es lo que permite decir «hace 4 min».
    pub updated_at: i64,
    /// Cómo enfocar la TUI. `None` = no se pudo resolver (ver §7).
    pub window: Option<PresenceWindow>, // { pid: u32, hwnd: isize }
    /// De dónde salió el estado. `waiting` SOLO es legítimo con `Hook`.
    pub source: PresenceSource,         // Jsonl | Hook | Process
}

pub enum PresenceStatus { Working, Waiting, Ready, Idle }
```

Tres decisiones dentro del tipo, cada una con su porqué:

- **`unread` no está.** Es del que mira, no del que trabaja. Dos ventanas de
  Atic tienen contadores distintos sobre la misma sesión, igual que hoy con
  `watching` en el store del chat. Vive en TypeScript.
- **`source` está.** Es lo que hace imposible mentir por accidente: el
  reductor rechaza `Waiting` si `source != Hook`. La honestidad queda en el
  tipo, no en la disciplina de quien escriba el próximo backend.
- **`preview` viaja sin recortar.** El recorte a 28 caracteres con `…` es una
  regla visual y tiene que existir **una sola vez**. Se extrae la de
  `readyLabel` a una función pura compartida.

### 5.3 El cable: snapshot, no deltas

Evento `agent-presence` con la **lista completa** de presencias, más un comando
`agent_presences()` para el montaje.

El chat necesita deltas porque un transcript es grande, mutable y no se puede
retransmitir a cada cambio. La presencia es lo contrario: cinco campos por
sesión y a lo sumo un puñado de sesiones. Un snapshot completo a ≤2 Hz es más
barato que la maquinaria de deltas, es idempotente (un evento perdido se corrige
solo en el siguiente) y una ventana que monta tarde no necesita ninguna danza de
adopción. Es la misma economía que ya se aplicó al revés en `bridge.rs`.

### 5.4 Convivencia con el chat de Atic

Tres reglas, todas testeables sin DOM:

1. **Claves distintas, listas distintas.** El chip de la pill agrega las dos
   fuentes; nada más las mezcla.
2. **Prioridad de estado:** `waiting` > `working` > `ready` > nada. A igualdad,
   **gana el chat**, porque de una sesión de Atic sabemos con certeza cómo
   mostrarla, y de una TUI dependemos de resolver una ventana.
3. **El clic va al ganador.** Si el ganador es una sesión de chat →
   `openAgentsConsole()` como hoy. Si es una presencia → enfocar su ventana.
   Nunca lo otro por accidente.

Y la regla que evita el doble conteo, que es un bug garantizado si no se
escribe ahora: **el watcher ignora los `sessionId` que son `providerSession` de
una sesión viva de Atic.** Cuando el chat de Atic corre `claude --resume`, ese
proceso escribe en el MISMO directorio de proyecto. Sin este filtro, una sola
conversación aparecería dos veces en la pill —una como chat, otra como TUI— y
la de TUI enfocaría… Atic. `bridge.rs` ya conoce esos ids.

### 5.5 Ciclo de vida

| Transición | Cuándo |
|---|---|
| Aparece | Un `.jsonl` con mtime dentro de la ventana viva (propuesta: 15 min) o un ping de hook |
| `Working` | Línea de prompt de usuario, o cualquier línea nueva con el turno abierto |
| `Ready` | `stop_reason != "tool_use"` en una línea `assistant` no-sidechain |
| `Waiting` | **Solo** hook (Fase 2) |
| `Idle` | `Ready` ya visto por el usuario: la presencia sigue en la lista, el chip se apaga |
| Desaparece | Sin señal por 30 min, o proceso muerto + `Ready` ya visto |

Con `Working` y silencio prolongado **no se cambia de estado**: sigue
`Working`, y el tooltip dice «sin novedades hace Xm». Es lo honesto: un silencio
puede ser un permiso pendiente o un `cargo build` de ocho minutos, y el archivo
no distingue.

---

## 6. Fase 1 — MVP Claude Code

**Tamaño: 1a = M · 1b = M.** Se pueden entregar por separado; 1a sola ya es
útil (semáforo sin foco).

### 6.1a El watcher

Un hilo dedicado, poll cada ~1 s, sin dependencias nuevas.

**Por qué poll y no `notify`:** sería una dependencia nueva, y
`ReadDirectoryChangesW` sobre carpetas sincronizadas es notoriamente ruidoso.
Un `stat` por directorio de proyecto reciente cuesta nada. El repo ya resuelve
así en `discover.rs` (caché con TTL) y en `console.rs` (hilos con `stop`
atómico).

Estrategia de barrido, en dos niveles para no leer 70 archivos por segundo:

1. `read_dir` de `~/.claude/projects` → `stat` de cada subdirectorio; descartar
   los que no se tocaron en la ventana viva.
2. Dentro de los vivos, `stat` de los `.jsonl`; para los que crecieron, leer
   **desde el offset guardado** hasta el final. Nunca releer el archivo entero:
   una sesión larga son megabytes.
3. Por cada línea nueva: `serde_json` tolerante (línea ilegible → se saltea, es
   lo que ya hace `load_transcript`), saltear `isSidechain: true`, y aplicar la
   tabla de §5.5.

Un detalle que hay que respetar: la última línea puede estar **a medio
escribir**. Si el parse falla en la última, no se avanza el offset y se
reintenta en el próximo tick.

### 6.1b Debounce, para no spamear la pill

Cada cambio de `agentAlert` puede provocar un reencuadre de la pill
(`morphsInPlace` en `pillPlan.ts`), y durante un turno activo llegan líneas cada
pocos cientos de milisegundos.

Dos frenos, los dos en Rust:

- **Coalescer:** máximo un `agent-presence` cada 400 ms, y **solo si cambió
  algo que se ve** (status o preview). El `updated_at` solo no emite.
- **Histéresis en `Ready`:** el preview se fija al cerrar el turno y no se
  reescribe hasta el próximo turno. Sin esto, la etiqueta de 28 caracteres
  parpadearía con cada bloque de texto.

El coalescer es una función pura con reloj inyectado → test directo.

### 6.1c El preview

Primera línea del último bloque `text` del mensaje que cerró el turno, cruda
hasta ~120 caracteres. La vista aplica el **mismo** recorte que hoy hace
`readyLabel` (28 chars + `…`), extraído a una función pura compartida por las
dos fuentes. Una sola definición de la regla visual.

### 6.2 El chip

Cambios chicos en `PillSurface.svelte`, con la decisión afuera:

```ts
// pill/pillAgentChip.ts  (nuevo, puro, testeado)
export type ChipTone = "waiting" | "working" | "ready" | "count" | "off";
export type ChipTarget =
  | { kind: "console" }                    // sesión de chat de Atic
  | { kind: "focus"; presenceId: string }  // TUI externa
  | { kind: "none" };

export function agentChip(state: {
  chat: { unread: number; working: boolean; waiting: number; readyLabel: string | null };
  presence: PresenceView[];
  chatEnabled: boolean;
  pagerEnabled: boolean;
}): { tone: ChipTone; label: string | null; target: ChipTarget };
```

En el componente sobrevive solo la ejecución: `agentAlert` pasa a ser
`chip.tone !== "off"`, el `onclick` despacha según `chip.target`, y las clases
`is-waiting` / `is-working` / `is-ready` / `is-count` salen de `chip.tone`. El
CSS, las animaciones y el bloque de `prefers-reduced-motion` **no se tocan**.

`aria-label` deja de ser fijo: con destino `focus` dice «Ir a Claude Code en su
terminal», no «Abrir la consola de agentes», que sería mentira.

### 6.3 El clic

1. Resolver el HWND (§7).
2. `force_foreground(hwnd)` y **verificar** con `GetForegroundWindow()`, tal
   como ya hace `restore_foreground_hwnd`: `SetForegroundWindow` falla en
   silencio y hay que enterarse.
3. **Solo si el foco se confirmó**, `unread = 0`. Si no llevamos al usuario a
   ningún lado, no se le borra el aviso.
4. Si el HWND resuelto es de Atic (`is_own_app_hwnd`), no es una TUI externa:
   es un agente corriendo en la consola PTY embebida. Ahí el destino correcto
   es abrir/traer el float de consola — que **es** su TUI, no el chat.

---

## 7. El foco de ventana en Windows

### 7.1 Resolver el HWND

Traer la ventana al frente ya está resuelto (`force_foreground`). Lo que falta
es **qué** ventana.

Camino automático, para `PresenceWindow.hwnd`:

1. Enumerar procesos del agente (`claude.exe`) y armar el mapa `pid → ppid`
   con `CreateToolhelp32Snapshot`.
2. Desde el pid del agente, **subir por los padres** hasta encontrar un
   ancestro con ventana top-level: `EnumWindows` + `GetWindowThreadProcessId`,
   filtrando por `IsWindowVisible` y sin owner. La cadena típica es
   `claude.exe → pwsh.exe → OpenConsole.exe → WindowsTerminal.exe`, y el HWND
   está al final.
3. Primer acierto gana.

Esto necesita agregar la feature `Win32_System_Diagnostics_ToolHelp` a
`windows-sys`, que **ya es dependencia**. No entra ninguna crate nueva.

> **Verificar antes de codear:** que la cadena de padres sobreviva a ConPTY.
> WT lanza la shell a través de `OpenConsole.exe` y no está garantizado que el
> ppid quede colgando del proceso de la ventana. Es un experimento de veinte
> minutos con Process Explorer y decide el diseño.

### 7.2 El problema que no tiene solución elegante

**No hay forma de atar un `.jsonl` a un pid.** El archivo trae `sessionId`,
`cwd`, `version` y `gitBranch`; no trae pid. Y leer el cwd de un proceso ajeno
en Windows exige `NtQueryInformationProcess` + leer el PEB — API no
documentada, frágil, y desproporcionada para esto.

Tres respuestas, en orden de honestidad:

| Camino | Cuándo aplica | Costo |
|---|---|---|
| **Único vivo** | Un solo `claude.exe` vivo y un solo `.jsonl` activo → se atan | Gratis. Cubre el caso más común |
| **Vinculación manual** | Cualquier otro caso. Acción «Vincular ventana»: con la TUI al frente, el usuario la fija y Atic guarda el HWND | S. Es la respuesta general del MVP |
| **Pid del hook** | Fase 2: el hook manda su propio pid y la atadura es exacta | Sale gratis con la Fase 2 |

Que el hook resuelva **también** esto es un argumento fuerte para hacer la
Fase 2 temprano, no solo por `waiting`.

### 7.3 Fallback cuando no hay ventana

Chip con destino `{kind:"none"}`: sigue mostrando el estado (es información
útil por sí sola), y al clic **no abre nada**. Muestra el hint de vincular y
deja el `unread` intacto. Explícitamente: **no cae al chat de Atic**. Abrir un
chat vacío cuando el usuario pidió «llevame a mi agente» es peor que no hacer
nada, porque enseña que el chip es impredecible.

Complemento barato y educado, si el foco falla: `FlashWindowEx` sobre el HWND
—parpadea en la barra de tareas— en vez de forzar. Está en la feature de
`windows-sys` que ya está activa.

### 7.4 macOS

Fuera del MVP, sin bloquearlo. La parte de estado (JSONL) es idéntica: rutas
POSIX y el mismo formato. La parte de foco es otro problema —
`NSRunningApplication.activate` llega a la app, no a la pestaña, y tampoco hay
mapeo sesión→ventana. Se documenta como «en progreso», igual que el audio de
sistema.

---

## 8. Fase 2 — El hook `agent-ping`

**Tamaño: S si los hooks son lo que parecen; M con el transporte.**
**Opcional en el sentido estricto: sin esto, `waiting` no existe.**

### 8.1 Lo que hay que verificar SÍ O SÍ (no está en el árbol)

En esta máquina, `~/.claude/settings.json` tiene
`['permissions','model','enableWorkflows','enabledPlugins','effortLevel','skipDangerousModePermissionPrompt']`
— **no hay clave `hooks`**. Así que del árbol no sale nada. Antes de codear,
**verificar en docs/CLI**:

1. Qué eventos existen y cómo se llaman exactamente (`Stop`, `Notification`,
   ¿algo alrededor del permiso?).
2. Si el `command` puede ser un ejecutable arbitrario, y qué recibe: ¿argv?
   ¿JSON por stdin? ¿variables de entorno?
3. Si el payload trae `session_id` y `cwd` (los dos son necesarios: sin
   `session_id` no se puede casar con el `.jsonl`).
4. Si el hook es **bloqueante** — si el CLI espera a que el proceso termine,
   el pager no puede costar 300 ms por evento.
5. Si Atic puede **escribir** la config del hook o solo instruir al usuario.
   Escribir en el `settings.json` de otra herramienta sin pedir permiso va en
   contra del principio 5 (sugerir, no forzar): el default debería ser mostrar
   el fragmento para pegar.

### 8.2 El transporte, decidido con una medición

El comando IPC son tres campos: `session_id`, `status`, `preview?` — más el pid
del proceso que llama, que es el regalo del §7.2.

| Opción | A favor | En contra |
|---|---|---|
| **1. `Atic.exe agent-ping …`** vía el reenvío de `tauri-plugin-single-instance` (ya registrado, `_args` hoy ignorado) | Cero dependencias, cero binarios nuevos, cero superficie de red | Lanza el binario grande de la GUI por evento. **Hay que medir el costo en frío** |
| **2. Named pipe + un bin chico `atic-ping.exe`** | Milisegundos, exacto | Un target más en el workspace y en el instalador |
| **3. Hook que anexa a un archivo** que el watcher ya tailea | Sin IPC ninguno | El quoting del one-liner en Windows y el reenvío de stdin lo vuelven frágil |

**Decisión: arrancar por la 1, con la 2 como plan B si la medición da mal.** No
es una pregunta abierta, es una compuerta con criterio: si un ping cuesta más
de ~150 ms o mete al usuario un parpadeo de ventana, se cambia.

### 8.3 Qué desbloquea

- `waiting` de verdad («te necesita»), con `source = Hook`.
- Atadura exacta pid→sesión, que arregla el §7.2 sin heurísticas.
- `ready` inmediato y sin ambigüedad, sin depender de leer `stop_reason`.

Sin hook: se queda en dos estados y el plan sigue siendo útil. Con hook: los
tres, y bien.

---

## 9. Fase 3 — Codex, Cursor, OpenCode

**Tamaño: M–L.** Un backend por vez, en su propio archivo.

La regla es la misma para los tres: **si no hay rastro fiable, la presencia es
solo «proceso vivo + cwd»**, con estado `Working` mientras vive y nada más. Es
poco, pero es honesto y ya sirve para el foco.

| CLI | Punto de partida (verificado en disco) | Qué hay que investigar |
|---|---|---|
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | Shape de las líneas; ¿hay marcador de fin de turno? Ojo: el app-server es `[experimental]` y el rollout puede cambiar con él |
| Cursor | `~/.cursor/chats/<hash>/<uuid>/` y `~/.cursor/acp-sessions/<uuid>/` | Qué archivo hay dentro y si se escribe en vivo. `~/.cursor/agent-cli-state.json` es solo config de UI |
| OpenCode | `~/.local/share/opencode/opencode.db` (+WAL) | **Solo lectura**, y abrir SQLite de otro proceso con WAL activo tiene sus reglas. Si hace falta escribir algo, lo decide un humano — este plan no lo autoriza |

Prohibido en esta fase, igual que en las otras: scrapear la TUI, y forzar ACP
en paralelo sobre el mismo proceso.

---

## 10. Mapa de archivos

### Fase 0 — contrato

| Archivo | Qué |
|---|---|
| `apps/desktop/src-tauri/src/agents/presence.rs` | **nuevo.** Tipos, registro en memoria, coalescer, evento `agent-presence`, comandos `agent_presences` / `agent_presence_focus` / `agent_presence_bind` |
| `apps/desktop/src-tauri/src/agents/mod.rs` | +`pub mod presence;` +`pub const PAGER_ENABLED` |
| `apps/desktop/src-tauri/src/lib.rs` | Registrar los comandos nuevos; arrancar el watcher en `setup` |
| `apps/desktop/src/lib/core/types.ts` | `AgentPresence`, `PresenceStatus`, `PresenceSource` |
| `apps/desktop/src/lib/ipc/events.ts` | `"agent-presence": AgentPresence[]` en `AticEvents` |
| `apps/desktop/src/lib/core/tools.ts` | `AGENT_PAGER_ENABLED` |
| `apps/desktop/src/lib/agentPresence.svelte.ts` | **nuevo.** Store chico: lista + `unread` por sesión + `watching` |

### Fase 1 — MVP

| Archivo | Qué |
|---|---|
| `apps/desktop/src-tauri/src/agents/watch_claude.rs` | **nuevo.** Barrido, tail por offset, línea → estado |
| `apps/desktop/src-tauri/src/agents/focus.rs` | **nuevo.** `trait WindowFocus` + impl Win32 + fake para tests. Delega en `clipboard_history::force_foreground` |
| `apps/desktop/src-tauri/src/agents/claude_sessions.rs` | Reusar helpers; hacer `pub(crate)` lo mínimo |
| `apps/desktop/src-tauri/src/agents/bridge.rs` | Exponer los `providerSession` vivos para el filtro anti-doble-conteo |
| `apps/desktop/src-tauri/Cargo.toml` | +feature `Win32_System_Diagnostics_ToolHelp` en `windows-sys` (dep existente) |
| `apps/desktop/src/lib/surfaces/overlay/pill/pillAgentChip.ts` | **nuevo, puro.** Unión de fuentes → tono, etiqueta y destino |
| `apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte` | Los `$derived` del chip pasan por `agentChip()`; el `onclick` despacha por destino. CSS intacto |
| `apps/desktop/src/lib/agentSessions.svelte.ts` | Extraer el recorte de `readyLabel` a la función compartida (sin cambiar comportamiento) |

### Fase 2 — hook

| Archivo | Qué |
|---|---|
| `apps/desktop/src-tauri/src/lib.rs` | El closure de `single_instance` deja de ignorar `_args` |
| `apps/desktop/src-tauri/src/agents/presence.rs` | Entrada `ping(session, status, preview, pid)` |
| Ajustes (UI) | Mostrar el fragmento de `settings.json` para pegar, y un botón «copiar». No escribir el archivo ajeno sin pedirlo |

### Fase 3

Un `watch_<backend>.rs` por CLI, detrás del mismo `AgentPresence`.

---

## 11. Riesgos

1. **El JSONL es formato interno.** Verificado en 2.1.233 y en una máquina. Si
   cambia, el pager tiene que degradarse —«hay una sesión viva», sin
   estado— y nunca romper. Mitigación: parser tolerante como el que ya existe,
   feature detrás de flag, y un test con fixture de línea desconocida.
2. **Doble conteo con el chat de Atic** (§5.4). Es un bug garantizado si el
   filtro de `providerSession` no entra en la Fase 0.
3. **Dos sesiones en el mismo cwd.** Resuelto por diseño: la clave es el id de
   sesión, no la carpeta. Lo que sigue abierto es cuál ventana es cuál (§7.2).
4. **Windows Terminal enfoca la ventana, no la pestaña.** Si el agente está en
   la pestaña 3, el usuario llega a la ventana y tiene que cambiar de pestaña.
   No hay API pública para más. Es una limitación que hay que aceptar o
   compensar con vinculación manual por ventana.
5. **`SetForegroundWindow` falla en silencio.** Ya documentado en el repo. Por
   eso se verifica el resultado y el `unread` solo baja si el foco se confirmó.
6. **Permisos duplicados.** El riesgo no es técnico sino de diseño: la
   tentación de encender `waiting` con una heurística de silencio. El tipo lo
   impide (`source`), y el test lo fija.
7. **La consola PTY embebida es un tercer caso.** Un `claude` corriendo ahí
   escribe el mismo JSONL y su «ventana» es Atic. Resuelto con
   `is_own_app_hwnd` → destino = float de consola.
8. **Costo del poll.** 70 archivos en esta máquina, y crecen. El barrido en dos
   niveles lo acota, pero conviene medirlo con un `~/.claude/projects` grande.
9. **Alcance.** Esto reabre parcialmente una feature que el instalador esconde.
   Si producto no quiere el chip visible todavía, la Fase 1 se entrega con el
   flag en `false` y se prueba con el flag en `true` — no cambia el plan.

---

## 12. Criterio de aceptación del MVP

### Checklist manual (Windows, con `claude` corriendo en Windows Terminal)

- [ ] Con la pill en reposo y `claude` trabajando en otra ventana, aparece el
      chip en modo «trabajando» (pulso) sin que Atic abra nada.
- [ ] Al terminar el turno, el chip pasa a «listo» con la primera línea de la
      respuesta, recortada igual que hoy.
- [ ] La etiqueta **no parpadea** mientras el agente escribe: cambia una vez,
      al cerrar el turno.
- [ ] La pill **no se reencuadra** más de un puñado de veces por turno.
- [ ] Clic en el chip → Windows Terminal al frente, y el contador a cero.
- [ ] Si la terminal se cerró, el clic **no abre el chat de Atic** y el aviso
      no se borra.
- [ ] Dos sesiones de Claude en carpetas distintas → un solo chip, con el
      estado más urgente, y el clic va a **esa**.
- [ ] Una sesión del chat de Atic + una TUI a la vez → un chip; el clic
      respeta la prioridad de §5.4 y no confunde destinos.
- [ ] Con `prefers-reduced-motion` activo: sin animación, mismo estado legible.
- [ ] Con `AGENT_PAGER_ENABLED = false`, nada de esto existe (ni hilo, ni
      evento, ni chip).
- [ ] Un `.jsonl` corrupto o a medio escribir no rompe nada ni ensucia el log.

### Tests automáticos

**Rust** (`cargo test -p atic-desktop --lib`):

- `watch_claude`: fixtures de línea → estado esperado. Incluye `isSidechain`,
  `stop_reason: stop_sequence`, línea ilegible, y `user` sin `promptSource`.
- Tail por offset: escribir un temporal, anexar, verificar que solo se leyó lo
  nuevo; y que una última línea truncada no avanza el offset.
- Coalescer: reloj inyectado; N cambios en 400 ms → 1 emisión; cambio de
  `updated_at` solo → 0 emisiones.
- Regla de honestidad: un `AgentPresence` con `status: Waiting` y
  `source: Jsonl` se normaliza a `Working`.
- `focus`: `trait WindowFocus` con un fake — se prueba la **decisión** (a qué
  se enfoca, si el `unread` baja), no el Win32.

**TypeScript** (`pnpm test`, patrón de `pillPlan.test.ts`):

- `agentChip()`: matriz de las dos fuentes × estados → tono, etiqueta y
  destino. Incluye los dos flags apagados y la prioridad chat vs TUI.
- Recorte compartido: mismo resultado que el `readyLabel` de hoy (test de
  no-regresión, se escribe **antes** de extraer la función).
- Reductor del store de presencia: snapshot → vista, `unread` que sube al
  llegar `ready` sin mirar y baja al enfocar con éxito.

**Validación mínima antes de dar por cerrada cada fase:**
`cargo test -p atic-desktop --lib` · `pnpm check` · `pnpm test` · `pnpm lint`.
Y **no** `cargo check` con `tauri dev` corriendo — comparten `target/` y el
enlazado revienta.

---

## 13. Estimación relativa

| Fase | Tamaño | Riesgo | Nota |
|---|---|---|---|
| 0 — contrato + flag + store | **S** | bajo | Tipos y cableado; nada que pueda fallar en runtime |
| 1a — watcher + chip | **M** | medio | El riesgo es el formato ajeno, no el código |
| 1b — foco de ventana | **M** | **alto** | El §7.1 puede no cerrar; el fallback manual lo salva |
| 2 — hook | **S–M** | medio | S si los hooks son lo que parecen; el transporte es la incógnita |
| 3 — Codex / Cursor / OpenCode | **M–L** | medio | Uno por vez; OpenCode es el distinto (SQLite) |

Camino más corto a algo defendible: **0 + 1a**. Es un semáforo sin foco, y ya
justifica la pill.

---

## 14. Preguntas abiertas

Cuatro. Todo lo demás está decidido arriba con su porqué.

1. **Hooks de Claude Code** — ¿existen `Stop` / `Notification`, pueden invocar
   un binario arbitrario, y el payload trae `session_id` + `cwd`? *Bloquea la
   Fase 2 entera y la atadura exacta pid→sesión.* No se puede resolver desde el
   árbol: hay que ir a docs/CLI.

2. **Granularidad del foco** — ¿alcanza «enfoca la ventana, tú cambias de
   pestaña» para el MVP, o la vinculación manual (§7.2) tiene que entrar desde
   el día uno? *Cambia el tamaño de la Fase 1b.* Mi recomendación: aceptar el
   nivel ventana y meter la vinculación manual solo como fallback.

3. **El interruptor** — ¿confirmás que el pager puede encenderse con la
   consola de agentes todavía oculta (§4.3)? Es una decisión de producto, no
   técnica. Mi recomendación: sí, son features distintas y el pager no expone a
   Atic como interfaz de chat.

4. **Alcance de la Fase 3** — ¿los tres backends, o Codex primero y los otros
   cuando haga falta? Codex tiene el rastro más parecido al de Claude; Cursor y
   OpenCode son investigación de verdad, y OpenCode encima cambia de tecnología
   (SQLite). Mi recomendación: Codex, y los otros dos como plan aparte.
