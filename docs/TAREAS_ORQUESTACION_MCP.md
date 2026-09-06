# Tareas — Orquestación de agentes vía MCP (traspaso de implementación)

Este documento es para quien **escribe el código** del plan
[`PLAN_ORQUESTACION_MCP.md`](PLAN_ORQUESTACION_MCP.md). El plan explica el
porqué; esto dice **qué hacer, dónde, en qué orden y cómo saber que quedó**.
Si algo acá contradice al plan, manda el plan y anota la discrepancia en el
reporte final.

Escrito el 2026-09-05 (Fable 5.1) sobre `main` en 0.4.28. Los números de
línea son de ese commit: úsalos como pista, no como verdad.

---

## 0. En una frase

Un binario `atic-mcp` (servidor MCP stdio, `rmcp`) que cualquier agente CLI
carga como tool server, y que le pide a Atic —por HTTP en localhost— que
liste, levante y le encargue un turno a otro agente usando el harness que ya
existe en `apps/desktop/src-tauri/src/agents/`.

Fuera de alcance (no lo hagas aunque parezca fácil): A2A, ACP como puerta del
padre, fan-out, reintentos, picker humano, router por prosa, inyectar el MCP
en hijos Codex/Cursor/OpenCode, spawn con Atic cerrado, SSH como hijo,
streaming token a token, extraer `agents/` a un crate, escribir configs de
Claude/Codex/Cursor/OpenCode en el disco del usuario.

---

## 1. Antes de escribir una línea

Lee, en este orden:

1. [`PLAN_ORQUESTACION_MCP.md`](PLAN_ORQUESTACION_MCP.md) completo, incluida
   la sección «Revisión 2026-09-05» y las enmiendas E1–E5.
2. [`PLAN_AGENTES.md`](PLAN_AGENTES.md) § «Trampas que ya pagamos» y
   § «Decisiones que conviene no revisar».
3. `agents/mod.rs` (traits `AgentBackend` / `AgentSession`, `StartOptions`),
   `agents/bridge.rs` (registro `SESSIONS`, `agent_start`, `agent_send`,
   `agent_interrupt`), `agents/model.rs` (`AgentDelta`, `ItemKind`,
   `TurnStatus`), `agents/turns.rs`, `agents/ping.rs` (precedente del
   snippet), `agents/exe.rs` (shims de Windows).
4. `CONTRIBUTING.md` y `docs/DEVELOPMENT.md` (toolchain, `CPATH`, validación).

Reglas del repo que aplican a todo lo de abajo:

- **Español de Chile, tuteo**, en UI, docs, errores, comentarios, nombres de
  tests y descripciones de tools. Sin voseo.
- Tests unitarios **en el mismo archivo** (`#[cfg(test)] mod tests`), con
  nombres que describen la regla: `el_tercer_salto_se_rechaza`, no `test_depth`.
