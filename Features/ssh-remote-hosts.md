# Hosts SSH para agentes remotos

**Estado:** `mvp` (CRUD hosts + test + Claude Code remoto vía SSH)

## Resumen

Guardar hosts (`user@host` + autenticación), probar la conexión y abrir una
sesión de agente cuyo proceso local sea `ssh … -- claude|opencode|…`, con el
CLI corriendo **en la máquina remota**. El chat de Atic sigue igual: el puente
stdio no cambia de protocolo, solo de transporte.

No es terminal embebida, ni escritorio remoto, ni clipboard remoto como camino
principal.

## Objetivo / no-objetivo

### MVP (sí)

- CRUD de hosts en Ajustes + acceso rápido desde la consola de agentes.
- Elegir **Local** vs **Remoto (host)** al arrancar sesión.
- Test de conexión SSH desde la UI.
- Arrancar **un** backend remoto: **Claude Code** (`claude -p … stream-json`).
- Secretos solo en keyring; frontend nunca ve claves privadas ni passphrases.
- Preferir `ssh-agent`; opcionalmente ruta a identity file + passphrase en
  keyring.

### Fuera del MVP (no)

- Terminal interactiva, RDP/VNC, sync de clipboard como producto.
- Explorar FS remoto en el `FolderBrowser` (cwd se escribe a mano o se usa el
  default del host).
- Reconnect / resume de sesión CLI tras caída de red.
- ControlMaster / multiplexado SSH.
- Daemon Atic en el host remoto.
- Codex / OpenCode / Cursor remotos (misma idea, después).
- Subir o generar claves privadas desde Atic.

## Cómo se usa

1. En **Ajustes → Agentes / Hosts SSH**: alta de host (etiqueta, `user`, host,
   puerto, auth).
2. **Probar conexión** → ok / error legible (timeout, host key, auth).
3. En la consola de agentes (`AgentsDemo`): chip o selector **Local | Remoto**.
4. Si Remoto: elegir host; el chip de carpeta pide un **cwd remoto** (texto;
   default del host si existe).
5. Enviar mensaje → Rust spawnea `ssh … -- claude -p --input-format stream-json
   …` con cwd remoto; la UI recibe los mismos `agent-event`.

Indicadores:

- Chip de destino: `Local` o `host-label` (no solo el path).
- Estado del test: punto verde/ámbar/rojo en la ficha del host (último resultado
  cacheado en memoria o timestamp en config, sin secretos).
- Mientras corre: mismo spinner/turno que hoy; si SSH muere, `Failed` con
  mensaje de transporte.

## Modelo de datos

### Registro de host (no secreto)

Persistir en **`config.json`** como lista en `Config` (mismo patrón que el
resto de preferencias). No va a SQLite: no es historial de chat.

```text
SshHost {
  id: String            // uuid estable
  label: String         // "prod-api"
  user: String
  host: String          // hostname o IP
  port: u16             // default 22
  auth: "agent" | "key" | "password"   // MVP: agent + key; password opcional/fase 1.1
  identityFile: Option<String>         // ruta local al .pem/.pub key file; NUNCA el contenido
  defaultRemoteCwd: Option<String>     // p.ej. "/home/deploy/app"
  remoteAgentBin: Option<String>       // override; default "claude"
  // lastTestOk / lastTestAt: opcionales, no secretos
}
```

Campos en hilos (`agent_threads` / `StoredThread`):

- Hoy: `cwd: String`.
- MVP: sumar `remote_host_id: Option<String>` (nullable) para reabrir con el
  mismo destino. El `cwd` en remoto es path POSIX en el host; no se valida como
  path local.

### Secretos (keyring)

Extender el patrón de [`crates/core/src/secrets.rs`](../crates/core/src/secrets.rs):

- Servicio sigue siendo `com.ciat.atic`.
- Claves **parametrizadas por host id** (el enum fijo actual no escala):
  - `ssh_host_{id}_passphrase` — passphrase de la identity file
  - `ssh_host_{id}_password` — solo si se habilita auth por password
- API hacia UI (igual que SMTP/API keys): `has_*` booleano, `set`/`delete`;
  **nunca** `get` hacia el frontend.

## Seguridad

| Regla | Detalle |
|---|---|
| Preferir ssh-agent | Auth default: agent del SO (`SSH_AUTH_SOCK` / Pageant / OpenSSH Agent). Sin secretos en Atic. |
| Keyring solo auxiliares | Passphrase o password; no private keys. |
| Identity file = path | La UI elige archivo; Rust pasa `-i path` a `ssh`. El contenido no cruza IPC. |
| known_hosts | Usar el de OpenSSH del usuario. No `StrictHostKeyChecking=no`. Primer contacto: mensaje claro (fingerprint / “acepta en terminal una vez” o `accept-new` documentado). |
| Frontend ciego | Comandos Tauri no devuelven material secreto; listados de hosts omiten secretos. |
| BatchMode en test | `ssh -o BatchMode=yes …` para no colgar UI pidiendo password interactivo; si falta auth, error accionable. |
| Windows | Requiere cliente OpenSSH (`ssh` en PATH). Documentar dependencia. |

