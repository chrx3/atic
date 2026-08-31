# Plan — Orquestación de agentes vía MCP

Objetivo: que **cualquier agente** (Claude Code, Codex, Cursor Agent, OpenCode,
y también Cursor IDE / Codex CLI en su app original) pueda **listar, elegir,
levantar y encargarle un turno a cualquier otro**, con Atic como directorio
central. Sin un protocolo N×N. Sin A2A. Sin committee/advisor.

> Estado: **plan cerrado, listo para fase 0.** Revisión Fable 5 (2026-08-30):
> las afirmaciones contra el código se sostienen; las diez preguntas están
> cerradas más abajo. No hay código de hub ni de servidor MCP todavía. El
> harness de los cuatro backends **sí** existe y es lo que este plan reusa.
> Si vienes a retomar, arranca por [Traspaso](#traspaso-para-quien-siga).

Relacionado: [`PLAN_AGENTES.md`](PLAN_AGENTES.md) (harness, modelo canónico,
punto 6: orquestador). Este plan entra un orquestador **chico**: no es Synara
`create_threads`, es un **directorio + mensajero de un turno**. Los puentes de
Atic (dictado, OCR, captura como tools MCP) siguen siendo otra feature
([PLAN_AGENTES](PLAN_AGENTES.md) § lo que sigue, punto 5) y **no** se mezclan
acá: el día que existan, son tools nuevas en el **mismo** servidor `atic`.

La línea divisoria con Synara: **quién es dueño del loop**. En Synara el
orquestador crea hilos, rutea y reintenta solo. Acá el modelo padre decide y
Atic es directorio + mensajero, con límites duros. No hay reintento automático
ni fan-out a varios hijos en una llamada. `atic_spawn` / `atic_prompt` /
`atic_delegate` cuentan contra las **mismas** reglas de profundidad y
presupuesto.

---

## El hallazgo que ordena todo

Los cuatro CLIs no se hablan entre sí. Ninguno es cliente ACP de los otros.
Todos sí son **hosts MCP**: pueden cargar un servidor de herramientas.

Por eso la malla no es una malla. Es una **estrella**:

```text
Claude Code     Codex CLI      Cursor IDE / cursor-agent     OpenCode
     \               \                    |                     /
      \               \                   |                    /
                   MCP stdio  (el mismo servidor: `atic-mcp`)
                              |
                         hub localhost
                              |
                            Atic
              (bridge.rs: agent_start / agent_send / …)
                    /         |          \
              Claude       Codex      Cursor / OpenCode
```

- **Hacia el padre** (quien está hablando contigo): MCP. Es la única puerta que
  Claude, Codex, Cursor y OpenCode ya abren para extensiones.
- **Hacia el hijo** (el agente al que se le encarga el trabajo): el protocolo
  nativo que Atic ya traduce — stream-json, `codex app-server`, ACP.
- **Entre `atic-mcp` y Atic**: un bus local (no MCP). El proceso MCP es hijo
  del host (Cursor.exe, `codex`, `claude`). No es la ventana de Atic. Sin bus,
  no hay directorio compartido ni UI.

MCP **no** es el protocolo entre Claude y Codex. Es el adaptador de un solo
lado. Intentar MCP de punta a punta (Codex como servidor MCP que Claude llama
directo) pelea con la forma de esos CLIs y duplica lo que `bridge.rs` ya hace.

### Por qué no A2A ni ACP en el borde

| Protocolo | Encaje | Por qué no es la puerta |
|---|---|---|
| MCP | Lo que los hosts ya cargan | **Sí es la puerta** |
| ACP | Perfecto *hacia abajo* (Cursor, OpenCode) | Claude Code no es cliente ACP |
| A2A (Google) | Diseñado para pares | Ningún CLI de esta lista lo habla; sería un protocolo huérfano |
| HTTP propio de Atic | El bus interno | Cursor/Codex originales no le pegan a menos que alguien los envuelva — ese alguien es MCP |

---

## Qué ve el usuario

Tres entradas, el mismo directorio:

1. **En Atic.** Consola de agentes como hoy. Además, un agente vivo puede
   llamar tools `atic_*` y levantar a otro; el hijo aparece como hilo colgando
   del padre.
2. **En Cursor IDE o Codex CLI originales.** El usuario pega el servidor MCP
   de Atic en *esa* app. Escribe «pásale esto a Claude» o «elige quién hace el
   review». El modelo llama `atic_list_agents` / `atic_delegate`. Si Atic está
   en la bandeja, el hijo se ve en Atic. Si Atic no está, el MCP responde con
   un error claro (v1: el hub es obligatorio).
3. **En Claude Code de terminal**, igual que 2, con el MCP en su config o el
   que Atic ya sabe pasar con `--mcp-config`.

No se fusionan las UIs. Cursor IDE sigue siendo Cursor IDE. Atic muestra los
hilos que **ella** spawneó o adoptó. El padre ve el resultado como output de
una tool.

---

## Qué hay hoy (para no redescubrirlo)

Verificado en la revisión contra el código (2026-08-30):

| Pieza | Dónde | Sirve para |
|---|---|---|
| Cuatro backends, mismo modelo | `agents/{claude_code,codex,acp}.rs` | Spawn + prompt del hijo |
| Ids estables | `claude-code`, `codex`, `opencode`, `cursor` | Contrato MCP; no inventar alias |
| Lista de backends + `available` | `bridge.rs` `agent_backends()` | `atic_list_agents` — **no es gratis** (`is_available` lanza un proceso por backend; el comentario en `bridge.rs` dice no llamarlo en cada render) |
| Sesiones vivas | `SESSIONS` en `bridge.rs` | `atic_list_sessions` / `prompt` |
| `agent_start` / `agent_send` / `interrupt` / `stop` / `permission` | `bridge.rs` | El hub es una fachada de esto |
| Deltas por callback → evento Tauri | `bridge.rs` `on_delta` | El hub necesita **además** un primitivo de espera a `TurnEnded` (canal o condvar). Hoy no existe |
| `StartOptions.mcp_config` | `mod.rs` | Inyectar *este* MCP en el hijo |
| `--mcp-config` **solo en Claude** | `claude_code.rs` | Codex y ACP reciben `mcp_config` y lo **ignoran** |
| Editor JSON de MCP para el agente | `McpServersModal.svelte` | Config de *otros* servidores, no de este |
| Shims `.cmd` de npm | `agents/exe.rs` | Lanzar `atic-mcp` y los CLIs en Windows |
| Atic es un solo proceso | `tauri-plugin-single-instance` | El hub vive ahí; no hay segundo Atic |
| Snippet para pegar, no escribir configs ajenas | `agents/ping.rs` | Mismo trato para instalar el MCP en Cursor/Codex; stdin no se reenvía |
| Codex tarda ~8 s al abrir **porque levanta sus MCP** | `codex.rs` cabecera | Inyectar este MCP en cada Codex **encarece el handshake** |
| Data dir | `AppDirs` → `%APPDATA%\ciat\atic\data` | `hub.json` |
| Ejemplo punta a punta | `cargo run -p atic-desktop --example agente_real` | Cómo probar un backend sin UI |

---

## Perímetro

### En v1

- Servidor MCP stdio (`atic-mcp`) con las tools de [Contrato MCP](#contrato-mcp).
  Un solo servidor `atic`; el sidecar se diseña para **sumar grupos de tools**
  después (capturas/dictado = tools nuevas, no un segundo proceso).
- Hub en el proceso de Atic, localhost + token, **solo si Atic está corriendo**.
  No es una limitación: sin Atic no hay dónde responder permisos, y el default
  de permisos es preguntar en Atic. Un spawn sin UI obligaría a auto-aprobar.
- Los cuatro backends que Atic ya conoce como **hijos**.
- Padre: cualquiera que cargue el MCP (Atic, Claude Code, Cursor IDE, Codex CLI,
  OpenCode).
- Grafo: profundidad máxima 2, anti-ciclo, rechazo (no reuso) si ya hay hilo
  vivo del mismo backend+cwd, un turno por recado, timeout 5 min.
- Hijos visibles en la consola de Atic (padre, backend, estado).
- Instalación del MCP en apps originales: **snippet para pegar** (como los
  hooks de `ping.rs`). Botón opcional «copiar config» por host. No pisar
  `~/.codex/config.toml` ni el MCP de Cursor en silencio.
- Windows primero. macOS si el hub es TCP localhost (no hay API Win32).

### Fuera de v1

- Hub embebido cuando Atic está cerrada (spawn sin UI). Choca con el modelo de
  permisos.
- Extraer `agents/` a un crate compartido.
- A2A, ACP como puerta del padre, committee, advisor, worktrees, reintento
  automático, fan-out a varios hijos en una llamada.
- Dictado / captura / OCR como tools MCP (punto 5 de PLAN_AGENTES; mismo
  servidor `atic` el día que existan).
- Inyectar en ChatGPT web, Cursor Cloud, o cualquier host sin MCP local.
- SSH remoto como padre o como hijo vía este MCP.
- Streaming token a token del hijo hacia el padre (v1: progreso + resultado al
  cerrar el turno, o timeout-como-traspaso).
- Elegir un chat **ajeno** que no pasó por Atic (TUI de Codex ya abierta,
  Composer de Cursor). El MCP spawnea **sesiones Atic**, o habla con las que
  Atic ya tiene.
- Picker humano cuando el padre dice `"auto"`.
- Inyectar este MCP en hijos Codex / Cursor / OpenCode.

---

## Arquitectura

### Tres procesos, no uno

```text
[Cursor.exe | claude | codex | Atic UI]
        stdio MCP (JSON-RPC, Content-Length)
              │
         atic-mcp.exe          ← binario chico, sin ventana
              │  HTTP JSON en 127.0.0.1:<puerto>
              │  header Authorization: Bearer <token>
              ▼
         Atic (tray)           ← hub + harness + UI
              │
         hijo: claude | codex | cursor-agent | opencode
```

`atic-mcp` no implementa Claude ni ACP. Es un **proxy**: traduce `tools/call`
a pedidos del hub. El harness no se mueve de `atic-desktop`. El binario se
organiza para registrar **grupos** de tools (`atic_list_agents`, … hoy;
capturas/dictado después) sin cambiar de servidor ni de snippet.

**Por qué no es un subcomando de `atic.exe`:** el desktop es Tauri, single-
instance, y si Cursor lanza `Atic.exe --mcp` o bien se come la instancia que
ya corre (y no reenvía stdin: ya lo documentó `ping.rs`) o bien abre otra
ventana. Un binario sidecar evita las dos trampas.

**Por qué el hub no es MCP:** MCP es stdio uno-a-uno con *un* host. El
directorio tiene que atender a Cursor y a Claude a la vez. Eso es un servidor
de muchos clientes. MCP no es ese servidor; el hub sí.

**Por qué el hub es obligatorio:** el default de `permissionMode` es preguntar
en Atic. Sin ventana no hay consentimiento informado para que el hijo edite
archivos. El copy del error cuando Atic está cerrado tiene que decir eso, no
un stack ni «connection refused».

### Transporte del hub

- Escuchar `127.0.0.1` en puerto efímero (no `0.0.0.0`).
- Al arrancar Atic, escribir `%APPDATA%\ciat\atic\data\hub.json` (via
  `AppDirs`) con `{ "port", "token", "pid", "version" }`.
- ACL del archivo: solo el usuario. Token aleatorio por sesión de Atic.
- Al salir Atic, borrar el archivo (best-effort) y dejar de aceptar.
- `atic-mcp` lee `hub.json`; si no hay archivo o el pid murió → error claro,
  en tuteo, p. ej.: «Atic no está abierto. Ábrelo desde la bandeja para
  delegar: sin Atic no hay quién apruebe los permisos del otro agente.»
- Un request = un JSON. Sin websocket en v1. `atic_delegate` y `atic_prompt`
  (`wait: true`) **bloquean** con notificaciones de progreso MCP y timeout
  5 min. No hay job + poll: el modelo a veces no vuelve a pollar.

No named pipes en v1: TCP localhost es el mismo código en Windows y macOS.
El token evita que otra app en la máquina hable con el hub solo por conocer el
puerto.

### Espera a fin de turno (nombrar en fase 0–1)

Hoy `agent_send` dispara y los deltas salen por el callback hacia el evento
Tauri. El hub, para bloquear hasta `TurnEnded`, necesita un primitivo que
**aún no existe**: canal o condvar por sesión, alimentado desde el mismo
`on_delta` (o un wrapper). Sin eso, `wait: true` no se puede implementar
desde Rust. Va en fase 0 (la interfaz) y se cablea en fase 1. No es un
detalle de implementación que «aparezca después».

### Cache de `available`

`agent_backends()` llama `is_available` por backend, y eso **lanza un
proceso**. El hub no lo llama en cada `atic_list_agents`. Cache con TTL
(p. ej. 60 s) o refresco al arrancar el hub y cuando falle un `spawn` por
«no instalado». La UI de Atic ya tenía esta advertencia; el MCP la hereda.

### Inyección del MCP en cada padre

El servidor que hay que anunciar es siempre el mismo:

```json
{
  "mcpServers": {
    "atic": {
      "command": "C:\\ruta\\a\\atic-mcp.exe",
      "args": []
    }
  }
}
```

La ruta sale del instalador (junto a Atic). En dev: ruta absoluta del
`atic-mcp` compilado, no un `atic-mcp` suelto en el PATH.

Cómo llega a cada padre:

| Padre | v1 |
|---|---|
| Claude Code **arrancado por Atic** | Merge del servidor `atic` en `--mcp-config` (el flag ya existe). Suma, no reemplaza, lo de `McpServersModal`. |
| Codex / Cursor / OpenCode **arrancados por Atic** | **No** heredan este MCP. `ATIC_DELEGATE_DEPTH` y el id del padre **sí** viajan en el env del proceso hijo, aunque el MCP no se inyecte. Así, si ese proceso más tarde carga el MCP por config global del usuario, el grafo no miente. |
| Cursor IDE, Codex CLI, Claude de terminal | Snippet en Ajustes → Agentes → «Usar Atic desde otras apps». El usuario pega. |
| Hijo Claude spawneado por `atic_spawn` / `atic_delegate` | Recibe el MCP (es Claude). Cadena típica: Cursor IDE → Claude (plan) → Codex (parche). El segundo salto no lleva MCP en Codex; el tercero no existe. |

**`mcp_config` en Codex y ACP:** hoy se traga y se ignora. v1 lo deja
explícito: comentario en el adaptador + `tracing::warn` la primera vez, y no
se documenta como si funcionara. **No** se rechaza `agent_start` si viene
`mcp_config` (el campo es compartido; Claude lo necesita). No implementar
inyección nativa en Codex/ACP en v1.

**No escribir** `~/.claude.json`, `~/.codex/config.toml` ni `.cursor/mcp.json`
sin una acción explícita. El precedente es `ping.rs`.

### El grafo

Cada pedido al hub lleva (o el hub infiere):

- `parent`: id de sesión Atic del padre, o `external:<host>:<pid>` si el padre
  es Cursor IDE / Codex CLI (no hay sesión Atic).
- `depth`: entero. `atic-mcp` lo lee de `ATIC_DELEGATE_DEPTH` (default 0) y
  manda `depth+1` al spawn del hijo. El hijo **siempre** hereda
  `ATIC_DELEGATE_DEPTH=n` en el env, inyecte o no el MCP.
- `root`: id del encargo original, para detectar ciclos.

Reglas v1 (constantes, no UI):

| Regla | Valor | Por qué |
|---|---|---|
| Profundidad máxima | 2 | Con hijos no-Claude sin MCP, la cadena de dos saltos pasa por un hijo Claude: Cursor IDE → Claude planifica → Codex parchea. Profundidad 1 mata ese patrón. |
| Mismo backend + mismo cwd + hilo vivo | **Rechazar.** Mensaje que sugiere `atic_prompt` a esa sesión | Reusar mezcla contextos de dos encargos. Clonar en silencio deja dos Codex sobre los mismos archivos |
| Ciclo A→B→A en el mismo `root` | Rechazar | El padre ya tiene el contexto |
| Presupuesto | 1 turno hijo por `atic_delegate` / `atic_prompt`; timeout 5 min. `atic_spawn` cuenta para profundidad, no abre turno | Las tres tools comparten el grafo. No hay reintento ni fan-out |
| Permisos del hijo | `permissionMode` del padre; default `default` (preguntar en Atic) | Aprobar `atic_delegate` en Cursor no es consentimiento para que el hijo edite. El padre puede pasar `acceptEdits` si el usuario lo pide |

Si el hijo pide permiso, el turno del padre está **congelado** en el
`tools/call`. Atic muestra el permiso del hijo; el MCP manda progreso
«esperando permiso en Atic». Al timeout (de turno o de permiso) **no se mata
la sesión**: se interrumpe la espera, se devuelve transcript parcial + id de
sesión + hint de seguir con `atic_prompt` / `atic_list_sessions`. Ver
[timeout como traspaso](#timeout-como-traspaso).

Dos capas de permiso: en Cursor, «¿puedo llamar `atic_delegate`?»; en Atic, el
del hijo. No hay picker humano extra para `"auto"`.

---

## Contrato MCP

Nombre del servidor: `atic`. Prefijo de tools: `atic_`. Un solo servidor; los
dominios se separan por prefijo, no por proceso.

Descripciones en **español de Chile, tuteo**, porque el modelo las lee y el
producto es chileno. Cortas, accionables, sin voseo.

### `atic_list_agents`

Sin argumentos. Devuelve los backends (desde la **cache** de disponibilidad):

```json
{
  "agents": [
    {
      "id": "claude-code",
      "name": "Claude Code",
      "available": true,
      "blurb": "Planes largos, repo, tools propias."
    }
  ]
}
```

`blurb` es texto estático por `id`. El modelo padre se apoya en esto para
elegir un `backend` concreto. `"auto"` no olfatea estos blurbs: mira `kind`.

Ids: `claude-code`, `codex`, `opencode`, `cursor`.

### `atic_list_sessions`

Opcional `backend`. Lista hilos **vivos en Atic** (no las TUI ajenas). Id,
backend, cwd, si hay turno corriendo, `parent` si es hijo de una delegación.

### `atic_spawn`

```json
{
  "backend": "codex",
  "cwd": "C:/Users/…/atic",
  "model": null,
  "permissionMode": "default",
  "label": "review del diff"
}
```

Equivale a `agent_start`. Cuenta contra la profundidad. Devuelve
`{ "session": "<uuid Atic>" }`. No manda el prompt.

Rechaza si `backend` no está `available`, si `depth` ya es el máximo, si ya
hay hilo vivo de ese backend+cwd (con sugerencia a `atic_prompt`), o si Atic
no está. Un spawn = un hijo. No hay fan-out.

### `atic_prompt`

```json
{
  "session": "<uuid>",
  "text": "…",
  "wait": true
}
```

Equivale a `agent_send`. Cuenta como un turno del presupuesto. Si `wait: true`
(default, y el único modo en v1), bloquea hasta `TurnEnded` o timeout y
devuelve siempre el `session`, el `status`, el texto (tope 32 KB) y, si no
terminó, un `hint`. `wait: false` queda para v2.

### `atic_delegate`

El verbo de producto. Un shot: spawn + un prompt + wait.

```json
{
  "backend": "auto",
  "kind": "review",
  "cwd": "C:/Users/…/atic",
  "text": "revisa el diff y lista riesgos",
  "permissionMode": "default"
}
```

`backend` es el id del harness **o** `"auto"`. Nunca las dos cosas a la vez.

Con `"auto"` el hub elige de forma **determinista sobre campos**, nunca sobre
el texto libre de `text`:

| Campo `kind` (enum, opcional) | Backend |
|---|---|
| `"plan"` | `claude-code` si está available; si no, el siguiente del desempate que esté |
| `"patch"` | `codex` si está |
| `"review"` | `codex` si está |
| `"apply"` | `cursor` si está |
| ausente, o el elegido no está available | orden de desempate: `claude-code`, `codex`, `cursor`, `opencode` — el primero available |
| un solo agente available | ese, ignore `kind` |

El modelo padre, si quiere ser fino, llama `atic_list_agents` y pasa un
`backend` concreto (`"codex"`, no `"auto"`). No se olfatea prosa. No se
entrena un router. La tabla se cambia en código, no en un modelo.

La respuesta **siempre** incluye el id de sesión, aunque el status no sea
`done`:

```json
{
  "session": "<uuid Atic>",
  "status": "done",
  "text": "…",
  "hint": null
}
```

`status`: `done` | `failed` | `timeout` | `permission_timeout`.

Por dentro: `spawn` + `prompt` + `wait`. La sesión **queda viva** para un
`atic_prompt` siguiente. No hay `stop` al terminar. No hay reintento si
falla. No hay segundo hijo en la misma llamada.

### Timeout como traspaso

Al timeout de 5 min (turno o permiso) el hub **no** devuelve solo un error y
**no** mata la sesión:

```json
{
  "session": "<uuid Atic>",
  "status": "timeout",
  "text": "<transcript parcial, tope 32 KB>",
  "hint": "La sesión sigue viva en Atic. Sigue con atic_prompt o mira atic_list_sessions."
}
```

Misma forma para `permission_timeout` (el hint dice que hay un permiso
pendiente en Atic). Los tokens ya gastados no se botan: el padre puede
continuar.

### `atic_cancel`

`{ "session": "<uuid>" }` → `agent_interrupt`. v1: interrupt del turno, no
matar la sesión.

### Lo que no va en el MCP

- Tools de filesystem, browser, git. Eso lo tiene cada CLI.
- `atic_dictate`, captura, OCR. Otro plan; mismo servidor cuando existan.
- Resources MCP (`atic://session/…`) en v1.
- Sampling MCP.
- Fan-out, reintento, committee.

---

## UI en Atic

Mínima en v1. El valor está en el MCP, no en una pantalla nueva.

- Ajustes → Agentes: bloque «Desde otras apps», con el estado del hub (puerto
  / «en marcha»), snippets por host (Claude, Codex, Cursor, OpenCode), y la
  ruta de `atic-mcp`.
- Consola: un hilo hijo se etiqueta con el padre («Codex · pedido por Cursor
  IDE» o «Codex · pedido por Claude …»). Reusar el transcript que ya existe;
  no inventar una vista de grafo.
- Permisos del hijo: el mismo `Permission` de siempre. Si el pedido vino por
  MCP, el copy puede decir que hay un padre esperando.
- Sin picker «¿a quién delego?». Elige el modelo padre o `"auto"` + `kind`.

Cero cambios de overlay / pill.

---

## Código propuesto (dónde vive, no el diff)

Nuevo:

| Ruta | Rol |
|---|---|
| `apps/desktop/src-tauri/src/agents/hub.rs` (o módulo vecino) | Servidor localhost, auth, jobs, reglas de grafo, cache de `available`, espera a `TurnEnded`. Fachada de `bridge` |
| `crates/atic-mcp/` (`[[bin]]` `atic-mcp`) | Cliente MCP stdio + HTTP al hub. `rmcp` **solo acá**. Grupos de tools registrables (hoy orquestación) |
| `Features/orquestacion-agentes.md` | Ficha `idea` → `parcial` cuando haya spawn+delegate |
| Ajustes: sección snippets | Svelte, i18n `es.ts` / `en.ts` |

Cambios:

| Ruta | Qué |
|---|---|
| `bridge.rs` | Operaciones internas (no solo `#[tauri::command]`). Primitivo de espera a fin de turno: el callback de deltas también despierta a quien espera `TurnEnded` |
| `claude_code.rs` | Al armar `--mcp-config`, **merge** del servidor `atic` |
| `codex.rs` / `acp.rs` | Comentario + `tracing::warn` si llega `mcp_config`: no se aplica. No fallar el start |
| spawn de hijos (bridge/hub) | Setear `ATIC_DELEGATE_DEPTH` y padre en el env **siempre** |
| `lib.rs` | Arrancar/parar el hub con la app |
| `crates/core` `AppDirs` | `hub.json` junto al resto de data |
| Empaquetado NSIS / sidecar | `atic-mcp.exe` al lado de Atic; snippet con ruta absoluta |
| Tests | Grafo (profundidad, ciclo, rechazo de reuso, ruteo por `kind`) **sin** CLI. Timeout → payload de traspaso. Cache de available. `atic-mcp` con hub fake |

No extraer `agents/` a un crate en v1.

SDK MCP: `rmcp` en el binario chico. En el desktop, JSON HTTP a mano (un hilo
+ `std::net::TcpListener`). No meter tokio en `atic-desktop` por esto.

---

## Fases

Cada fase cierra con algo usable.

### Fase 0 — Contrato y grafo, sin CLIs (~1–2 días)

- Tipos del hub: `list_agents`, `spawn`, `prompt`, `delegate`, `cancel`.
- Interfaz del primitivo de espera (`TurnEnded` / timeout / permission).
- Reglas de profundidad, ciclo, **rechazo** de reuso, presupuesto compartido
  entre spawn/prompt/delegate, ruteo `"auto"` **solo** por `kind`.
- Forma del payload de timeout-como-traspaso.
- Tests de todo lo anterior. Cero procesos de agente.

**Hecho cuando:** `cargo test` del grafo y del ruteo pasa.

### Fase 1 — Hub + `atic-mcp` contra Atic (~3–5 días)

- Hub localhost + `hub.json`.
- Cache de `available` (TTL o al arrancar el hub).
- Espera real a `TurnEnded` cableada al callback de deltas.
- `atic-mcp` stdio: handshake MCP, las tools, proxy al hub, progreso, timeout
  con transcript parcial.
- Atic arranca el hub; `list_agents` usa la cache.
- `spawn` / `prompt` / `delegate` contra el harness real.
- `ATIC_DELEGATE_DEPTH` en el env de cada hijo.
- Ajustes: «hub en marcha» + snippet Claude/Cursor/Codex.
- Claude Code **dentro de Atic** recibe el MCP mergeado en `--mcp-config`.
- Copy del error si Atic está cerrado.

**Hecho cuando:** con Atic abierto, Claude (el de Atic) puede
`atic_list_agents` y `atic_delegate` a Codex (si está instalado), el hilo
hijo se ve en la consola, y un timeout devuelve `session` + texto parcial.

**Riesgo:** permisos del hijo cuelgan el `tools/call`. Ejercerlo acá.

### Fase 2 — Apps originales (~2–3 días)

- Probar Cursor IDE y Codex CLI con el snippet pegado a mano.
- Copy de error cuando Atic está cerrado (el humano puede no estar mirando
  Atic: el mensaje tiene que bastar).
- Etiqueta de padre `external:cursor` / `external:codex` en la consola.
- No automatizar escritura de configs ajenas.

**Hecho cuando:** un recado desde Cursor IDE llega a Claude/Codex vía Atic y
vuelve el texto. Idem desde `codex` TUI. Atic cerrado → error legible.

### Fase 3 — Pulido (~2–4 días)

- Warn explícito de `mcp_config` ignorado en Codex/ACP.
- Tope de output, cancel, i18n (es + en) de snippets y descriptions de tools.
- Ficha `Features/` a `parcial`.
- No inyectar MCP en hijos Codex (decisión cerrada; esta fase no la reabre).
  Medir handshake de Codex solo si alguien propone reabrirla.

No hay fase «router inteligente», «A2A» ni «inyectar en Codex hijo».

Estimación total v1: **~2 semanas** de una persona que ya conoce el harness.

---

## Decisiones cerradas (no reabrir sin leer el porqué)

1. **Estrella, no N×N.** Un MCP, un hub, cuatro adaptadores que ya existen.
2. **MCP afuera, protocolo nativo adentro.** No A2A. No ACP como puerta del padre.
3. **Atic tiene que estar abierto.** Coherencia con permisos en Atic, no pereza
   de no embeber el harness.
4. **Binario sidecar `atic-mcp`,** no `Atic.exe --mcp` (single-instance + stdin).
   Un servidor, grupos de tools sumables después.
5. **No escribir configs de terceros** sin acción explícita (`ping.rs`).
6. **`"auto"` rutea por `kind` (enum) o desempate fijo.** Nunca por prosa.
7. **Profundidad 2, un turno por recado, timeout 5 min.** Spawn/prompt/delegate
   comparten el grafo. Sin reintento, sin fan-out.
8. **Permisos del hijo en Atic, default `default`.** El padre puede pasar
   `acceptEdits` explícito.
9. **Hijos no-Claude no heredan el MCP.** `ATIC_DELEGATE_DEPTH` viaja igual.
10. **Sesión hija viva.** Timeout = traspaso (parcial + id), no fracaso ciego.
11. **Mismo backend+cwd vivo → rechazar** con sugerencia a `atic_prompt`.
12. **`rmcp` solo en `atic-mcp`.** Desktop sin tokio nuevo.
13. **Ids de backend = los del harness.**
14. **No es Synara:** el modelo padre es dueño del loop.

---

## Trampas (las conocidas más las de este diseño)

Las de PLAN_AGENTES siguen valiendo: shims `.cmd`, `async-process`,
`AcpAgent::from_str` y `\`, handshake lento de Codex.

Encima:

- **Single-instance se come `Atic.exe --mcp`.** Por eso el sidecar. `ping.rs`
  ya pagó esta clase de error con stdin.
- **Codex levanta todos los MCP al abrir el hilo (~8 s).** Por eso v1 no
  inyecta `atic` en hijos Codex.
- **`agent_backends()` no es gratis.** Cachear `available`.
- **No hay espera a `TurnEnded` hoy.** Hay que construirla; no aparece sola
  al «exponer bridge a Rust».
- **`mcp_config` silencioso en Codex/ACP.** Warn + comentario, no fingir.
- **Recursión.** `ATIC_DELEGATE_DEPTH` viaja aunque el hijo no tenga el MCP
  (config global del usuario, o un Claude en el medio).
- **Cursor IDE ≠ `cursor-agent`.** El MCP spawnea el CLI que Atic conoce.
  Decirlo en el blurb de `cursor`.
- **El padre aprueba la tool; el hijo pide otra cosa.** Timeout-como-traspaso
  evita el deadlock y no bota el trabajo.
- **Output enorme.** Tope 8 KB en el item Atic; tope 32 KB en la respuesta MCP.
- **PATH de `atic-mcp` en el snippet.** Ruta absoluta real. Un binario «en el
  PATH» falla en Windows igual que `opencode`.
- **Framing MCP.** Lo cubre `rmcp`. No reimplementar a mano.
- **macOS:** el sidecar tendrá que firmarse el día que el desktop lo haga.
- **No mezclar el servidor `atic` de orquestación en `McpServersModal`.** El
  merge de `--mcp-config` y el editor del usuario son dos listas.

---

## Cómo probar

Sin gastar tokens (fase 0):

```text
cargo test -p atic-desktop -- hub::
```

(o el módulo que sea: grafo, `kind`, rechazo de reuso, payload de timeout)

Con Atic abierto y CLIs instalados (fase 1+):

1. Atic en bandeja. Ajustes → hub en marcha.
2. Consola Atic: Claude Code, prompt: «lista los agentes de Atic y no hagas
   nada más». Tiene que llamar `atic_list_agents`.
3. «Delega a Codex: responde solo hola». Hilo hijo en la consola; la
   respuesta trae `session`; Claude recibe «hola».
4. Segundo `atic_delegate` al mismo backend+cwd → rechazo con sugerencia a
   `atic_prompt`.
5. Pegar el snippet en Cursor IDE. Recado hacia Claude. Padre
   `external:cursor`. Cadena Cursor IDE → Claude → Codex si se pide un
   parche (`kind: "patch"` o `backend: "codex"` desde Claude).
6. Atic cerrado: Cursor llama la tool y el error habla de abrir Atic y de
   permisos, no un stack.
7. Profundidad: el tercer salto falla con mensaje de profundidad.
8. Permiso: hijo en modo default, Atic muestra el escudo; no aprobar; al
   timeout el padre recibe `permission_timeout` + `session` + hint.
9. Timeout de turno: igual, `status: "timeout"` + transcript parcial.

No usar `agente_real` para el MCP. Si hace falta un ejemplo: `atic-mcp`
contra un hub de fixture, o uno vivo.

---

## Cierres de la revisión (Fable 5, 2026-08-30)

Las afirmaciones del plan contra el código se sostienen: `mcp_config` solo
está cableado en `claude_code.rs`; ids `claude-code` / `codex` / `opencode` /
`cursor`; handshake de 8 s documentado en `codex.rs`; single-instance + stdin
pagado en `ping.rs`; data dir real de `AppDirs`. Arquitectura estrella y
perímetro, correctos.

Enmiendas respecto al borrador que se revisó: timeout-como-traspaso (2),
rechazo con sugerencia en vez de reuso (4), ruteo solo por `kind` (6).
Hallazgos que el borrador no nombraba: cache de `available`, primitivo de
espera a `TurnEnded`, ejemplo JSON inválido (corregido).

1. **Hub obligatorio: sí.** Sin Atic no hay dónde responder permisos. El
   default de la 7 lo vuelve coherencia de seguridad, no recorte. Cuidar el
   copy del error.
2. **`delegate` bloqueante: sí**, progreso MCP + timeout 5 min. Al timeout:
   transcript parcial + id de sesión + hint (`atic_prompt` /
   `atic_list_sessions`). No job+poll.
3. **Hijos no-Claude no heredan el MCP: sí.** `ATIC_DELEGATE_DEPTH` viaja
   igual. `mcp_config` en Codex/ACP: warn + comentario, no fingir ni fallar
   el start.
4. **Sesión hija viva: sí.** Toda respuesta de `atic_delegate` lleva
   `session`. Mismo backend+cwd vivo → rechazar, sugerir `atic_prompt`.
5. **Picker humano para `"auto"`: no en v1.** El humano puede no estar
   mirando Atic.
6. **Tabla tonta + `backend` explícito: sí.** Sin olfatear `text`. Solo
   `kind` (enum) o desempate fijo.
7. **`permissionMode` default = `default`: sí.** Aprobar la tool en Cursor
   no autoriza al hijo a editar. `acceptEdits` solo si el padre lo pasa.
8. **Profundidad 2: sí.** La cadena útil es Cursor IDE → Claude → Codex.
   Profundidad 1 la mata. Los otros frenos ya acotan el costo.
9. **Un solo servidor `atic`: sí.** El sidecar acepta grupos de tools
   después. Dos servidores = dos snippets y peor handshake de Codex.
10. **No es Synara; spawn/prompt se quedan.** El padre es dueño del loop.
    Las tres tools comparten profundidad y presupuesto. Sin reintento, sin
    fan-out.

---

## Traspaso para quien siga

1. Leer este archivo y [PLAN_AGENTES.md](PLAN_AGENTES.md) (hallazgo ACP,
   trampas de spawn, punto 5 vs 6). Las preguntas **ya están cerradas**.
2. Fase 0 primero: grafo + ruteo por `kind` + payload de timeout + interfaz
   de espera a `TurnEnded`. Tests. Cero `rmcp`, cero `bridge.rs` todavía
   más allá de leerlo.
3. No extraer `agents/` «por si el MCP lo necesita».
4. No escribir configs de Claude/Codex/Cursor en el disco del usuario.
5. No inyectar este MCP en Codex/ACP «por si acaso».
6. No olfatear el texto del recado para elegir backend.

La primera línea de código es el módulo puro del grafo, no el binario MCP.
