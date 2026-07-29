# Plan — Agentes multi-proveedor

Objetivo: Claude Code, Codex, OpenCode y Cursor conversando dentro de Atic, con
permisos, herramientas, historial persistente y reanudación. Sin git,
sin worktrees, sin revisión de diffs — eso queda documentado al final como
"lo que no se hace".

> Estado: **modelo canónico + 4 backends + UI + persistencia andan.** Claude
> Code, OpenCode, Codex y Cursor están cableados. Resume expuesto para Claude y
> Codex. Subagentes nativos se ven como items `collab`. Worktrees/checkpoints
> siguen fuera de alcance. Roadmap de adopción Synara/T3: composer UX, resume,
> collab visible, approvals/listados ligeros. Si venís a retomar, arrancá por
> [Traspaso](#traspaso-para-quien-siga).

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
`state.rs` no aportaba nada todavía).

**Cerrado después:** la vista del historial. Una pastilla «Historial» en el
compositor abre la lista dentro de la burbuja, y un hilo guardado se dibuja con
el **mismo** código que uno vivo — tienen la misma forma, así que la vista no
distingue: lo único que cambia es que el guardado ya no crece. Borrar pide
confirmación en el propio botón. No hay «Reanudar»: `options.resume` solo lo
honra `claude_code.rs`, y en OpenCode el botón arrancaría una sesión nueva
fingiendo que sigue la vieja.

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
`used` **y** `size`; y `available_commands_update` trae las skills con
descripción.

**Corregido en la Fase 2:** ese `size` llegaba al `ThreadPatch` de Rust y **el
TypeScript lo tiraba** — no existía el campo. La vista siguió dibujando el
anillo contra una constante de un millón hasta que Codex, con su ventana de
258K, dejó el error a la vista: mostraba un cuarto de lo consumido de verdad.

**Lo que descubrió la prueba real:** OpenCode manda el razonamiento y la
respuesta del mismo turno con el **mismo `messageId`**. El id de item tiene que
ser `tipo:messageId` y no `messageId` solo, o los dos bloques quedan pegados.

**Pendiente de la fase:**

- El camino de permisos no se ejerció en vivo: OpenCode no pidió permiso para
  leer un archivo. El mapeo tiene tests; el ida y vuelta con el agente real, no.
- Reanudar (`options.resume`) no está cableado para ACP, aunque el agente
  declara `sessionCapabilities: {list, resume, close}`.
- Cursor no se pudo probar: no está instalado en esta máquina.

### Fase 2 — Codex · **HECHA**

`agents/codex.rs` sobre `codex app-server`: JSON-RPC 2.0 por stdio, **una línea
por mensaje** (JSONL, sin cabeceras `Content-Length`). Probado contra
`codex-cli 0.145.0`.

**El esquema lo genera el propio CLI.** Es la fuente autoritativa y evita
adivinar:

```bash
codex app-server generate-json-schema --out <dir>   # 234 archivos, v1 y v2
codex app-server generate-ts --out <dir>            # lo mismo en TypeScript
```

**Corrección a lo que decía este plan:** Codex **no** manda pares
`exec_command_begin` / `…_end`. Eso es el protocolo viejo. El v2 es
`item/started` → `item/completed` sobre un **item con id estable**, que es
exactamente la forma de `model.rs`. La traducción terminó pareciéndose a la de
ACP y no a la de Claude Code — el modelo canónico aguantó un tercer transporte
sin tocarse.

**Lo que sí es más grande de lo esperado:** 89 peticiones y 70 notificaciones.
Se traduce el subconjunto que el modelo sabe mostrar y **el resto se ignora en
silencio**, al revés que en `claude_code.rs`, donde lo desconocido sale como
`Notice`. Allá el vocabulario es chico y una línea nueva vale la pena verla; acá
la mayoría habla de cuentas, plugins, watchers de disco y sesiones de voz, y
mostrarlas enterraría la conversación.

**El handshake es lento, y eso mandó el diseño.** `thread/start` tardó **8
segundos** en esta máquina: antes de contestar levanta todos los servidores MCP
que el usuario tenga configurados. Hacerlo dentro de `start()` congelaría la
interfaz mientras se abre la burbuja, así que corre en el hilo lector y
`send` **encola** lo que se escriba mientras tanto. Para quien escribe, la
sesión está lista desde el primer momento.