## Arquitectura

### Hoy (local)

```text
UI → agent_start(backend, StartRequest{ cwd, … })
  → AgentBackend::start(StartOptions)
  → Command::new("claude") + stdin/stdout piped
  → deltas → store + emit("agent-event")
```

Puntos clave ya existentes:

- [`StartOptions`](../apps/desktop/src-tauri/src/agents/mod.rs) / [`StartRequest`](../apps/desktop/src-tauri/src/agents/bridge.rs)
- Spawn Claude: [`claude_code.rs`](../apps/desktop/src-tauri/src/agents/claude_code.rs) (`Command` + hilos stdout/stderr)
- OpenCode/Cursor: [`acp.rs`](../apps/desktop/src-tauri/src/agents/acp.rs) (ACP vía `async-process`; más frágil de envolver)
- Cwd UI: chip + [`FolderBrowser.svelte`](../apps/desktop/src/lib/features/agents/FolderBrowser.svelte) (FS **local** vía [`fs_browse.rs`](../apps/desktop/src-tauri/src/agents/fs_browse.rs))
- Persistencia hilos: [`store.rs`](../apps/desktop/src-tauri/src/agents/store.rs)

### Cambio: `RemoteTarget` en el arranque

```text
StartRequest / StartOptions +=
  remote_host_id: Option<String>   // None = local (comportamiento actual)
```

Resolución **solo en Rust** (bridge o módulo `agents/ssh.rs`):

1. Cargar host desde config por id.
2. Armar argv de `ssh`: puerto, `-i`, `-o IdentitiesOnly=yes` si key, BatchMode
   según contexto, `user@host`.
3. Comando remoto:  
   `bash -lc 'cd <cwd_escaped> && exec <bin> <args…>'`  
   (o equivalente seguro sin shell si el cwd es `.`).
4. Proceso local: `Command::new("ssh")` con **los mismos** `stdin`/`stdout`/`stderr`
   piped que hoy usa Claude. El adaptador de protocolo no cambia.

```text
UI ──agent_start──► bridge
                      │
                      ├─ resolve SshHost + keyring (passphrase via SSH_ASKPASS helper si hace falta)
                      │
                      └─ ClaudeCode::start
                            │
                            ├─ local:  Command::new("claude") …
                            └─ remoto: Command::new("ssh") … -- bash -lc 'cd … && exec claude …'
```

`is_available()` local sigue mirando el PATH de la máquina del usuario. En
remoto, el probe de “¿hay `claude`?” es el test de conexión o un
`ssh … -- command -v claude` opcional (fase temprana del checklist).

### Por qué Claude Code primero (no OpenCode)

| | Claude Code | OpenCode (ACP) |
|---|---|---|
| Spawn | `std::process::Command` sync, args planos | `async-process` + handshake JSON-RPC en [`acp.rs`](../apps/desktop/src-tauri/src/agents/acp.rs) |
| UI actual | Camino principal en [`AgentsDemo.svelte`](../apps/desktop/src/lib/features/agents/AgentsDemo.svelte) | Cableado pero secundario en UX |
| Remoto | `ssh -- claude -p …` reutiliza el traductor stream-json tal cual | Hay que enrutar el launcher ACP por SSH y cuidar el reactor |
| `exe::launcher` | Irrelevante en el host (Linux/mac remoto típico) | Shims Windows solo aplican al lado local |

**Decisión MVP:** solo `claude-code` remoto. OpenCode/Cursor/Codex: misma
`RemoteTarget` + wrapper SSH cuando el spawn local ya esté abstraído.

### Código a tocar (previsto)

| Área | Archivos |
|---|---|
| Config hosts | [`crates/core/src/config.rs`](../crates/core/src/config.rs), tipos TS [`AppConfig`](../apps/desktop/src/lib/core/types.ts) |
| Keyring | [`crates/core/src/secrets.rs`](../crates/core/src/secrets.rs) (+ helper por host id); IPC tipo [`mail.rs`](../apps/desktop/src-tauri/src/mail.rs) |
| SSH + test | **nuevo** `apps/desktop/src-tauri/src/agents/ssh.rs` |
| Arranque | [`bridge.rs`](../apps/desktop/src-tauri/src/agents/bridge.rs) (`StartRequest`), [`mod.rs`](../apps/desktop/src-tauri/src/agents/mod.rs) (`StartOptions`) |
| Spawn Claude | [`claude_code.rs`](../apps/desktop/src-tauri/src/agents/claude_code.rs) |
| Hilos | [`store.rs`](../apps/desktop/src-tauri/src/agents/store.rs) + migración SQLite si hace falta `remote_host_id` |
| Ajustes | sección en Settings (hosts CRUD + test + secret flags) |
| Consola | [`AgentsDemo.svelte`](../apps/desktop/src/lib/features/agents/AgentsDemo.svelte): Local/Remoto, cwd remoto, pasar `remoteHostId` al `agent_start` |
| IPC | `apps/desktop/src/lib/ipc/…` — `ssh_hosts_*` / reutilizar `set_config` + comandos `ssh_test_host`, `ssh_set_host_secret` |

