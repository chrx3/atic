# Plan — Agentes multi-proveedor

Objetivo: Claude Code, Codex, OpenCode y Cursor conversando dentro de Atic, con
permisos, herramientas, historial persistente y reanudación. Sin git,
sin worktrees, sin revisión de diffs — eso queda documentado al final como
"lo que no se hace".

> Estado: **Fases 0 y 1 hechas.** Claude Code y OpenCode funcionan de verdad;
> Cursor comparte el código de OpenCode pero no se pudo probar. Codex y la UI
> quedan pendientes. Si venís a retomar esto, arrancá por
> [Traspaso](#traspaso-para-quien-siga) al final.

## El hallazgo que ordena todo

Los cuatro agentes no hablan cuatro protocolos. Hablan **tres transportes**, y
dos de ellos son el mismo:

| Agente | Cómo se habla | Verificado en esta máquina |
|---|---|---|
| OpenCode | `opencode acp` — **ACP** por stdio | `opencode` 1.15.13, subcomando `acp` presente |
| Cursor | `cursor-agent acp` — **ACP** por stdio | no instalado; mismo adaptador |
| Codex | `codex app-server` — JSON-RPC por stdio | `codex-cli` 0.144.6, marcado `[experimental]` |
| Claude Code | `--input-format stream-json` por stdio | `claude` 2.1.220, **ya implementado** |

Claude Code **no** habla ACP (no hay flag en 2.1.220) y no va a hablarlo: el
adaptador propio de `claude_code.rs` se queda para siempre. Está bien, ya
funciona.

En Rust existe la implementación oficial de Zed:

```toml
agent-client-protocol = "2"
agent-client-protocol-tokio = "0.11"
```

**Un adaptador ACP = OpenCode + Cursor + Gemini + lo que venga.** No son cuatro
integraciones: son dos adaptadores nuevos sobre uno que ya anda.

### Y ACP ya trae el modelo de datos

`SessionNotification.update` es un modelo de **items con identidad**, no un log
plano:

| Variante ACP | Qué es |
|---|---|
| `agent_message_chunk` / `agent_thought_chunk` | texto que se acumula |
| `tool_call` → `tool_call_update` | **un item que muta**, con `toolCallId` |
| `plan` | lista de pasos con estado |
| `available_commands_update` | comandos de barra |
| `current_mode_update` | modo de permisos |
| `usage_update` | contexto consumido |
| `session/request_permission` | permiso, con opciones |

Y un `tool_call` trae:

```
toolCallId  title (legible, lo escribe el agente)  kind  locations
status: pending | in_progress | completed | failed
content  rawInput  rawOutput
```

`kind` es uno de `read | edit | delete | move | search | execute | think |
fetch | switch_mode | other`, y `locations` son los archivos que el tool tocó
("follow-along" en la UI).

**Decisión de diseño: el modelo canónico de Atic se moldea sobre ACP.** No se
inventa uno propio. Consecuencias:

- OpenCode y Cursor entran casi como passthrough.
- Claude Code y Codex traducen **hacia** una forma estándar, no hacia una
  invención local que hay que redefinir cada vez que aparece un agente nuevo.
- `AgentToolCard.svelte` deja de adivinar. Hoy `toolSummary()` en
  `routes/agents/+page.svelte:578` deduce de la entrada cruda lo que ACP
  entrega servido en `title` y `kind`.

## Qué se tira y qué se queda

Lo que hay **no está fatal**. `agents/mod.rs` ya tiene la separación correcta
—driver por backend, evento canónico, UI que solo conoce el canónico— que es
la misma decisión que toma T3 Code con su `ProviderDriver`. El problema está
concentrado en un solo eje.

### Se queda

| Archivo | Por qué |
|---|---|
| `agents/claude_code.rs` | Es el backend más difícil y ya anda. El detalle de `--permission-prompt-tool stdio` (que no aparece en `claude --help`) es conocimiento caro. Se le cambia la salida, no la entrada. |
| `agents/mod.rs` — traits `AgentBackend` / `AgentSession` | La forma es correcta. Se les agrega el ciclo de vida que les falta. |
| `agents/skills.rs` | Ortogonal al resto. |
| `PermissionDecision` | Mapea limpio a las `options` de ACP. |
| El puente de eventos y la adopción de sesiones vivas | La idea de que el proceso lo tiene Rust y cualquier ventana lo adopta es correcta y hay que conservarla. |

### Se rehace

**1. `AgentEvent` es un log plano append-only.** Es *el* problema, y de él salen
todos los demás:

- `ToolCall` y `ToolResult` son dos eventos sueltos, unidos solo por un `id`
  que la UI tiene que emparejar escaneando. Una tarjeta no puede pasar de
  "ejecutando" a "listo": se re-renderiza o se duplica.
- `Delta` no dice a qué pertenece. Por eso en el front hay un
  `streaming: string` **paralelo** al log (`agentSessions.svelte.ts:71`). Se
  rompe apenas dos bloques transmitan a la vez — que es exactamente lo que hace
  Claude cuando piensa y escribe en el mismo turno.
- No hay turnos. `Finished` cierra algo que nunca se abrió.
- No se puede persistir: no hay nada con identidad que guardar.

**2. No hay persistencia.** `bridge.rs:32`:

```rust
static SESSIONS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
```

Cerrás Atic y la conversación se evapora. El CLI sí la guardó en su propio
disco, pero Atic no tiene con qué encontrarla.

**3. `backends()` devuelve una lista fija de uno** (`bridge.rs:35-37`).

**4. `routes/agents/+page.svelte` — 1.562 líneas** mezclando geometría de la
pill, protocolo y render.

## Modelo canónico

En `agents/model.rs` (nuevo). Nombres en la forma de ACP para que la traducción
sea obvia en ambas direcciones.

```rust
/// Un hilo: la conversación completa con un agente, en una carpeta.
pub struct Thread {
    id: ThreadId,
    backend_id: String,
    /// Id de sesión DEL BACKEND, para reanudar. Distinto de `id`.
    provider_session: Option<String>,
    cwd: PathBuf,
    model: String,
    mode: String,
    turns: Vec<Turn>,
}

/// Un ciclo usuario → agente.
pub struct Turn {
    id: TurnId,
    items: Vec<Item>,
    status: TurnStatus,   // running | done | failed | cancelled
    cost_usd: Option<f64>,
    tokens: Option<u64>,
}

/// Todo lo visible tiene id estable y estado mutable.
pub struct Item {
    id: ItemId,
    kind: ItemKind,
}

pub enum ItemKind {
    Message   { role: Role, text: String, streaming: bool },
    Reasoning { text: String, streaming: bool },
    Tool {
        title: String,           // lo escribe el agente
        kind: ToolKind,          // read | edit | execute | search | …
        status: ToolStatus,      // pending | in_progress | completed | failed
        locations: Vec<PathBuf>,
        raw_input: Value,
        output: Vec<ToolContent>,
    },
    Plan       { entries: Vec<PlanEntry> },
    Permission { tool: String, description: String, input: Value,
                 options: Vec<PermissionOption>, status: PermissionStatus },
    Notice     { text: String },
}
```

Y lo que sale hacia la UI deja de ser "el evento que pasó" para ser "qué cambió":

```rust
pub enum AgentDelta {
    TurnStarted  { thread: ThreadId, turn: TurnId },
    ItemAdded    { turn: TurnId, item: Item },
    ItemChunk    { item: ItemId, text: String },      // se ACUMULA en el item
    ItemPatched  { item: ItemId, patch: ItemPatch },  // status, output, locations
    TurnEnded    { turn: TurnId, status: TurnStatus, cost_usd: Option<f64> },
    ThreadPatched{ thread: ThreadId, patch: ThreadPatch }, // model, mode, commands, tokens
    Failed       { thread: ThreadId, message: String },
}
```

La diferencia con hoy, en una línea: **`ItemChunk` sabe a qué item pertenece**, y
por eso el `streaming: string` paralelo del front desaparece.

### Cómo mapea cada backend

| Canónico | Claude (stream-json) | ACP | Codex app-server |
|---|---|---|---|
| `ItemAdded(Message)` | bloque `text` de `assistant` | `agent_message_chunk` (1ro) | `agent_message` |
| `ItemChunk` | `stream_event` parcial | `agent_message_chunk` (resto) | delta |
| `ItemAdded(Reasoning)` | bloque `thinking` | `agent_thought_chunk` | reasoning |
| `ItemAdded(Tool)` | bloque `tool_use` | `tool_call` | `exec_command_begin` etc. |
| `ItemPatched(Tool)` | `tool_result` del turno `user` | `tool_call_update` | `…_end` |
| `ItemAdded(Permission)` | `control_request/can_use_tool` | `session/request_permission` | `applyPatchApproval` etc. |
| `ThreadPatched(commands)` | `Commands` del canal de control | `available_commands_update` | — |
| `ThreadPatched(tokens)` | suma de `usage` (ya está) | `usage_update` | `token_count` |

La columna de Claude ya existe casi entera en `claude_code.rs`: es re-cablear
`translate()`, no reescribirla.

## Fases

### Fase 0 — El modelo y la persistencia · **HECHA**

1. **`agents/model.rs`** — `Thread` / `Turn` / `Item` / `AgentDelta`, con
   `Thread::apply` para reconstruir sin frontend. 6 tests.
2. **`agents/store.rs`** + migración 2 en `crates/core/src/db.rs` — tabla
   `agent_threads`. Los turnos van como **JSON en una columna**, no
   normalizados: un hilo se lee y se escribe entero, y normalizarlo obligaría a
   `atic-core` a conocer un tipo que vive en la app. A columnas sale solo lo que
   se usa para listar. 7 tests.
3. **`claude_code.rs`** — `translate()` pasó de función pura a `Translator` con
   estado. El spawn, los flags, el canal de control y `stop`/`Drop` no se
   tocaron. 18 tests, incluidos los viejos portados.
4. **`bridge.rs`** — comandos `agent_threads` / `agent_thread` /
   `agent_thread_delete`.
5. **Front** — `log: AgentEventPayload[]` → `turns: AgentTurn[]`. Se cayó el
   `streaming: string` paralelo y la derivación de `rows` que emparejaba
   `toolCall` con `toolResult` a mano.

Tres cosas se arreglaron de paso, sin estar en el plan:

- **El mensaje del usuario ahora existe.** No estaba en ningún lado: el registro
  solo tenía lo del backend, así que la conversación se leía como un monólogo y
  al guardarla faltaba la mitad de cada intercambio.
- **El lector de stdout colgaba de `if let Some(stderr)`.** Sin stderr no se
  leía tampoco stdout. Nunca pasó, pero los dos flujos no tenían por qué
  depender uno del otro.
- **`toolSummary()` desapareció.** El backend arma el `title` una vez en vez de
  la vista deducirlo de la entrada cruda en cada render.

**Cuándo se escribe a disco:** en los bordes del turno (fin de turno, fallo,
cierre de sesión), no con cada delta. Los trozos llegan cada pocos milisegundos
y el texto autoritativo llega igual al cerrar el bloque. Lo que esto **no**
cubre: una caída dura de Atic con un turno a medio correr pierde ese turno.

**Pendiente de la fase:** el `static SESSIONS` sigue en `bridge.rs` (mover a
`state.rs` no aportaba nada todavía) y la interfaz aún no ofrece la lista de
hilos guardados — los comandos están, la vista no.

### Fase 1 — ACP · **HECHA**

`agents/acp.rs` atiende a OpenCode y Cursor con el mismo código: cambia una
constante. Probado de punta a punta contra `opencode` instalado, con lectura de
archivo incluida.

**Sin tokio.** El crate va con `futures` + `async-io`, que traen su propio
reactor, así que la conexión corre con `block_on` en un hilo dedicado igual que
el adaptador de Claude. `agent-client-protocol-tokio` NO se usa: está en 0.11.1
y depende del core 0.11.1, no del 2.0.

**Correcciones a lo que decía este plan:**

- `PermissionOptionKind` es un enum **cerrado** (`AllowOnce`, `AllowAlways`,
  `RejectOnce`, `RejectAlways`). Los nombres que muestra el agente son libres,
  la semántica no — así que los tres botones que ya tenía la interfaz mapean sin
  cambios. No hace falta renderizar opciones arbitrarias.
- El spawn en Windows falla por **dos** causas, no una (ver `agents/exe.rs`).

**Lo que el modelo de la Fase 0 aguantó sin tocarse:** los `tool_call` /
`tool_call_update` de ACP son exactamente el item que muta; `usage_update` da
`used` **y** `size`, así que el tamaño de la ventana ya no se adivina con una
constante; y `available_commands_update` trae las skills con descripción.

**Lo que descubrió la prueba real:** OpenCode manda el razonamiento y la
respuesta del mismo turno con el **mismo `messageId`**. El id de item tiene que
ser `tipo:messageId` y no `messageId` solo, o los dos bloques quedan pegados.

**Pendiente de la fase:**

- El camino de permisos no se ejerció en vivo: OpenCode no pidió permiso para
  leer un archivo. El mapeo tiene tests; el ida y vuelta con el agente real, no.
- Reanudar (`options.resume`) no está cableado para ACP, aunque el agente
  declara `sessionCapabilities: {list, resume, close}`.
- Cursor no se pudo probar: no está instalado en esta máquina.

### Fase 2 — Codex

`agents/codex.rs` sobre `codex app-server` (JSON-RPC por stdio). Su propio
`--help` lo marca `[experimental]`: la API va a cambiar, y este adaptador va a
ser el que más mantenimiento pida. Va tercero por eso.

### Fase 3 — UI

Recién acá, cuando ya se sabe qué tienen en común los cuatro. Partir
`routes/agents/+page.svelte`:

- geometría del globo anclado a la pill → su propio módulo (hoy convive con el
  protocolo en el mismo archivo)
- render de la conversación → un componente que recorre `turns[].items[]`
- selector de proveedor / modelo / modo → aparte
- `AgentToolCard.svelte` usa `kind` y `title` del item en vez de adivinar

## Riesgos

1. **Spawn en Windows.** `opencode` y `cursor-agent` se instalan como shims
   `.cmd`; `Command::new("opencode")` falla donde `claude` funciona. T3 Code
   tiene `resolveSpawnCommand` e `isWindowsCommandNotFound` justamente por
   esto. La otra mitad —`CREATE_NO_WINDOW`, `claude_code.rs:111-117`— ya está
   resuelta.
2. **Codex app-server es experimental.** Asumir ruptura en cada actualización.
3. **ACP se mueve.** El crate va por 2.0. Pero es *una* superficie que seguir en
   vez de tres.
4. **Claude nunca va a hablar ACP.** El adaptador propio es permanente.
5. **Alcance del producto.** El README dice "asistente que graba llamadas".
   Esto lo convierte en otra cosa. Encaja porque la pill ya es multi-herramienta
   (dictado, portapapeles, snippets, capturas, OCR) — pero el README y el
   `PLAN.md` van a necesitar decir qué es Atic ahora.

## Lo que NO se hace

Decidido explícitamente, para que no se filtre por los bordes:

- **Checkpoints de git por turno** y revisión de diffs antes de aceptar.
- **Worktrees paralelos** con varios agentes sobre el mismo repo.
- **Acceso remoto** (agente corriendo en otra máquina). Toda la arquitectura de
  servidor + WebSocket + relay de T3 Code existe para esto; Atic es local.
- **Editor de código propio.** Atic no compite con VS Code; convive.

El modelo de la Fase 0 tiene turnos con identidad, así que los checkpoints
entran después sin rehacerlo — pero no se construye nada especulativo para
sostenerlos.

## Lo que Atic tiene y T3 Code no

Es lo que justifica hacerlo acá en vez de usar t3code y listo: **dictado, OCR,
captura de pantalla y una pill flotante**. Hablarle al agente y pasarle una
captura sin salir de lo que estás haciendo no se copia de ningún lado porque no
existe en ningún lado. Los puentes entre las herramientas existentes y los
agentes son tan prioritarios como los proveedores mismos.

---

# Traspaso para quien siga

Escrito al terminar las Fases 0 y 1, para poder retomar en otra máquina sin
reconstruir el contexto leyendo diffs.

## Qué anda hoy, comprobado

| Cosa | Estado |
|---|---|
| Modelo canónico (`agents/model.rs`) | Anda. 6 tests. |
| Claude Code (`agents/claude_code.rs`) | Anda. 18 tests. Era lo que ya existía, re-cableado. |
| OpenCode vía ACP (`agents/acp.rs`) | **Anda de verdad**, probado contra el CLI instalado, con herramienta incluida. |
| Cursor vía ACP | Mismo código que OpenCode. **Sin probar** — no estaba instalado. |
| Persistencia (`agents/store.rs` + migración 2) | Anda. 7 tests. Escribe en los bordes del turno. |
| Resolución del ejecutable (`agents/exe.rs`) | Anda. 10 tests. |
| Codex | **No empezado.** |

Validación completa en verde: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace`, y `pnpm check` del
frontend.

## Antes de compilar en una máquina nueva (Windows)

Esto costó media hora de perseguir un error que no era del proyecto, así que
queda escrito. `whisper-rs` compila whisper.cpp con bindgen, y en Windows
necesita **tres** cosas en el entorno, no una:

```powershell
# 1. libclang, que es lo que el README ya pedía
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

# 2. el entorno de MSVC (cabeceras y linker)
cmd /c '"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && set' |
  ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { Set-Item "env:$($matches[1])" $matches[2] -ErrorAction SilentlyContinue } }