- Comentarios que explican **por qué**, no qué. Mira el estilo de `codex.rs`.
- Validación antes de dar por hecha una fase:

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cd apps/desktop && pnpm verify
  ```

- No escribas `~/.claude.json`, `~/.codex/config.toml`, `~/.cursor/mcp.json`
  ni `opencode.json`. Se **ofrece** texto para pegar; el usuario lo pega.
- No agregues `tokio` ni `rmcp` a `apps/desktop/src-tauri`. Van solo en
  `crates/atic-mcp`.
- No toques `McpServersModal.svelte` salvo lo indicado en 5.4.
- No hagas commit ni push por tu cuenta; deja el árbol listo y reporta. La
  carpeta `keys/` está ignorada y así se queda.
- Commits (si te los piden): chicos, en español, imperativo: «Añadir el grafo
  del hub», «Cablear la espera a fin de turno».

---

## 2. Mapa de archivos

### Nuevos

| Ruta | Qué es |
|---|---|
| `apps/desktop/src-tauri/src/agents/hub/mod.rs` | Arranque/parada del hub, `hub.json`, comandos Tauri `hub_status` / `hub_snippet`, ruta de `atic-mcp` |
| `apps/desktop/src-tauri/src/agents/hub/graph.rs` | **Puro.** Reglas: profundidad, ciclo, reuso, ruteo `"auto"`, presupuesto de espera por host. Sin I/O |
| `apps/desktop/src-tauri/src/agents/hub/wait.rs` | **Puro.** `TurnWatch`: acumula el texto del turno y despierta a quien espera `TurnEnd` |
| `apps/desktop/src-tauri/src/agents/hub/server.rs` | HTTP JSON en `127.0.0.1:<efímero>` con token. Fachada sobre `bridge` |
| `apps/desktop/src-tauri/src/agents/hub/api.rs` | Tipos de request/response del hub (serde). Los comparte el server y los tests |
| `crates/atic-mcp/Cargo.toml`, `src/main.rs`, `src/tools.rs`, `src/hub_client.rs`, `src/budget.rs`, `src/payload.rs` | El sidecar MCP |
| `apps/desktop/src-tauri/binaries/` (ignorada en git) | Donde `tauri build` espera el sidecar con sufijo de triple |
| `scripts/build-mcp.ps1` y `scripts/build-mcp.sh` | Compilan `atic-mcp` y lo copian a `binaries/` con el nombre correcto |

### A modificar

| Ruta | Qué cambia |
|---|---|
| `Cargo.toml` (raíz) | `members += "crates/atic-mcp"` |
| `agents/mod.rs` | `pub mod hub;` y `StartOptions.env: Vec<(String, String)>` |
| `agents/bridge.rs` | `Entry` guarda `Arc<TurnWatch>` y metadatos de sesión; `agent_start` se parte en `start_session(...)` reutilizable; `on_delta` alimenta el `TurnWatch`; merge de `mcp_config`; env `ATIC_*`; helpers internos para el hub |
| `agents/claude_code.rs` | Aplicar `options.env` al `Command` local (línea ~88). Nada más |
| `agents/codex.rs` | Aplicar `options.env` (línea ~207); `tracing::warn!` una vez si llega `mcp_config` |
| `agents/acp.rs` | Aplicar `options.env` donde se arma el proceso; mismo `warn` |
| `agents/store.rs` + `crates/core` (db) | Campo `parent: Option<String>` en el hilo guardado, siguiendo el precedente de `remote_host_id` |
| `crates/core/src/paths.rs` | `pub fn hub_path(&self) -> PathBuf` → `data_dir/hub.json` |
| `src/lib.rs` | Registrar comandos del hub; arrancar el hub en `.setup`; pararlo en `RunEvent::Exit` **antes** de `stop_all` |
| `apps/desktop/src-tauri/tauri.conf.json` | `bundle.externalBin: ["binaries/atic-mcp"]` |
| `apps/desktop/package.json` | script `mcp:build` que llama al script de `scripts/`; `beforeBuildCommand` lo invoca |
| `.github/workflows/ci.yml`, `scripts/release-*.{ps1,sh}` | Compilar `atic-mcp` antes de `tauri build` |
| `.gitignore` | `apps/desktop/src-tauri/binaries/` |
| `apps/desktop/src/lib/ipc/agents.ts`, `core/types.ts` | `hubStatus()`, `hubSnippet(host)` |
| `apps/desktop/src/lib/features/settings/AgentsSection.svelte` | Grupo «Desde otras apps» |
| `apps/desktop/src/lib/core/i18n/es.ts` y `en.ts` | Claves nuevas bajo `settings.agents.*` |
| `apps/desktop/src/lib/AgentConversation.svelte` (o donde se pinte la cabecera del hilo) | Etiqueta «pedido por …» si el hilo tiene `parent` |
| `Features/orquestacion-agentes.md`, `PLAN_ORQUESTACION_MCP.md` | Estado y casillas al cerrar cada fase |

---

## 3. Contratos (esto es lo que no se negocia)

### 3.1 Tools MCP

Servidor: `atic`. Los hosts las exponen como `mcp__atic__<tool>`. Todas las
descripciones en español de Chile, tuteo, cortas. Los schemas van con
`additionalProperties: false`.

| Tool | Entrada | Salida |
|---|---|---|
| `atic_list_agents` | `{}` | `{ "agents": [ { "id", "name", "available", "blurb" } ] }` |
| `atic_list_sessions` | `{ "backend"?: Id }` | `{ "sessions": [ { "session", "backend", "cwd", "remote", "running", "parent", "label" } ] }` |
| `atic_spawn` | `{ "backend": Id, "cwd"?: string, "model"?: string, "permissionMode"?: Mode, "label"?: string }` | `{ "session": string }` |
| `atic_prompt` | `{ "session": string, "text": string, "wait"?: true }` | `Resultado` |
| `atic_delegate` | `{ "backend"?: Id \| "auto" (default `"auto"`), "kind"?: "plan" \| "patch" \| "review" \| "apply", "cwd"?: string, "text": string, "permissionMode"?: Mode, "model"?: string, "label"?: string }` | `Resultado` |
| `atic_wait` | `{ "session": string, "timeout_s"?: integer }` | `Resultado` |
| `atic_cancel` | `{ "session": string }` | `{ "session", "status": "cancelling" }` |

- `Id` = `"claude-code" | "codex" | "opencode" | "cursor"`. Son los `id()` del
  harness; no inventes alias.
- `Mode` = `"default" | "acceptEdits" | "plan" | "bypassPermissions" | "dontAsk"`.
  Default `"default"` (preguntar en Atic). Claude 2.1.261 acepta además
  `auto`; el harness lo desconoce y lo baja a `manual`: **no lo ofrezcas**.
- `atic_prompt` con `wait: false` responde error `wait_not_supported`
  («En esta versión `atic_prompt` siempre espera. Usa `atic_cancel` si quieres
  cortarlo.»). No lo implementes.
- `Resultado`:

  ```json
  {
    "session": "<uuid Atic>",
    "backend": "codex",
    "status": "done | failed | timeout | permission_timeout",
    "text": "<texto del asistente en ese turno, últimos 32 KB>",
    "hint": null,
    "elapsed_s": 12
  }
  ```

  `hint` es `null` en `done`; en los otros tres es una frase que dice qué
  hacer (ver 3.7). Si `text` se recortó, empieza con `[…recortado…]\n`.

- Descripciones (texto exacto, pégalo):

  | Tool | `description` |
  |---|---|
  | `atic_list_agents` | «Lista los agentes CLI que Atic puede levantar (Claude Code, Codex, Cursor, OpenCode) y si están instalados. Llámala antes de elegir un backend concreto.» |
  | `atic_list_sessions` | «Lista las sesiones de agente vivas en Atic: id, backend, carpeta y si están trabajando. Sirve para seguir una conversación con atic_prompt.» |
  | `atic_spawn` | «Abre una sesión nueva de un agente en una carpeta, sin mandarle nada todavía. Devuelve el id de sesión. Si ya hay una viva del mismo agente en esa carpeta, falla y te dice cuál usar.» |
  | `atic_prompt` | «Manda un mensaje a una sesión viva y espera la respuesta. Si el host corta antes de que termine, recibes el texto parcial y el id: sigue con atic_wait.» |
  | `atic_delegate` | «Encárgale una tarea a otro agente en un solo paso: lo abre, le manda el texto y espera. Con backend "auto", elige según kind (plan, patch, review, apply). La sesión queda viva para seguir con atic_prompt.» |
  | `atic_wait` | «Espera a que termine el turno que ya está corriendo en una sesión, sin mandar nada nuevo. Úsala cuando atic_delegate o atic_prompt volvieron con status timeout.» |
  | `atic_cancel` | «Interrumpe el turno en curso de una sesión. La sesión sigue viva.» |

- `blurb` estáticos por id:

  | id | name | blurb |
  |---|---|---|
  | `claude-code` | Claude Code | «Planes largos, repo completo, tools propias. El único que a su vez puede delegar.» |
  | `codex` | Codex | «Parches y reviews acotados. Tarda unos 8 s en abrir.» |
  | `cursor` | Cursor | «Aplica cambios con el CLI cursor-agent, no con el IDE.» |
  | `opencode` | OpenCode | «Generalista liviano, vía ACP.» |

### 3.2 API HTTP del hub

JSON sobre HTTP/1.1, solo `127.0.0.1`, puerto efímero. Header obligatorio
`Authorization: Bearer <token>`; sin él o con otro token → `401`. Body máximo
1 MB. Un request = una respuesta; nada de streaming.

| Método y ruta | Body | Respuesta |
|---|---|---|
| `GET /v1/health` | — | `{ "version", "pid" }` |
| `GET /v1/agents` | — | `{ "agents": [...] }` (desde la cache) |
| `GET /v1/sessions?backend=` | — | `{ "sessions": [...] }` |
| `POST /v1/spawn` | `{ backend, cwd, model, permissionMode, label, parent, depth, root, host }` | `{ "session" }` |
| `POST /v1/prompt` | `{ session, text, waitS }` | `Resultado` |
| `POST /v1/delegate` | `{ backend, kind, cwd, text, permissionMode, model, label, parent, depth, root, host, waitS }` | `Resultado` |
| `POST /v1/wait` | `{ session, waitS }` | `Resultado` |
| `POST /v1/cancel` | `{ session }` | `{ "session", "status": "cancelling" }` |

`parent`, `depth`, `root`, `host` los pone `atic-mcp` desde su entorno y sus
args (3.4). `waitS` es el presupuesto que el sidecar ya recortó al host (3.6);
el hub lo recorta otra vez a 300 s.

Errores: HTTP `400`/`401`/`404`/`409`, body
`{ "error": { "code", "message", "data"? } }`. Códigos:

| code | HTTP | Cuándo | `data` |
|---|---|---|---|
| `unauthorized` | 401 | Token malo o ausente | — |
| `bad_request` | 400 | JSON inválido, campo fuera del enum | — |
| `unknown_backend` | 400 | `backend` no es un id del harness | — |
| `backend_unavailable` | 409 | `available == false` en la cache (se refresca una vez antes de rendirse) | — |
| `depth_exceeded` | 409 | `depth >= 2` | `{ "depth", "max": 2 }` |
| `cycle` | 409 | Mismo `root`, backend+cwd ya es ancestro | `{ "chain": [...] }` |
| `already_running` | 409 | Hilo vivo del mismo backend+host+cwd | `{ "session" }` |
| `unknown_session` | 404 | Id que no está en `SESSIONS` | — |
| `remote_not_supported` | 400 | Alguien mandó `remote` | — |
| `wait_not_supported` | 400 | `wait: false` | — |

El `message` de cada error es la frase que el modelo va a leer: escríbela como
al usuario (3.7).

### 3.3 `hub.json`

Ruta: `AppDirs::hub_path()` = `<data_dir>/hub.json`
(`%APPDATA%\ciat\atic\data\hub.json` en Windows).

```json
{ "port": 51234, "token": "<64 hex>", "pid": 12345, "version": "0.4.28" }
```

- Se escribe al arrancar el hub, se borra (best-effort) al parar. Token nuevo
  por arranque: dos `uuid::Uuid::new_v4().simple()` concatenados.
- En Unix, `chmod 600`. En Windows, `%APPDATA%` ya es del usuario; no hagas
  ACLs a mano.
- `atic-mcp` lo lee en **cada** `tools/call`, no al arrancar. Si no existe, o
  `GET /v1/health` no responde en 1 s, es `hub_missing` (3.7). No revises el
  pid: el health lo cubre.

### 3.4 Variables de entorno de los hijos

Todo proceso de agente que Atic arranca (desde la UI **o** desde el hub) recibe:

| Var | UI arranca | Hub arranca |
|---|---|---|
| `ATIC_SESSION` | la clave local de la sesión | ídem |
| `ATIC_DELEGATE_DEPTH` | `0` | `depth + 1` del pedido |
| `ATIC_ROOT` | = `ATIC_SESSION` | el `root` del pedido |
| `ATIC_PARENT` | (no se setea) | el `parent` del pedido |

Van por `StartOptions.env` y cada adaptador las aplica con `Command::envs`.
Se setean **aunque el MCP no se inyecte** (Codex, Cursor, OpenCode): si ese
proceso carga el MCP por config global del usuario, el grafo no miente.
`atic-mcp` lee `ATIC_SESSION` como su `parent`; si falta, `parent =
"external:<host>:<pid del sidecar>"`. `root`: `ATIC_ROOT` si existe; si no,
el hub acuña uno en el primer spawn y lo devuelve en el env del hijo.

### 3.5 Reglas del grafo (constantes, en `graph.rs`)

| Regla | Valor |
|---|---|
| `MAX_DEPTH` | `2` (se rechaza cuando `depth >= 2` al pedir spawn) |
| Reuso | Rechazar si hay hilo vivo con misma `(backend, host, cwd)`; `host` = `"local"` o id SSH; `cwd` normalizado (ver abajo) |
| Ciclo | Rechazar si en la cadena del mismo `root` ya existe `(backend, cwd)` como ancestro |
| Presupuesto | `spawn` cuenta para profundidad, no abre turno; `delegate` y `prompt` abren **un** turno; `wait` y `cancel` no cuentan |
| Tope del hub por espera | `HUB_WAIT_MAX_S = 300` |
| Tope de texto | `TEXT_CAP_BYTES = 32 * 1024`, se conserva la **cola** |
| Cache `available` | TTL 60 s; refresco en background al arrancar el hub y al fallar un spawn por «no instalado» |

Normalización de `cwd` para la clave: `dunce`/`std::fs::canonicalize` si el
path existe, después lowercase en Windows y separadores `/`. Si no existe,
el string tal cual. Lo que **no** se hace: canonizar rutas remotas.

Ruteo `"auto"` (solo por `kind`, jamás por `text`):

| `kind` | Primera opción | Si no está |
|---|---|---|
| `plan` | `claude-code` | desempate |
| `patch` | `codex` | desempate |
| `review` | `codex` | desempate |
| `apply` | `cursor` | desempate |
| ausente | desempate | — |

Desempate: `claude-code`, `codex`, `cursor`, `opencode`, el primero
`available`. Un solo agente disponible → ese, ignorando `kind`. Ninguno →
`backend_unavailable`.

### 3.6 Presupuesto de espera por host (`budget.rs` en el sidecar)

| `--host` | `wait_s` |
|---|---|
| `claude-code` | 300 |
| `codex` | 50 |
| `cursor` | 50 |
| `opencode` | 50 |
| ausente / desconocido | 50 |

Si falta `--host`, intenta el `clientInfo.name` del `initialize` legacy (rmcp
lo expone en el peer); si contiene `claude` → 300, si no → 50. Un arg
opcional `--wait <s>` **reemplaza** el valor de la tabla (tope 300): es lo que
usa el snippet de Codex, porque ahí el usuario sube `tool_timeout_sec` y el
sidecar no tiene otra forma de saberlo. `atic_wait` usa
`min(timeout_s, presupuesto)`. Timeout HTTP del cliente = `wait_s + 15`.

Progreso: si el request trae `progressToken`, manda `notifications/progress`
cada 5 s con `progress` creciente y `message` «Esperando a Codex… (45 s)» o
«Esperando un permiso en Atic…» cuando el hub reporta permiso pendiente. Sin
token, no mandes nada.

### 3.7 Copys (texto exacto)

| Situación | Texto |
|---|---|
| `hub_missing` (sidecar) | «Atic no está abierto. Ábrelo desde la bandeja para delegar: sin Atic no hay quién apruebe los permisos del otro agente.» |
| `hint` en `timeout` | «La sesión sigue viva en Atic. Espera el turno con atic_wait, manda otro con atic_prompt, o mira atic_list_sessions.» |
| `hint` en `permission_timeout` | «El agente está esperando un permiso en Atic. Apruébalo o recházalo ahí y sigue con atic_wait.» |
| `hint` en `failed` | «El turno falló. Revisa el texto; la sesión sigue viva si quieres reintentar con atic_prompt.» |
| `already_running` | «Ya hay una sesión de {backend} viva en {cwd}: {session}. Sigue esa con atic_prompt en vez de abrir otra.» |
| `depth_exceeded` | «No se puede delegar más hondo: este agente ya es un hijo de un hijo. Resuelve tú la tarea o devuélvesela a quien te la pidió.» |
| `cycle` | «Ese agente ya está en la cadena de este encargo; devolvérselo sería un ciclo. Resuélvelo tú.» |
| `backend_unavailable` | «{name} no está instalado en este equipo. Llama atic_list_agents para ver cuáles hay.» |
| `remote_not_supported` | «Por ahora los hijos delegados corren en este equipo; SSH no se admite desde el MCP.» |

Los errores de tool van como `CallToolResult` con `isError: true` y el texto;
**no** como error JSON-RPC (el modelo lee mejor el primero).

### 3.8 Snippets por host (los genera `hub_snippet(host)`)

`{MCP}` es la ruta absoluta real de `atic-mcp`, con `\\` escapado dentro de
JSON/TOML.

Claude Code (comando):

```text
claude mcp add --scope user atic -- "{MCP}" --host claude-code
```

Cursor IDE / cursor-agent (JSON para `~/.cursor/mcp.json`):

```json
{ "mcpServers": { "atic": { "command": "{MCP}", "args": ["--host", "cursor"] } } }
```

Codex (TOML para `~/.codex/config.toml`; el `tool_timeout_sec` es la razón de
ir por TOML y no solo por `codex mcp add`):

```toml
[mcp_servers.atic]
command = "{MCP}"
args = ["--host", "codex", "--wait", "300"]
tool_timeout_sec = 330
```

Los dos números van juntos: si el usuario borra `tool_timeout_sec`, Codex
vuelve a cortar a los 60 s y el `--wait 300` solo consigue que el padre
pierda el `session`. Dilo en el texto de ayuda de Ajustes.

OpenCode (JSON para `opencode.json`):

```json
{ "mcp": { "atic": { "type": "local", "command": ["{MCP}", "--host", "opencode"], "enabled": true } } }
```

---

## 4. Fase 0 — Contrato y grafo, sin procesos (≈1–2 días)

Cero `rmcp`, cero HTTP, cero CLIs. Solo tipos y reglas con tests.

### 4.1 `agents/hub/api.rs`

Structs serde para todo lo de 3.2 (`SpawnRequest`, `DelegateRequest`,
`PromptRequest`, `WaitRequest`, `CancelRequest`, `Outcome`, `HubError { code,
message, data }`, `AgentInfo`, `SessionInfo`). `Outcome.status` es un enum
`OutcomeStatus { Done, Failed, Timeout, PermissionTimeout }` con
`rename_all = "snake_case"`. Deriva `Serialize + Deserialize + Clone + Debug
+ PartialEq` para poder compararlos en tests.

### 4.2 `agents/hub/graph.rs`

```rust
pub const MAX_DEPTH: u8 = 2;
pub const HUB_WAIT_MAX_S: u64 = 300;
pub const TEXT_CAP_BYTES: usize = 32 * 1024;