Abstracción deseable (aunque el primer PR pueda ser mínimo dentro de
`claude_code.rs`): función única `spawn_cli(program, args, cwd, remote)` para
no duplicar flags al sumar OpenCode después.

## Checklist MVP (PRs chicos, ordenados)

1. [x] **Modelo + persistencia** — `SshHost` en `Config`; serde/default; tipos TS.
2. [x] **Keyring por host** — set/has/delete passphrase (y password en API); sin leak al frontend.
3. [x] **Módulo SSH + test** — `agents/ssh.rs`; `ssh_test_host` con BatchMode + timeout; errores mapeados.
4. [x] **`RemoteTarget` en start** — `remote_host_id` en `StartRequest` / `remote` en `StartOptions`.
5. [x] **Spawn remoto Claude** — wrap `ssh -- bash -lc 'cd … && exec claude …'` (lab manual pendiente).
6. [x] **Ajustes UI** — sección Agentes: lista/alta/edición/borrar + Probar + auth agent/key.
7. [x] **Consola agentes** — Local/Remoto; cwd remoto (prompt); indicador; `remoteHostId`; columna en hilo.
8. [x] **Pulido MVP** — mensajes de error; OpenSSH documentado en UI; FolderBrowser local intacto.

Cada ítem ≈ un PR revisable. No mezclar FS remoto ni ControlMaster en estos PRs.

## Fases posteriores

| Fase | Qué |
|---|---|
| 1.1 | Auth password vía askpass seguro; o documentar “solo agent/key”. |
| 2 | `fs_browse` remoto (`ssh … ls`) + `FolderBrowser` en modo remoto. |
| 3 | OpenCode/Cursor/Codex remotos reutilizando el spawn wrapper. |
| 4 | Reconnect / reattach; aviso de sesión huérfana en el CLI remoto. |
| 5 | `ControlMaster` / `ControlPath` para multiplexar test + sesión. |
| 6 | Opción daemon/agente Atic en el host (si el wrap SSH no alcanza). |
| 7 | Resume Claude remoto (`--resume` + índice `~/.claude/projects` vía SSH). |

## Riesgos y supuestos

- **Supuesto:** el usuario tiene OpenSSH cliente en PATH (Windows 10+ / macOS).
- **Supuesto:** en el host remoto ya está instalado y logueado `claude` (Atic no autentica el CLI; igual que en local).
- **Host keys:** el primer contacto puede fallar en BatchMode hasta que exista entrada en `known_hosts`.
- **Passphrase / askpass:** en GUI sin TTY, hace falta helper `SSH_ASKPASS` (o agent precargado). Riesgo de fricción en Windows.
- **Citas / escaping:** el `bash -lc 'cd …'` mal escapado es bug de seguridad/usabilidad; centralizar escaping.
- **Latencia / cortes:** un SSH caído mata el Child igual que un CLI local; la UI debe mostrar `Failed`, no colgarse.
- **`claude_sessions` / transcript local:** listar `~/.claude/projects` en el PC **no** aplica al remoto en MVP; deshabilitar picker de sesiones CLI si hay `remote_host_id`.
- **Skills / media locales:** adjuntos y skills descubiertos en disco local pueden no existir en el remoto; MVP: no prometer paridad.
- **Paths Windows→Linux:** el cwd remoto no debe pasar por `canonicalize` local ni por `FolderBrowser` actual.

## Plan de prueba (manual)

Entorno: PC con `ssh` y un host de lab (VM o VPS) con `claude` en PATH y login CLI hecho.

1. Alta de host con auth **agent** (clave ya en ssh-agent) → guardar → aparece en lista.
2. **Probar conexión** → éxito; con host apagado → timeout/error claro.
3. Host con **identity file** + passphrase en keyring → test ok; reiniciar Atic → `has_passphrase` true, valor no visible.
4. Consola: Local + cwd local → chat Claude igual que hoy (regresión).
5. Consola: Remoto + host + cwd `/tmp` o repo → un mensaje → respuesta stream; tools/permisos si aplica.
6. Cortar red a mitad de turno → UI muestra fallo; se puede abrir sesión nueva.
7. Reabrir hilo guardado con `remote_host_id` → propone el mismo host (aunque el proceso no esté vivo).
8. Confirmar en DevTools/IPC que ningún payload de host incluye passphrase ni PEM.
9. (Negativo) Host sin `claude` → error de arranque entendible (“no se encontró claude en el remoto”).

## Pendiente / siguiente

- [x] Checklist MVP 1→8 en código (lab e2e manual con VM aún por confirmar)
- [x] Actualizar [agentes.md](agentes.md)
- [ ] Auth password vía askpass (fase 1.1); askpass de passphrase es mínimo (env + script temp)
- [ ] Probar e2e en lab: agent + identity file + corte de red

## Relacionado

- [agentes.md](agentes.md)
- [`docs/PLAN_AGENTES.md`](../docs/PLAN_AGENTES.md)
- [ajustes-onboarding.md](ajustes-onboarding.md) — patrón secretos / Settings
- Decisión de producto: agente en VM vía SSH (no terminal-only, no RDP, no clipboard remoto como primario)