# 3. el puente que falta: libclang NO lee INCLUDE, lee CPATH
$env:CPATH = $env:INCLUDE
```

Sin el paso 3 el error es `fatal error: 'stdio.h' file not found` seguido de
`attempt to compute 12_usize - 16_usize` en unos bindings de Linux — que no
tiene nada que ver con la causa y manda a buscar al lado equivocado.

## Lo que sigue, en orden

1. **Vista de hilos guardados.** Los comandos existen y están probados
   (`agent_threads`, `agent_thread`, `agent_thread_delete`), pero **ninguna
   pantalla los usa**. Hoy la persistencia funciona y no se puede ver: hay que
   mirar el `atic.db3` para comprobarla. Es lo más barato con más valor visible.
2. **Codex** (`codex app-server`, JSON-RPC por stdio). Va tercero a propósito:
   su propio `--help` lo marca `[experimental]` y va a ser el que más
   mantenimiento pida.
3. **Cerrar lo suelto de ACP** — ver la lista de la Fase 1.
4. **UI** (Fase 3) y **puentes de Atic** (Fase 5, Atic como servidor MCP).

## Trampas que ya pagamos, para no repetirlas

- **`Command::new("opencode")` falla y `Command::new("claude")` no.** No es el
  PATH: `claude` es un `.exe` y `opencode` un shim de npm. Y npm deja **tres**
  archivos juntos —`opencode` sin extensión, `.cmd` y `.ps1`—, de los cuales en
  Windows solo el `.cmd` sirve. Todo eso vive en `agents/exe.rs` con tests.
- **`async-process` no sabe lanzar un `.cmd`.** La std de Rust lo disimula desde
  1.77, pero el crate de ACP usa `async-process`, que no. Van por `cmd /C` —
  `exe::launcher`.
- **`AcpAgent::from_str` se come las `\` de Windows.** Parte con `shell-words`,
  que usa reglas POSIX. Hay que usar `AcpAgentConfig::new(ruta).arg(..)`.
- **OpenCode repite el `messageId`** entre el razonamiento y la respuesta del
  mismo turno. El id de item es `tipo:messageId`, nunca `messageId` solo.
- **`raw_output` puede traer el archivo entero.** Un `read` de un README de 200
  líneas devolvió ~30 KB. Hay tope de 8 KB en `acp.rs`.
- **`agent-client-protocol-tokio` está desactualizado** (0.11.1, contra el core
  2.0.0). No se usa. El core trae su propio transporte y no necesita tokio.

## Cómo probar sin la interfaz

Hay un ejemplo que corre el adaptador contra un agente instalado e imprime cada
`AgentDelta` como lo recibiría la vista. No es un test —lanza un proceso, tarda
y gasta tokens— y por eso vive como ejemplo:

```bash
cargo run -p atic-desktop --example acp_real -- opencode "lee README.md y di de que trata"
cargo run -p atic-desktop --example acp_real -- cursor "hola"
```

## Decisiones que conviene no revisar sin leer el porqué

Cada una tiene el motivo escrito en el archivo donde vive. Las que más caro
saldría deshacer sin entenderlas:

- **El modelo canónico tiene la forma de ACP** y no una propia
  (`model.rs`, cabecera). Es lo que hizo que el adaptador de OpenCode fuera
  copiar campos.
- **Los turnos se guardan como JSON en una columna**, no normalizados
  (`crates/core/src/db.rs`, sobre `MIGRATION_2`).
- **El turno lo abre quien escribe**, no quien lee (`agents/turns.rs`).
- **Las sugerencias de permiso de Claude no viajan al frontend**
  (`claude_code.rs`, en `Translator`).
- **El editor de MCP es JSON crudo a propósito** (`McpServersModal.svelte`) —
  un formulario con campos fijos se queda corto con cada variante nueva.