pub struct LiveSession { pub id: String, pub backend: String, pub host: String, pub cwd_key: String, pub root: Option<String>, pub running: bool }
pub struct SpawnCheck<'a> { pub backend: &'a str, pub host: &'a str, pub cwd_key: &'a str, pub depth: u8, pub root: Option<&'a str> }

pub fn cwd_key(cwd: &str) -> String;
pub fn check_spawn(req: SpawnCheck, live: &[LiveSession], chain_for_root: &[(String, String)]) -> Result<(), HubError>;
pub fn route(kind: Option<Kind>, available: &[(&str, bool)]) -> Option<&'static str>;
pub fn clamp_wait(requested_s: u64) -> u64;
pub fn cap_text(text: &str) -> String;
```

Tests (nombres orientativos, escribe al menos estos):

- `profundidad_dos_pasa_y_tres_se_rechaza`
- `mismo_backend_host_cwd_vivo_se_rechaza_con_la_sesion`
- `mismo_cwd_en_otro_host_no_es_reuso`
- `cwd_con_barras_y_mayusculas_da_la_misma_clave` (solo Windows: `#[cfg(windows)]`)
- `ciclo_en_el_mismo_root_se_rechaza`
- `mismo_backend_en_otro_root_no_es_ciclo`
- `kind_plan_prefiere_claude_y_cae_a_desempate`
- `kind_patch_y_review_prefieren_codex`
- `kind_apply_prefiere_cursor`
- `sin_kind_toma_el_primero_disponible_del_desempate`
- `un_solo_disponible_ignora_kind`
- `ninguno_disponible_no_rutea`
- `la_espera_se_recorta_a_300`
- `el_texto_se_recorta_por_la_cola_con_marca`