**Lo que Codex no da:** costo en dólares. `cost_usd` va en `None`. A cambio da
el tamaño real de la ventana de contexto (`modelContextWindow`), que es mejor
que la constante escrita a mano que había.

**Lo que mapeó uno a uno:** las tres decisiones de permiso
(`accept` / `acceptForSession` / `decline`) contra los tres botones que ya
tenía la interfaz. Existe una cuarta, `cancel` —denegar **y** cortar el turno—
que no se ofrece: es otra decisión, y la que hay significa «esto no, seguí».

**Trampas de esta fase:**

- El id del item de permiso **no puede ser** el del item que lo motiva
  (`perm:{itemId}`): son dos cosas distintas en la conversación. Es la misma
  lección que dejó OpenCode repitiendo el `messageId`.
- La salida de un comando llega por trozos, pero `ItemChunk` solo acumula en
  texto y razonamiento —es lo que el modelo define—, así que se junta en el
  adaptador y se entrega entera al cerrar el item.
- Codex escribe trazas por stderr, incluidos errores de OAuth de servidores MCP
  ajenos. Solo se muestran las líneas que dicen `ERROR`, o el registro se llena
  de ruido que no es de la conversación.

### Fase 3 — UI · **HECHA en código, PENDIENTE de mirar**

Recién acá, cuando ya se sabe qué tienen en común los cuatro. Lo que salió de
`routes/agents/+page.svelte`:

| Salió a | Qué se llevó |
|---|---|
| `lib/AgentConversation.svelte` | El render de `turns[].items[]`, con sus estilos. |
| `lib/bubble.svelte.ts` | La geometría del globo: ancla, vuelo, desanclado, y la conversión de píxeles físicos a lógicos. |
| `lib/agentModels.ts` | Los modelos, ahora **por backend**. |
| `AgentToolCard.svelte` | Ya usaba `kind` y `title` desde la Fase 0. |

**Que la conversación sea un componente no es solo orden:** dibujarla dejó de
depender de estar dentro de la burbuja, y por eso el historial de hilos
guardados la reusa tal cual. Un hilo guardado y uno vivo tienen la misma forma,
así que el componente no los distingue.

**Un bug que apareció al sacar los modelos:** la lista era la de Claude
—`opus`, `sonnet`, `haiku`— para los cuatro agentes. Elegir «Opus 5» en una
sesión de Codex le pasaba un modelo que no existe. Los de Codex ahora salen de
su propio `model/list`. Los de OpenCode y Cursor **no se inventan**: enrutan a
varios proveedores según lo que tengas configurado allá, así que se ofrece solo
«el de tu CLI».

> **Ojo:** esto pasó `pnpm check` sin errores ni avisos, pero **nadie lo vio
> corriendo**. Un `svelte-check` limpio no dice nada del vuelo de la burbuja ni
> de si un estilo se quedó sin mudar. Es lo primero que hay que mirar en el QA.

**Cómo tiene que verse ya está decidido**, en una maqueta que no es un dibujo
sino el modelo de la Fase 0 corriendo:

> https://claude.ai/code/artifact/cfc27024-e414-47c5-b80b-2f96b6ee60af

La burbuja está a tamaño real (580×520, lo que manda Rust) y se mueve con un
guion de `AgentDelta` escrito a mano — misma máquina de estados, sin CLI detrás.
Se puede cambiar de agente, redimensionar, y recorrer tres escenas: sesión
entera, permiso y dictado + captura. Lo que deja resuelto:

- la tarjeta de herramienta con el ícono desde `kind` y el texto desde `title`,
  y el estado en la **forma** (el punto late mientras corre), no sólo en color
- el permiso pegado al compositor, porque es lo único que detiene el turno
- el compositor en dos grupos: los ajustes se comprimen y bajan de línea, las
  acciones nunca — a 580px el botón de enviar se salía del panel
- el acento como única pieza que cambia entre los cuatro agentes, que es la
  prueba visible de que el modelo canónico sirvió