### 4.3 `agents/hub/wait.rs`

```rust
pub struct TurnWatch { inner: Mutex<TurnState>, cv: Condvar }

struct TurnState {
    running: bool,
    turn: Option<TurnId>,
    text: String,                 // texto del asistente del turno actual
    assistant_items: HashSet<ItemId>,
    pending_permissions: usize,
    last: Option<(TurnStatus, String)>,
    seq: u64,                     // sube en cada cambio; el que espera compara
}

pub enum WaitOutcome { Ended { status: TurnStatus, text: String }, Timeout { text: String, permission_pending: bool }, Idle { last: Option<(TurnStatus, String)> } }

impl TurnWatch {
    pub fn new() -> Arc<Self>;
    pub fn observe(&self, delta: &AgentDelta);          // lo llama on_delta
    pub fn wait_until(&self, deadline: Instant) -> WaitOutcome;
    pub fn is_running(&self) -> bool;
}
```

Reglas de `observe`:

- `TurnStart` → `running = true`, limpia `text`, `assistant_items`, `pending_permissions = 0`.
- `ItemAdd` con `ItemKind::Message { role: Assistant, text, .. }` → recuerda el
  `ItemId`, agrega `text`.
- `ItemChunk { item, text }` con `item` recordado → agrega.
- `ItemPatch { item, patch: { text: Some(t), .. } }` con `item` recordado →
  **reemplaza** el texto de ese item (los adaptadores mandan el texto completo
  al cerrar el streaming). Guarda el texto por item, no un solo buffer.
- `ItemAdd` con `ItemKind::Permission { status: Pending, .. }` →
  `pending_permissions += 1`; `ItemPatch` que lo deje en `Allowed`/`Denied` →
  `-= 1`.
- `TurnEnd { status, .. }` → `running = false`, `last = Some((status, cap_text(...)))`, `notify_all`.
- `Failed { message }` → `running = false`, `last = Some((TurnStatus::Failed, message))`, `notify_all`.
- Cualquier cambio: `seq += 1`, `notify_all`.

`wait_until`: si no está corriendo, devuelve `Idle` al tiro. Si corre,
`cv.wait_timeout` en bucle hasta `!running` o `deadline`.

Tests con deltas sintéticos (construye `AgentDelta` a mano, sin backend):

- `sin_turno_corriendo_devuelve_idle_de_inmediato`
- `el_texto_del_asistente_se_acumula_y_el_del_usuario_no`
- `un_patch_reemplaza_el_texto_del_item_y_no_lo_duplica`
- `turn_end_despierta_al_que_espera_con_el_status`
- `failed_cierra_el_turno_como_fallido`
- `al_vencer_dice_si_hay_permiso_pendiente`
- `el_texto_se_recorta_por_la_cola`

### 4.4 Hecho cuando

`cargo test -p atic-desktop hub::` pasa, `clippy -D warnings` limpio, y ningún
archivo fuera de `agents/hub/{mod,api,graph,wait}.rs` y `agents/mod.rs`
(solo el `pub mod hub;`) cambió.

---

## 5. Fase 1 — Hub + `atic-mcp` contra Atic (≈3–5 días)

### 5.1 `bridge.rs`: sesiones observables y arranque reutilizable

1. `Entry` suma `watch: Arc<TurnWatch>` y `meta: SessionMeta { cwd: String,
   remote_host_id: Option<String>, parent: Option<String>, label:
   Option<String>, depth: u8, root: String }`.
2. Parte `agent_start` en dos: el `#[tauri::command] agent_start(app, state,
   backend, options)` llama a `pub(crate) fn start_session(app: &AppHandle,
   backend: &str, req: StartRequest, spawn: SpawnMeta) -> Result<String,
   String>`. `SpawnMeta { parent, depth, root, label }`; desde la UI es
   `SpawnMeta::root()` (depth 0, root = la clave nueva).
3. Dentro de `start_session`, **antes** de `agent.start(...)`:
   - `options.env` = las cuatro vars de 3.4.
   - Si `backend == "claude-code"` y `remote.is_none()`: `options.mcp_config =
     Some(merge_mcp_config(&cfg.agent_mcp_servers, req.mcp_config, hub::mcp_server_entry()))`.
     `merge_mcp_config` produce `{"mcpServers": {...}}` con: los servidores
     del modal que estén `enabled` (la config guarda un array JSON de
     `{ name, json, enabled }`; mira `McpServerConfig` en `core/types.ts`),
     luego lo que venga en `req.mcp_config`, y al final `"atic"` si
     `hub::mcp_server_entry()` devuelve `Some` (hub corriendo **y** binario
     encontrado). Con conflicto de nombre gana el usuario, salvo `atic`, que
     es nuestro. Test: `el_merge_suma_atic_y_respeta_lo_del_usuario`.
   - Si `backend != "claude-code"` y `req.mcp_config.is_some()`: no lo
     rechaces; el adaptador va a hacer `warn` (5.3).
4. En el closure `on_delta`: primera línea `watch.observe(&delta);`, después
   lo que ya hay (store, emit). El orden importa: quien espera tiene que
   despertar aunque el emit falle.
5. Helpers `pub(crate)` para el hub, todos sin `State`/`AppHandle` salvo
   `start_session`:
   - `live_sessions() -> Vec<graph::LiveSession>` (lee `SESSIONS` + `meta` + `watch.is_running()`).
   - `session_watch(id) -> Option<Arc<TurnWatch>>`.
   - `send_text(id, text) -> Result<(), String>` (lo que hoy hace `agent_send` sin `origin`).
   - `interrupt(id)` (reusa `agent_interrupt`).
   - `session_info(id) -> Option<api::SessionInfo>`.
6. `store::open(...)` recibe `parent: Option<&str>` y lo guarda (5.5).

### 5.2 Env en los adaptadores

- `claude_code.rs` ~línea 88 (rama local): `cmd.envs(options.env.iter().cloned())`.
  La rama SSH no cambia (v1 no manda hijos remotos).
- `codex.rs` ~línea 207: ídem, antes de `spawn()`.
- `acp.rs`: busca dónde se construye el proceso (`super::exe::launcher(...)`
  y el `spawn` de `async_process`/`agent-client-protocol`) y aplica lo mismo.
  Si el tipo de comando no expone `envs`, busca `env`; si no expone nada,
  reporta y no improvises un wrapper de shell.

Test por adaptador (donde ya hay tests de args): `las_vars_atic_viajan_en_el_env`
para Claude (revisa el `Command` armado) y al menos un test de que
`StartOptions::default().env` está vacío no rompe nada.

### 5.3 `mcp_config` en Codex y ACP

En `start` de `codex.rs` y `acp.rs`:

```rust
if options.mcp_config.is_some() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| tracing::warn!(backend = %self.id(), "mcp_config no se aplica en este backend; solo Claude Code lo toma"));
}
```

Más un comentario de dos líneas encima que diga por qué no se implementa
(handshake de Codex, decisión 9 del plan). No falles el start.

### 5.4 `McpServersModal.svelte`

Solo una cosa: el texto de ayuda al pie ya dice que Atic no reemplaza los MCP
del CLI; agrega una frase: «El servidor `atic` de orquestación se suma solo;
no lo pongas acá.» (con su clave i18n). Nada más.

### 5.5 Hilo con padre

Sigue el camino que abrió `remote_host_id` (columna nullable en
`agent_threads`, campo en `AgentThreadRow`, en `StoredThread`, en `open(...)`
y en el tipo TS `StoredThread`). Campo `parent: Option<String>`. Valor: el
`parent` del pedido (`<uuid>` de una sesión Atic o `external:cursor:1234`).
Etiqueta en la cabecera del hilo: «Codex · pedido por Cursor IDE» /
«· pedido por Claude Code» / «· pedido por otra sesión». Mapa
`external:<host>` → nombre bonito en TS.

### 5.6 `agents/hub/server.rs` + `mod.rs`

- Dependencia nueva en el desktop: `tiny_http = "0.12"` (síncrono, sin tokio).
  Si prefieres `std::net::TcpListener` a mano, que el parser de HTTP sea de
  verdad (líneas de header, `Content-Length`, nada de `chunked`), con tests.
  `tiny_http` es la recomendación.
- `hub::start(app: AppHandle) -> Result<(), String>`: bind `127.0.0.1:0`,
  token, escribe `hub.json`, guarda `HubState { port, token, mcp_path }` en
  un `static OnceLock`, lanza el hilo aceptador. Cada request se atiende en
  **su propio hilo** (`std::thread::spawn`): las esperas bloquean minutos.
- `hub::stop()`: cierra el listener (guarda un flag y conecta a sí mismo para
  destrabar `accept`, o usa `Server::unblock`), borra `hub.json`.
- Rutas de 3.2. `delegate` = `route` (si `"auto"`) → `check_spawn` →
  `start_session` → `send_text` → `wait_until`. `prompt` = `send_text` →
  `wait_until`. `wait` = `wait_until`. Mapea `WaitOutcome` a `Outcome`:
  `Ended(Done)` → `done`; `Ended(Failed|Cancelled)` → `failed`; `Timeout {
  permission_pending: true }` → `permission_timeout`; `Timeout` → `timeout`;
  `Idle { last }` → el último resultado, o `done` con `text: ""`.
- El `root` del hijo: `req.root` o, si viene vacío, la clave nueva.
- Cache `available`: `Mutex<Option<(Instant, Vec<BackendInfo>)>>` en `mod.rs`
  con `refresh()` en un hilo al arrancar; `agents()` devuelve lo cacheado y
  dispara refresco si venció el TTL. Nunca llames `is_available` en el hilo
  del request salvo el reintento único de `backend_unavailable`.
- Comandos Tauri en `mod.rs`: `hub_status() -> HubStatus { running, port,
  mcp_path: Option<String> }` y `hub_snippet(host: String) -> Result<String,
  String>` (3.8). Regístralos en `lib.rs` junto a los otros `agents::*`.
- `hub::mcp_path() -> Option<PathBuf>`: (1) `current_exe().parent()/atic-mcp[.exe]`;
  (2) si `cfg!(debug_assertions)`: `<CARGO_MANIFEST_DIR>/../../../target/{debug,release}/atic-mcp[.exe]`,
  el más nuevo; (3) `None`.
- `hub::mcp_server_entry() -> Option<serde_json::Value>`:
  `{"type":"stdio","command":"<mcp_path>","args":["--host","claude-code"]}`
  si el hub corre y hay binario.
- En `lib.rs`: en `.setup`, después de leer `dirs`, `if agents::UI_ENABLED {
  if let Err(e) = agents::hub::start(app.handle().clone()) { tracing::warn!(...) } }`.
  En `RunEvent::Exit`, `agents::hub::stop();` **antes** de `stop_all(app)`.
- `paths.rs`: `pub fn hub_path(&self) -> PathBuf { self.data_dir.join("hub.json") }`.

Tests en `server.rs` (sin CLIs): arranca el hub con un `SESSIONS` vacío y
pega con `reqwest::blocking` (ya está en el desktop) a `/v1/health`
(200 con token, 401 sin token), `/v1/agents` (forma), `/v1/spawn` con
backend inválido (400 `unknown_backend`), `/v1/wait` con sesión desconocida
(404). No pruebes spawn real acá.

### 5.7 `crates/atic-mcp`

`Cargo.toml`:

```toml
[package]
name = "atic-mcp"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "atic-mcp"
path = "src/main.rs"

[dependencies]
rmcp = { version = "3", features = ["server", "transport-io", "macros"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
reqwest = { version = "0.12", default-features = false, features = ["json"] }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
directories = "6"
anyhow = { workspace = true }
```

Usa `rmcp::schemars` (reexportado) para los `JsonSchema`; no agregues
`schemars` aparte, o vas a pelear con versiones. Comprueba en
`cargo doc -p rmcp` o en el repo del SDK el nombre exacto de:
`#[tool_router]`, `#[tool]`, `Parameters<T>`, `RequestContext<RoleServer>`,
`peer.notify_progress`, `ServerHandler::get_info`. La regla del repo aplica:
escribe el archivo, compila, corrige por el error del compilador; no pases una
tarde leyendo docs antes de la primera línea.

Módulos:

- `main.rs`: parsea `--host <id>` y `--wait <s>` (a mano, sin `clap`), configura
  `tracing_subscriber` **a stderr**, `Server::new(host).serve(stdio()).await?;
  service.waiting().await?`. Nada, nunca, a stdout fuera de `rmcp`.
- `budget.rs`: tabla 3.6 + `wait_budget(host: Option<&str>, wait_flag:
  Option<u64>, client_name: Option<&str>) -> u64`. Tests:
  `claude_por_flag_tiene_300`, `cursor_tiene_50`, `sin_pistas_es_50`,
  `client_info_con_claude_da_300`, `wait_explicito_manda_y_se_recorta_a_300`.
- `hub_client.rs`: `locate() -> Result<Hub, HubMissing>` (lee `hub.json` vía
  `directories::ProjectDirs::from("com","ciat","atic")` — el mismo triple que
  `paths.rs` — y prueba `/v1/health` con timeout 1 s); métodos `agents()`,
  `sessions()`, `spawn()`, `prompt()`, `delegate()`, `wait()`, `cancel()`
  con timeout `wait_s + 15`. Traduce `{ error }` del hub a un tipo `HubError`
  y `HubMissing` al copy 3.7.
- `payload.rs`: los mismos tipos de `api.rs`, copiados (no compartas un crate
  con el desktop: decisión del plan). Un test que deserializa un `Outcome`
  de ejemplo.