Los tokens de color de la maqueta son los mismos de `+page.svelte`, así que se
puede leer como referencia de implementación y no sólo de intención.

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
| Modelo canónico (`agents/model.rs`) | Anda. Incluye `ItemKind::Collab` para subagentes nativos. |
| Claude Code (`agents/claude_code.rs`) | Anda. `Task`/`Agent` → Collab. Resume con `--resume`. |
| OpenCode vía ACP (`agents/acp.rs`) | Anda. Resume/MCP/modelo ACP aún parcial (no fingir en UI). |
| Cursor vía ACP | Mismo adaptador ACP. Probar con `agente_real -- cursor`. |
| Persistencia (`agents/store.rs` + migraciones 2–3) | Anda. Columna `preview` para listados sin deserializar turnos. |
| Resolución del ejecutable (`agents/exe.rs`) | Anda. |
| Codex (`agents/codex.rs`) | Traducción + collab + rechazo seguro de requests desconocidas. Resume vía `thread/resume`. |
| UI consola (`routes/agents/+page.svelte`) | Empty state, composer con estados, menú `+`, Continuar (Claude/Codex), riesgo en Acceso total. |
| Historial | Lista ligera + lectura + Continuar cuando hay `providerSession`. |

Validar con: `cargo test -p atic-core`, `cargo test -p atic-desktop --lib`,
`pnpm check` en desktop, y `agente_real` contra cada CLI.

## Antes de compilar en una máquina nueva (Windows)

Esto costó media hora de perseguir un error que no era del proyecto, así que
queda escrito. `whisper-rs` compila whisper.cpp con bindgen, y en Windows
necesita **cuatro** cosas en el entorno, no una:

```powershell
# 1. el entorno de MSVC (cabeceras y linker)
$vc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
& cmd.exe /c "`"$vc`" > nul & set" |
  ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2]) } }

# 2. libclang, que es lo que el README ya pedía
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"

# 3+4. el puente que falta: libclang NO lee INCLUDE, lee CPATH — y desde LLVM 19
#      tampoco encuentra sus PROPIAS cabeceras si no se las nombra.
$env:CPATH = "C:\Program Files\LLVM\lib\clang\22\include;" + $env:INCLUDE
```

Sin el paso 3 el error es `fatal error: 'stdio.h' file not found` seguido de
`attempt to compute 12_usize - 16_usize` en unos bindings de Linux — que no
tiene nada que ver con la causa y manda a buscar al lado equivocado. Sin el 4
es el mismo teatro con `'stdbool.h'`, que confunde todavía más porque ese
header lo trae el compilador y uno lo busca en el SDK.

Y si ya hubo un intento fallido, el entorno arreglado no alcanza: cargo guarda
los bindings malos y los reusa, así que el error vuelve igual. Hay que borrar
`target\debug\build\whisper-rs-sys-*` para que el build script corra de nuevo.

La versión de `Set-Item` que estaba acá antes falla en PowerShell 5.1 no
interactivo («too many arguments»); `[Environment]::SetEnvironmentVariable`
anda en los dos.

## Lo que sigue, en orden

1. **QA visual con la app abierta** (globo, conversación, historial, Continuar,
   tarjeta Collab, permiso naranja en Acceso total).
2. **Ejercer Codex/Cursor de punta a punta** con `agente_real` y permisos reales.
3. **ACP resume** cuando OpenCode/Cursor lo expongan de forma fiable; hasta
   entonces no ofrecer Continuar para esos backends.
4. **Interrupt de turno** sin matar la sesión (hoy el CTA en streaming solo
   bloquea envíos; no hay cancel suave).
5. **Puentes de Atic** (dictado, OCR, captura como tools MCP del harness).
6. **Fuera todavía**: worktrees, checkpoints, diff review, orquestador propio
   tipo Synara `create_threads`, committee/advisor.

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

Hay un ejemplo que corre **cualquiera** de los cuatro adaptadores contra el CLI
instalado e imprime cada `AgentDelta` como lo recibiría la vista. No es un test
—lanza un proceso, tarda y gasta tokens— y por eso vive como ejemplo:

```bash
cargo run -p atic-desktop --example agente_real -- codex    "responde solo: hola"
cargo run -p atic-desktop --example agente_real -- opencode "lee README.md y di de que trata"
cargo run -p atic-desktop --example agente_real -- cursor   "hola"
cargo run -p atic-desktop --example agente_real -- claude   "hola"
```

Que cubra los cuatro es el punto: los cuatro emiten los **mismos** deltas, así
que la salida tiene que leerse igual para todos. Si para uno se lee distinto, la
traducción está mal. Los permisos se conceden solos y se avisa por pantalla, o
un agente que pregunta dejaría el ejemplo colgado esperando a nadie.

(Antes era `acp_real` y solo sabía de ACP. Se unificó al sumar Codex: la
alternativa era duplicar el impresor de deltas en dos archivos.)

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