- `tools.rs`: struct `AticServer { host: Option<String> }` con el
  `#[tool_router]`. Cada tool: `locate()` → si falla, `CallToolResult::error`
  con el copy; si no, arma el request con `parent` (3.4), `depth`
  (`ATIC_DELEGATE_DEPTH`, default 0), `root` (`ATIC_ROOT`), `host`, `waitS`;
  mientras espera la respuesta HTTP, un `tokio::spawn` manda progreso cada 5 s
  si hay `progressToken`. `get_info()` declara `tools` y el nombre `atic`.

Tests de integración en `crates/atic-mcp/tests/`:

- `hub_fake.rs`: un `std::net::TcpListener` en un hilo que responde JSON
  fijo; escribe un `hub.json` en un `data_dir` temporal apuntándolo mediante
  una variable de entorno `ATIC_HUB_JSON` que `locate()` respeta **solo en
  tests** (`cfg!(test)` no cruza crates: usa una feature `test-hooks` o lee la
  var siempre y documenta que es para tests).
- `tools_contra_hub_fake.rs`: `atic_list_agents` devuelve la lista;
  `atic_delegate` con el hub caído devuelve `isError` con el copy exacto de
  `hub_missing`; `atic_prompt` con `wait: false` devuelve `wait_not_supported`.
- `stdio_handshake.rs`: lanza el binario (`env!("CARGO_BIN_EXE_atic-mcp")`),
  escribe una línea `initialize` legacy (`protocolVersion: "2025-11-25"`) y
  otra `tools/list`, y comprueba que **cada** respuesta es una línea JSON
  válida, que `tools` trae las siete y que no salió nada raro por stdout.

### 5.8 Empaquetado del sidecar

- `scripts/build-mcp.ps1`: `cargo build -p atic-mcp --release`, luego copia
  `target/release/atic-mcp.exe` a
  `apps/desktop/src-tauri/binaries/atic-mcp-$(rustc --print host-tuple).exe`.
  `build-mcp.sh` igual sin `.exe`.
- `apps/desktop/package.json`: `"mcp:build": "node ../../scripts/build-mcp.mjs"`,
  y que ese `.mjs` elija `powershell -NoProfile -File build-mcp.ps1` en
  Windows y `sh build-mcp.sh` en el resto (la máquina de desarrollo tiene
  Windows PowerShell 5.1, no `pwsh`). Mismo estilo que
  `scripts/test-color-picker.mjs`.
- `tauri.conf.json`: `"beforeBuildCommand": "pnpm mcp:build && pnpm build"` y
  `"bundle": { ..., "externalBin": ["binaries/atic-mcp"] }`. Ojo: `tauri-build`
  exige que el archivo exista **al compilar**, no solo al empaquetar; por eso
  `pnpm tauri dev` también necesita el binario. Documenta en
  `docs/DEVELOPMENT.md` que la primera vez hay que correr `pnpm mcp:build`.
- `.gitignore`: `apps/desktop/src-tauri/binaries/`.
- `.github/workflows/ci.yml`: paso `pnpm mcp:build` antes de
  `pnpm tauri build --no-bundle`. `scripts/release-windows.ps1` y
  `release-macos.sh`: lo mismo antes del build.

### 5.9 Ajustes → Agentes → «Desde otras apps»

En `AgentsSection.svelte`, un `SettingsGroup` nuevo debajo del pager:

- Fila «Hub de orquestación»: «En marcha en el puerto N» / «No arrancó» (con
  el error si lo hay). Botón «Actualizar».
- Fila «Ruta de atic-mcp»: la ruta o «No se encontró el binario; ejecuta
  `pnpm mcp:build`» en dev.
- Cuatro filas (Claude Code, Cursor, Codex, OpenCode), cada una con el
  snippet en un `<pre>` y botón copiar, **reusando** el patrón `copySnippet`
  que ya está en el archivo. Texto de ayuda: «Pega esto en la app
  correspondiente. Atic no toca esos archivos.»
- Todo detrás de `AGENTS_ENABLED`.
- Claves i18n bajo `settings.agents.hub*` en `es.ts` y `en.ts`.
- `ipc/agents.ts`: `hubStatus()`, `hubSnippet(host)`; tipos en `core/types.ts`.

### 5.10 Hecho cuando (demostrable, con Atic abierto)

1. `hub.json` aparece al abrir Atic y desaparece al cerrarla.
2. Consola de Atic → Claude Code → «lista los agentes de Atic y no hagas nada
   más» → llama `mcp__atic__atic_list_agents` y muestra los cuatro con su
   `available`.
3. «Delega a Codex: responde solo hola» → aparece un hilo hijo en la consola
   etiquetado «Codex · pedido por Claude Code»; Claude recibe `status: done`,
   `text` con «hola» y `session`.
4. Repetir 3 → `already_running` con el id y la sugerencia.
5. Con el hijo en modo `default` y una tool que pida permiso (p. ej.
   «crea un archivo x.txt»): Atic muestra el escudo; si no lo apruebas, a los
   300 s Claude recibe `permission_timeout` + `session` + hint; si lo
   apruebas, `done`.
6. `atic_wait` sobre esa sesión mientras trabaja devuelve `done` al terminar.
7. Cierra Atic y llama una tool desde un `claude` de terminal con el snippet
   pegado: el texto es el de `hub_missing`, no un stack.
8. `cargo test --workspace` verde, `clippy` limpio, `pnpm verify` verde,
   `pnpm tauri build` produce el NSIS con `atic-mcp.exe` al lado de `Atic.exe`.

---

## 6. Fase 2 — Apps originales (≈2–3 días)

- Pega el snippet de Cursor en `~/.cursor/mcp.json`; en el chat de Cursor:
  «usa atic para pedirle a Claude que resuma este repo en tres líneas». Debe
  volver el texto antes de 50 s o volver `timeout` **con** `session`, y un
  segundo turno con `atic_wait` debe traer el resultado. Padre en la consola:
  «pedido por Cursor IDE».
- Ídem con `codex` TUI y el TOML completo (`--wait 300` +
  `tool_timeout_sec = 330`): un encargo de dos minutos vuelve entero, sin
  pasar por `atic_wait`. Prueba también el TOML **sin** `tool_timeout_sec`:
  Codex corta a los 60 s y el modelo ve el error del host; eso es lo esperado
  y es lo que el texto de ayuda advierte.
- Cadena Cursor IDE → Claude (plan) → Codex (parche): el segundo salto pasa
  porque Claude lleva el MCP; el tercero devuelve `depth_exceeded`.
- Atic cerrada en cada host: el modelo muestra el copy de `hub_missing`.
- No automatices la escritura de configs ajenas. Si un host necesita un
  paso más (Cursor pide aprobar el servidor la primera vez), ponlo en el
  texto de ayuda de Ajustes.

Hecho cuando los seis puntos de arriba se cumplen en Cursor y en Codex, y
queda anotado en `Features/orquestacion-agentes.md` cuáles hosts se probaron
con qué versión.

---

## 7. Fase 3 — Pulido (≈2–4 días)

- i18n completo (es + en) de snippets, ayuda y estados del hub.
- `atic_cancel` probado contra los cuatro backends (Codex y ACP tienen
  `interrupt` propio; comprueba que `TurnEnd { Cancelled }` llega y el
  `TurnWatch` lo mapea a `failed` con hint «cancelado»).
- Tope de 8 KB por item en la consola para los hilos hijos si el texto se
  dispara (revisa qué hace hoy `store::apply` con textos grandes antes de
  tocar nada).
- `Features/orquestacion-agentes.md`: estado `idea` → `parcial`; casillas.
- `PLAN_ORQUESTACION_MCP.md`: línea de estado «fase N hecha, commit X».
- No reabras «inyectar en Codex hijo». Si alguien lo pide, mide el handshake
  primero y anótalo en el plan.

---

## 8. Trampas (léelas dos veces)

- **stdout es sagrado en el sidecar.** Un `println!` de debug rompe el
  transporte. `tracing` a stderr, siempre.
- **Framing MCP** = una línea JSON por mensaje. Nada de `Content-Length`.
  `rmcp` lo hace; no lo toques.
- **`tools/list` sin tocar el hub.** OpenCode corta a los 5 s, Codex a los
  10 s de arranque, Claude `-p` espera hasta 30 s. La lista es estática.
- **El host corta a los 60 s (Codex, Cursor).** Por eso el presupuesto por
  `--host` y `atic_wait`. Si te tienta «esperar un poco más», no.
- **Claude interactivo manda a segundo plano a los 2 min.** No es un error:
  el `tools/call` sigue abierto y el resultado llega como notificación. No
  intentes «evitarlo».
- **`agent_backends()` lanza procesos.** Cache con TTL; jamás en el hilo del
  request salvo el único reintento de `backend_unavailable`.
- **Shims `.cmd` en Windows.** Usa `exe::resolve` / `exe::launcher` como los
  adaptadores; nunca `Command::new("opencode")`.
- **Single-instance se come `Atic.exe --mcp`.** Por eso el sidecar. No
  agregues un flag `--mcp` al desktop.
- **`agent_mcp_servers` hoy no se aplica.** No asumas que la UI manda
  `mcpConfig`; el merge vive en Rust (5.1).
- **Codex tarda ~8 s en abrir.** Un `delegate` a Codex con presupuesto de
  50 s tiene ~40 s útiles. Está bien: el resto es `atic_wait`.
- **`ATIC_*` viaja siempre**, inyectes o no el MCP. Si te falta en un
  adaptador, el grafo miente y el tercer salto pasa.
- **`cwd` remoto no se canoniza.** Solo local. Y `atic_spawn` rechaza
  `remote` de plano.
- **Nombre doble `mcp__atic__atic_*`.** Es feo y es estable. No lo cambies sin
  actualizar snippets, descripciones y el plan a la vez.
- **`--permission-mode auto`** existe en Claude 2.1.261; el harness lo baja a
  `manual`. No lo ofrezcas en el enum del MCP.
- **Permisos del hijo**: el turno del padre se queda en el `tools/call`. El
  progreso dice «esperando un permiso en Atic». No auto-apruebes nunca.
- **Rutas en snippets**: absoluta y real. `binaries/…-x86_64-pc-windows-msvc.exe`
  es el nombre de **build**; instalado es `atic-mcp.exe` junto a `Atic.exe`.

---

## 9. Qué entregar al terminar cada fase

Un reporte corto (en el PR o en el chat) con:

1. Fase cerrada y commits (o el árbol sin commitear, si así te lo pidieron).
2. Salida de `cargo test --workspace` (resumen), `clippy` y `pnpm verify`.
3. Los pasos de «Hecho cuando» que ejecutaste a mano, con qué versión de cada
   CLI (`claude --version`, `codex --version`, `cursor-agent --version`,
   `opencode --version`).
4. Lo que **no** hiciste y por qué, y cualquier punto donde este documento o
   el plan no calzaban con el código. Eso se corrige en el plan, no se
   esquiva en el código.
5. Archivos de docs tocados: este, el plan, la ficha de `Features/`,
   `DEVELOPMENT.md` si cambió el flujo de build.

---

## 10. Correcciones tras la validación (Fable 5.1, 2026-09-05)

Validado en máquina: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace` (284 en el desktop
con 24 de `hub::`, migración 5 en `core`, 9 en `atic-mcp` incluido el
handshake stdio), `pnpm check` (0 errores) y `pnpm test` (499). `pnpm lint`
y `pnpm lint:css` fallan en archivos que esta tarea no tocó. Falta la demo
manual del §5.10 y `pnpm tauri build`.

Lo que queda por corregir, en orden:

1. **`hub::server::stop()` no borra `hub.json`** (§3.3). Guarda la ruta en
   `HubState` y haz `remove_file` best-effort en `stop()`.
2. **La cache de `available` bloquea.** `refrescar_disponibles()` corre en
   el hilo aceptador **antes** de `aceptar()`: los primeros segundos tras
   abrir Atic el hub no acepta y el sidecar responde «Atic no está abierto».
   Y `disponibles()` vuelve a sondear en el hilo del request cuando venció
   el TTL. Lanza el refresco en su propio hilo y sirve la cache vieja
   mientras tanto.
3. **Schemas del sidecar con `String` suelto.** `backend`, `kind` y
   `permissionMode` tienen que ser enums en el schema (§3.1): un `kind` con
   typo hoy cae al desempate sin aviso y `permissionMode: "auto"` termina en
   `manual` en silencio. El hub valida `permission_mode` contra la lista.
4. **Snippet de Claude con `\\`.** `hub_snippet` escapa las barras para
   todos los hosts; en la línea de comandos de `claude mcp add` no
   corresponde. Escapa solo en JSON y TOML.
5. **Toast equivocado en Ajustes.** El botón de copiar de los snippets del
   hub muestra `settings.agents.copiedToast` («Fusiona la clave hooks…»).
   Clave propia.
6. **Código de error al fallar `start_session`.** `spawn` y `delegate`
   devuelven `409` con `code: "bad_request"`. Usa `spawn_failed`.
7. **Tests del servidor del hub.** Se quitaron por un 0xc0000139 atribuido
   al módulo de tests; ese código es «entry point not found» de una DLL y
   el binario de tests del desktop carga bien en esta máquina. Vuelve a
   escribirlos con `aceptar(listener, token, None)` en un hilo: `health`
   con y sin token, forma de `agents`, `spawn` con backend inventado, `wait`
   con sesión desconocida.
8. **`tools_contra_hub_fake.rs` no ejercita el sidecar.** El primer test le
   pega al hub falso con `reqwest`; el segundo comprueba que un literal
   contiene un trozo de sí mismo. O expones `hub_client`/`tools` en un
   `lib.rs` del crate y los llamas, o los borras: la cobertura real está en
   `stdio_handshake.rs`.
9. **`cwd_key` muestra `//?/c:/…`** en el mensaje de `already_running`
   (prefijo verbatim de `canonicalize`). Quítalo.
10. Menores: `_state: State<AppState>` sin uso en `agent_start`; «Kapta» →
    «captura» en un comentario de `bridge.rs`; un padre Claude que delega a
    Claude en su propio cwd recibe `already_running` apuntándose a sí mismo
    (el mensaje podría decirlo).

Fuera del MCP, pero mezclado en el mismo árbol (decide antes de commitear):

- `launcher_recents.rs`: `process_fits_app` cambió para que pasara el test
  preexistente `code_no_es_barcode`. Ahora la rama por título de ventana
  exige además que el stem del `.exe` aparezca en título, ventana o id.
  Riesgo: apps cuyo exe no se parece al nombre (WINWORD/Word,
  POWERPNT/PowerPoint, msedge/Edge) pueden dejar de detectarse como abiertas.
- Rama de la pill (`ChipTarget`, `AgentChip.id`, presencia, foco): trabajo
  en curso ajeno a esta tarea; `ChipTarget.console` ganó `presenceId?` para
  que compilara.
- Lints `-D warnings` en archivos preexistentes (`annotate`, `commands`,
  `overlay`, `ssh`, `focus`, `fs_browse`, `claude_sessions`, `claude_code`,
  `secrets`, `claude_usage` con el voseo) y 13 archivos solo reformateados
  por `cargo fmt`.

Sugerencia de commits: (1) fmt + lints en archivos preexistentes;
(2) la rama de la pill; (3) la orquestación MCP completa (`hub/`,
`atic-mcp/`, puente, adaptadores, migración 5, UI, scripts, docs,
`Cargo.lock`).

### Estado al 2026-09-06 (segunda validación, Fable 5.1)

Los diez puntos de arriba quedaron aplicados (segunda ronda de Grok) y
verificados. Además:

- **0xC0000139 al correr los tests del desktop, resuelto.** No era el
  entorno ni un módulo de tests: `tauri-build` embebe el manifiesto de
  Windows (Common-Controls v6) solo en los binarios, el exe de tests carga
  `comctl32` v5 de System32, y `rfd`/`muda` importan `TaskDialogIndirect`,
  que solo existe en la v6. Apareció con los tests HTTP del hub porque
  alcanzan código que referencia esos crates. `build.rs` ahora enlaza con
  `/DELAYLOAD:comctl32.dll` (+ `delayimp.lib`): el exe de tests no lo
  resuelve al arrancar y la app lo carga en el primer uso, ya con el
  manifiesto activo. Repetir el recurso con `rustc-link-arg` no sirve: el
  bin falla con `CVT1100 duplicate resource`.
- **Tests HTTP del hub: carrera arreglada.** El flag `VIVO` era global y
  cada test lo apagaba al terminar, matando el `accept` de los demás. Ahora
  es un `Arc<AtomicBool>` por aceptador; `stop()` apaga el del hub en marcha.
- **Demo §5.10 hecha con Atic en dev y CLIs reales** (`claude` 2.1.261,
  `codex` 0.153.4), hablándole al sidecar por stdio: `initialize` legacy y
  `tools/list`; `atic_list_agents` (4 disponibles); `atic_delegate` a Codex
  → `done` «hola» en 11 s con progreso cada 5 s; repetir → `already_running`
  con el id; `atic_list_sessions` con `parent` y `label`; `atic_prompt` →
  «chao»; `atic_wait` sin turno → último resultado; `atic_cancel`;
  hijo Claude en modo `default` con una escritura → `permission_timeout` a
  los 50 s con `session` y `hint`; hijo Claude con `bypassPermissions` lee
  `ATIC_DELEGATE_DEPTH=1`, `ATIC_ROOT`/`ATIC_SESSION` = su clave, y llama
  `atic_list_agents` por el MCP mergeado en `--mcp-config`. `parent` queda
  en SQLite. Con Atic cerrado el sidecar responde el copy de `hub_missing`
  (Grok).

Pendiente:

- `launcher_recents::tests::winword_abre_word_por_titulo` está **rojo**: es
  un test nuevo (ronda 2 de Grok) que espera un alias `winword` → Word que
  no existe en el código; el título «Informe - Word» lleva la app al final y
  `process_fits_app` solo mira el primer segmento. Implementar o quitar el
  test; es del launcher, no del MCP.
- `TurnWatch::combinado()` pega los mensajes del asistente sin separador
  («…en paralelo.depth=1…»). Unir con `\n`.
- No se observó en vivo el borrado de `hub.json` al cerrar Atic con
  gracia (solo el test unitario `stop_borra_hub_json`); matar el proceso lo
  deja en disco, y el sidecar lo tolera vía `health`.
- `pnpm tauri build` con el sidecar dentro del NSIS; fase 2 (Cursor IDE y
  Codex TUI con los snippets); la cadena de tres saltos hasta
  `depth_exceeded`.

---

## 11. Todos delegan, todos reciben (2026-09-06)

Decisión del usuario que **revierte la «decisión 9»** del plan (que solo Claude
heredara el MCP): cualquier consola de agentes que abra Atic tiene que poder
delegar sin que nadie configure nada.

### 11.1 El servidor `atic` en los cuatro caminos

`hub::atic_mcp(host)` devuelve el servidor en forma neutral
(`AticMcp { command, args }`) y cada adaptador lo traduce. El `--host` es el id
del backend: de ahí salen el presupuesto de espera del sidecar (`budget.rs`) y
la clave de `already_running`.

| Backend | Cómo entra |
|---|---|
| Claude Code | `--mcp-config` con el merge de siempre (`mcp_server_entry`) |
| Codex | `codex app-server -c mcp_servers.atic.command=… -c …args=… -c …tool_timeout_sec=330`. El valor de `-c` se parsea como TOML y JSON es un subconjunto válido para cadenas y arrays, así que `serde_json` escapa las barras de Windows |
| OpenCode, Cursor, Grok | `mcp_servers` del `session/new`: es parte del propio ACP (`NewSessionRequest.mcp_servers`), no un añadido nuestro |
| Antigravity | `agy mcp add atic …` una vez por arranque de Atic. Es el único que no acepta servidores por invocación y el único sitio donde Atic escribe en la config de otro CLI |

Los servidores del modal (`agent_mcp_servers`) siguen siendo solo de Claude:
vienen en su forma JSON y traducirlos a TOML/ACP es otra tarea. Los adaptadores
lo avisan con un `warn` de una sola vez.

### 11.2 Dos backends nuevos

- **Grok** (`acp::GROK`): `grok agent stdio` habla ACP v1 —responde
  `initialize` con `agentCapabilities` y `authMethods`— aunque su ayuda no diga
  «acp». Entra como constante, igual que OpenCode y Cursor.
- **Antigravity** (`antigravity.rs`): adaptador propio. NDJSON en las dos
  direcciones, pero **no** es el `stream-json` de Claude: entra
  `{"event":"user","message":{"role":"user","content":…}}` y salen `init`,
  `step_update` (con `step_index`, `state` `ACTIVE`/`DONE`/`ERROR`, `text_delta`
  **incremental** y `tool_info`) y `result` por turno. El proceso sobrevive a
  los turnos. `--print=` va pegado y vacío: separado, `agy` toma el flag
  siguiente como prompt.

Los dos suman su id a los enums del sidecar (`BackendId`, `BackendOrAuto`) y su
snippet en Ajustes → «Desde otras apps», que en su caso es un `mcp add` y no un
archivo de config.

### 11.3 Instalado no es lo mismo que con sesión

`AgentBackend::signed_in() -> Option<bool>` (`login.rs`) mira los archivos de
credenciales que cada CLI deja en el disco —los mismos que ya usaba el panel de
cupos— en vez de preguntarle al CLI: `login status` levanta un proceso y a veces
sale a la red, y esto se consulta al listar agentes.

Viaja en `BackendInfo` y en el `AgentInfo` del hub (`signedIn`), **sin apagar
`available`**: son dos problemas con dos arreglos distintos, y esconder al
segundo deja al usuario sin saber cuál tiene. Lo que sí hace el hub es negarse a
delegarle: `exigir_disponible` rechaza con `backend_unavailable` y el mensaje
«está instalado pero sin sesión iniciada», que es accionable. Un `None` —no
sabemos mirar ese backend— pasa: frenar por no saber sería peor.

### 11.4 Nombres de los hijos

`label` ya viajaba en `atic_delegate`/`atic_spawn` y se guardaba, pero la vista
lo tiraba. Ahora `bridge` lo hace único contra las sesiones vivas
(`etiqueta_unica`: el segundo «revisor» pasa a «revisor 2», porque quien delega
no puede saber qué hay abierto), lo devuelve en `agent_sessions` y el rail lo
muestra delante del backend.

### 11.5 Pendiente

- Ver la **cadena entera**: hoy cada sesión dice quién la pidió, pero la lista
  es plana. Agrupar por `root`/`parent` en árbol es lo que falta para «ver la
  interacción completa».
- El archivo de conversaciones no muestra `label`: `store::open` no lo recibe.
- **Antigravity**: sin cancelación (no acepta ningún evento de interrupción por
  stdin) y sin catálogo de modelos (`agy models` los lista, nadie los lee).
- Nada de esto se probó con Atic empaquetado: falta `pnpm tauri build`.
