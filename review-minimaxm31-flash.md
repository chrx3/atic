# Revisión de código — Atic

> **Documento de traspaso.** Escrito por Mavis (minimax-M3.1-flash) el 2026-09-27
> para que otro agente (Claude Code) tome los hallazgos y los arregle.
> **No es documentación del proyecto**: es una lista de trabajo. Bórralo cuando
> los puntos estén resueltos.

- **Alcance revisado:** backend Rust completo (`apps/desktop/src-tauri/` + `crates/`),
  frontend Svelte (`apps/desktop/src/`), `apps/web`, CI, higiene de secretos.
- **Método:** lectura de código únicamente. No se ejecutó `cargo`, `pnpm`, `vitest`
  ni ningún build. Nada fue modificado.
- **Commits al momento de la revisión:** `50dbd9b` (v0.4.41).
- **Working tree:** tenía cambios sin commitear al empezar, y aparecieron más durante la
  revisión (la sesión de desarrollo del dueño estaba abierta; esta revisión no
  modificó nada). Verificamos que los cambios de `clipboard_history.rs` caen en el rango
  1057-1091, por debajo de las líneas citadas acá, así que las referencias siguen
  vigentes. Si el árbol cambió más desde entonces, revalida las líneas antes de actuar.

## Cómo leer este documento

Cada hallazgo tiene un nivel de confianza:

- **VERIFICADO** — lo leyó directamente quien hizo la revisión, con línea y contexto.
- **SIN VERIFICAR** — reportado por un sub-revisor, con línea citada pero sin abrir.
  **Confírmalo antes de tocar código.**

Los hallazgos están ordenados por severidad, no por archivo.

## Resumen ejecutivo

| # | Severidad | Hallazgo | Confianza |
|---|---|---|---|
| 1 | **Alto** | La CI nunca corre clippy ni los tests de Rust | VERIFICADO |
| 2 | **Alto** | Token del hub en claro y escrito de forma no atómica | VERIFICADO |
| 3 | **Alto** | `Drop` mata el proceso hijo pero no lo cosecha | VERIFICADO |
| 4 | **Alto** | No se mata el árbol de procesos de los agentes CLI | VERIFICADO |
| 5 | **Medio** | Dos botones quedan inutilizables si el comando falla | VERIFICADO |
| 6 | **Medio** | `ping::drain()` pierde eventos de presencia en silencio | VERIFICADO |
| 7 | **Medio** | `PillSurface.svelte`: god-component de 8.392 líneas | VERIFICADO |
| 8 | **Bajo** | La hoja de la pill contradice su propia regla de animación | VERIFICADO |
| 9 | — | Varios de accesibilidad y frontend | SIN VERIFICAR |

**El punto 1 es el más importante del lote:** es el único que deja entrar bugs a `main`
sin avisar.

---

# 1. [ALTO · VERIFICADO] La CI nunca corre clippy ni los tests de Rust

**Archivo:** `.github/workflows/ci.yml`

```yaml
56	  rust:
57	    name: Rust (windows)
58	    if: ${{ inputs.windows }}        # <-- solo workflow_dispatch
...
76	  build:
77	    name: Build app (windows)
78	    if: ${{ inputs.windows }}
```

`inputs` solo existe en `workflow_dispatch` (líneas 10-16, con `windows.default: false`).
En un `push` a `main` o en un `pull_request` la expresión es vacía → los jobs se saltan.

**Consecuencia:** en cada push y cada PR corren únicamente `frontend`
(`pnpm verify`) y `rustfmt`. **clippy y los ~97 módulos de test de Rust no se ejecutan
nunca de forma automática.** El badge de CI del README puede estar verde sin que la
suite de Rust haya corrido jamás.

**Arreglo propuesto:** agregar un gatillo `schedule` (cron semanal) al job `rust` en
Windows, dejando el `workflow_dispatch` manual para `build`. Requiere deliberación del
dueño del repo: implica gastar minutos de runner.

---

# 2. [ALTO · VERIFICADO] Token del hub en claro y escrito de forma no atómica

**Archivo:** `apps/desktop/src-tauri/src/agents/hub/server.rs:69-75`

```rust
69	    std::fs::write(dirs.hub_path(), hub_json.to_string())
70	        .map_err(|e| format!("no se pudo escribir hub.json: {e}"))?;
71	    #[cfg(unix)]
72	    {
73	        use std::os::unix::fs::PermissionsExt;
74	        let _ = std::fs::set_permissions(dirs.hub_path(), std::fs::Permissions::from_mode(0o600));
75	    }
```

Dos problemas en el mismo bloque:

1. **Permisos solo en Unix.** El `set_permissions(0o600)` está dentro de `#[cfg(unix)]`,
   así que en Windows el archivo con el token de autenticación (64 hex, generado en
   `:54-58`) hereda los permisos de `%APPDATA%`. Cualquier proceso local del mismo
   usuario puede leerlo y ganar acceso a `/v1/spawn`, `/v1/prompt` y `/v1/delegate`.
2. **No es atómico.** `std::fs::write` trunca antes de escribir. Si Atic muere en el
   medio, el sidecar lee JSON corrupto.

**El proyecto ya tiene el helper correcto y lo usa en 5 lugares.** Solo se saltó aquí:

- `crates/core/src/fs_atomic.rs:35` (definición, con `sync_all` antes del rename)
- `apps/desktop/src-tauri/src/agents/mcp_install.rs:344`
- `apps/desktop/src-tauri/src/clipboard_history.rs:235` y `:262`
- `crates/core/src/config.rs:1052`
- `crates/core/src/summary.rs:29`
- `crates/core/src/transcript.rs:80`

**Arreglo:** usar `atic_core::fs_atomic::write_atomic` y aplicar permisos restrictivos
también en Windows (vía ACL o, más simple, guardando el archivo con permisos heredados
de un directorio restringido). Verificar de paso `hub_client.rs:106`, que ya maneja el
JSON inválido como `HubFallo::Ausente` — vale un test que fije ese comportamiento.

---

# 3. [ALTO · VERIFICADO] `Drop` mata el proceso hijo pero nunca lo cosecha

**Archivo:** `apps/desktop/src-tauri/src/agents/claude_code.rs`

```rust
1365	        let _ = self.child.kill();
1366	        let _ = self.child.wait();     // stop(): kill + wait  ✓
1367	    }
1368	}
1369	
1370	impl Drop for ClaudeSession {
1371	    fn drop(&mut self) {
1372	        self.stopping.store(true, Ordering::SeqCst);
1373	        self.stdin.take();
1374	        let _ = self.child.kill();      // drop(): kill sin wait  ✗
1375	    }
1376	}
```

La asimetría es evidente. En el camino de error —por ejemplo si `child.stdout.take()`
falla dentro de `start()` y el `?` retorna, droppeando el `Child` sin `wait()`— el handle
queda sin cosechar. En Windows cada `Child` sin cosechar retiene un HANDLE; en Unix deja
un zombie.

**Mismo patrón en:** `codex.rs:1494-1502`.

**Arreglo:** que `Drop` llame al mismo `stop()` (o al menos añada el `wait()`).

---

# 4. [ALTO · VERIFICADO] No se mata el árbol de procesos de los agentes CLI

**Archivos:** `claude_code.rs:1365`, `codex.rs:1489`, `antigravity.rs:557`

Todo el apagado es `child.kill()` sobre el proceso directo. Se confirmó por grep sobre
todo el backend que **no existe ninguna forma de matar el subárbol**: cero coincidencias
para `taskkill|process_group|CREATE_NEW_PROCESS_GROUP|killpg|job_object`.

Eso mata el shim de Node, pero los hijos que el CLI lanza (Bash, servidores de lenguaje,
MCP servers) sobreviven como huérfanos, con acceso al filesystem del proyecto. Cada
"cerrar sesión" puede dejar procesos acumulándose.

**Arreglo:** en Windows, `taskkill /PID <pid> /T /F`. En Unix, lanzar en su propio
process group (`process_group(0)` en `std::os::unix::process::CommandExt`) y matar el
grupo con `killpg`. `console.rs:895` sí mata la PTY, que es la raíz del subárbol de la
shell — puede servir de referencia.

---

# 5. [MEDIO · VERIFICADO] Dos botones quedan inutilizables si el comando falla

## 5a. `confirmForce` traga el error

**Archivo:** `apps/desktop/src/lib/features/system/SystemPanel.svelte:130-140`

```ts
async function confirmForce() {
  const target = forceTarget;
  if (!target) return;
  forceBusy = true;
  try {
    await system.forceApp(target.id);
    forceTarget = null;        // <-- solo en éxito
  } finally {
    forceBusy = false;
  }
}
```

No hay `catch`. Si el comando falla (proceso ya terminado, acceso denegado), la promesa
queda rechazada sin manejar desde `onConfirm={() => void confirmForce()}`, `forceTarget`
conserva su valor y el `{#if forceTarget}` mantiene el `ConfirmDialog` abierto. El botón
se rehabilita (`finally`), así que el usuario puede reintentar indefinidamente sin
ninguna señal de que falló.

**El patrón correcto ya está en el mismo archivo**, a 30 líneas de distancia:

- `closeApp` — `SystemPanel.svelte:96-107` → `catch` + `notices.push(...)`
- `confirmAsk` — `SystemPanel.svelte:119-128` → `catch` + `notices.push(...)`

Es una omisión local, no un estilo de la casa.

## 5b. `restart()` deja el botón muerto para siempre

**Archivo:** `apps/desktop/src/lib/features/permissions/PermissionsList.svelte:92-96`

```ts
async function restart() {
  if (restarting) return;
  restarting = true;
  await relaunch();        // sin catch, sin finally
}
```

Si `relaunch()` rechaza, `restarting` queda en `true` para siempre: el botón queda
deshabilitado sin explicación y no hay forma de reintentar.

**El mismo archivo usa `try/finally` correctamente** en `request()` (`:68-80`) y
`openSettings()` (`:85-89`).

**Arreglo para ambos:** añadir `catch` que muestre el error, siguiendo el patrón de los
vecinos. En total: 5 minutos de trabajo, dos botones que quedan inservibles.

---

# 6. [MEDIO · VERIFICADO] `ping::drain()` pierde eventos de presencia en silencio

**Archivo:** `apps/desktop/src-tauri/src/agents/ping.rs:172-185`

```rust
172	    for line in reader.lines() {
...
176	        consumed += line.len() as u64 + 1;
177	        if let Ok(v) = serde_json::from_str::<Value>(&line) {
178	            if let Some(ping) = classify_hook(&v) { apply_ping(ping); }
...
183	    if let Some(mut o) = offset {
184	        *o = consumed.min(len);
185	    }
```

`BufRead::lines()` entrega la última línea aunque **no** termine en `\n`, y esa línea
suele venir cortada: el hook escribe de golpe con `Add-Content` y Atic puede leer a
mitad de escritura. El código cuenta `+1` por un salto de línea que todavía no está, y
la línea parcial se descarta como JSON inválido.

El `.min(len)` de la línea 184 enmascara el error de offset, así que el síntoma real es
solo que **un evento de presencia se pierde en silencio**. El caso inverso también es
posible: si `consumed` queda 1 byte por debajo, el próximo drain reintenta desde dentro
de una línea y el JSON no parsea otra vez.

No es grave (es un chip de presencia y se auto-cura en el siguiente `Stop`), pero el
archivo nunca se trunca y el escritor es externo.

**Arreglo + test:** `drain()` es puro sobre `std::env::temp_dir()`, así que es testeable
inyectando la ruta. Hoy solo se testea `classify_hook`. Falta: archivo con líneas previas
ya consumidas, archivo truncado (rama `ping.rs:161-165`), y **archivo cuya última línea
no termina en `\n`** — que es exactamente el caso que produce el bug.

---

# 7. [MEDIO · VERIFICADO] `PillSurface.svelte` es un god-component

**Archivo:** `apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte`

Medido sobre el archivo: **8.392 líneas**, 41 `$effect`, 67 `$derived`, 32 `$state`.

La pill está **siempre montada**, así que esos efectos corren por toda la vida del
proceso, y cada dependencia mal declarada se paga en el componente más caro de la app.

**No propongo reescribirlo.** El código está bien organizado dentro: los listeners
pesados viven en un `onMount` (`:4534`) en vez de en `$effect`, así que no se
re-registran; y el efecto de animación (`:1163-1171`) cancela su `requestAnimationFrame`
en el teardown.

**Regla accionable:** cada `$effect` nuevo que toque `surface`/`at`/`dock` debería ir a
un hijo, no a la pill.

---

# 8. [BAJO · VERIFICADO] La hoja de la pill contradice su propia regla de animación

`PillSurface.svelte:7297` dice literalmente que las animaciones en bucle de la pill deben
salir de `--clock` (vía `holdMotionClock`) y **no** de `@keyframes`, porque una animación
CSS infinita redibuja la pill a la frecuencia del monitor.

En el mismo archivo, `@keyframes p-media-eq` está definido en `:6876` (renderizado en
`:5067`). Y en `AticMark.svelte:559` hay
`.am-dict rect { animation: am-dict-wave 0.9s steps(27) infinite; }`, con `AticMark`
montado dentro de la pill en `:4922, 5112, 5836, 5883, 5906, 5940, 5982`.

**Mitigante:** los tres están gateados a estados genuinos (`markState === "dictating"` en
`AticMark.svelte:507`, el bloque de cue de media), así que solo corren mientras el
usuario dicta o reproduce algo. Es la regresión que los commits de rendimiento
`102f5f1` y `c824f29` acaban de arreglar, pero acotada a dos ventanas de interacción.

Deuda relacionada: el campo de partículas de `ParticleWheel.svelte:239-244` es código
muerto en producción — la única instancia montada pasa `particles={false}`
(`PillSurface.svelte:5729`), así que el `runField` completo (~330 líneas en
`ParticleWheel.svelte:440-516`) nunca se ejecuta.

---

# 9. [SIN VERIFICAR] Varios, principalmente accesibilidad y frontend

**No abras estos a ciegas. Confirma la línea antes de tocar nada.**

| Hallazgo | Ubicación |
|---|---|
| `AgentsFloat` sería la única superficie flotante sin `role="dialog"` ni `aria-label` (sus 7 hermanas sí lo declaran) | `lib/surfaces/overlay/agents/AgentsFloat.svelte:1142-1160` |
| `clipboard.hydrate()` sería el único `hydrate` sin autoprotección, invocado con `void` pelado por 3 llamadores; `captures` y `snippets` apagan el rechazo en el mismo bloque | `lib/domain/clipboard.svelte.ts:32-38` · `toolPeek.svelte.ts:96-102` · `PillSurface.svelte:3820` · `ClipboardFloat.svelte:619` |
| `expect("turn")` depende de un invariante implícito de dos variantes del enum; si se agrega un tercer estado terminal pasa a ser panic en la ruta de render | `agents/claude_sessions.rs:255-263` |
| `apps/web` tendría un fork de `AticMark` con el bucle `requestAnimationFrame` anterior a los commits de perf, calibrado a 180 Hz | `apps/web/src/lib/atic/AticMark.svelte:210-258` |
| `is_available` spawnea `claude --version` sin timeout, mientras `discover.rs:425-431` ya maneja ese caso correctamente | `claude_code.rs:59-67` · `mcp_install.rs:92-98` |
| Las tres superficies flotantes duplicarían el ciclo de vida completo (18 funciones). Confirmado: tamaño equivalente (738 / 834 / 721 líneas), no el match exacto | `ClipboardFloat.svelte` · `SnippetsFloat.svelte` · `SystemFloat.svelte` |
| `summary_base_url` sin validación de esquema: la API key podría viajar en HTTP plano si el usuario pega una URL `http://` | `crates/core/src/config.rs:787-804` · `crates/summarize/src/openai_compat.rs:99-105` |

## Hallazgo descartado — NO lo "arregles"

Un sub-revisor reportó que el error del resumidor manda el body crudo del proveedor a
la UI. **Es falso**: `crates/summarize/src/error.rs:87` ya trunca con
`body.chars().take(500).collect()`. No hay nada que hacer acá.

---

# Lo que está bien hecho — no lo rompas

Estas decisiones son deliberadas y están bien documentadas. Son la razón por la que el
resto de los hallazgos son pocos.

1. **`fs_atomic::write_atomic` con `sync_all` antes del rename**
   (`crates/core/src/fs_atomic.rs:35-63`). El comentario explica el modo de fallo real
   (rename aplicado, contenido todavía en caché) y el `.tmp` va en el mismo directorio
   justamente para no cruzar volúmenes. Tiene 5 tests que cubren los casos que importan.

2. **Los secretos nunca tocan `config.json`** (`crates/core/src/secrets.rs`). Todo va al
   llavero del SO, y `validate_ssh_host_id` (`:141-150`) acota el id a
   `[A-Za-z0-9_-]{1,64}` antes de construir la clave del llavero — cierra la inyección de
   nombres de clave. La passphrase SSH viaja **solo por variable de entorno** al askpass,
   nunca en el script (`ssh.rs:255-258`, `:304-312`).

3. **El contrato TS↔Rust verificado por test, no por comentario**
   (`lib/surfaces/overlay/contract.test.ts`). Lee el `.rs` real con `?raw` y compara las
   constantes, y falla ruidosamente también si Rust *renombra* la constante.

4. **El test de escaping que decodifica en vez de assertar la forma**
   (`codex.rs:1524-1538`). No hace `assert!(args.contains("escapado"))`, sino que
   decodifica el TOML de vuelta y comprueba que el valor sigue siendo la ruta original.
   Es el test correcto para ese caso.

5. **`MutexExt::lock_or_recover` con justificación escrita** (`crates/core/src/sync.rs:6-41`).
   La doc explica por qué ignorar el veneno del mutex es mejor que propagar el pánico, y
   separa explícitamente esa decisión de la de escritura atómica. Es la clase de comentario
   que evita que alguien lo "arregle" al revés dentro de seis meses.

6. **Consentimiento fail-closed antes de grabar** (`state.rs:127-151`). El preflight de
   Bluetooth/Hands-Free **aborta** pidiendo confirmación, en vez de solo avisar después.

7. **Nunca `StrictHostKeyChecking=no`** (`ssh.rs:133`). El error de host desconocido
   manda al usuario a una terminal real en vez de desactivar la verificación.

8. **El watcher de portapapeles consulta la config en cada vuelta**
   (`clipboard_history.rs:495-497`), así que apagar el historial surte efecto en el acto,
   y consulta `clipboard_is_sensitive()` **antes** de leer texto o imagen (`:518`).

9. **Higiene de timers y listeners: impecable.** No se encontró un solo
   `setInterval` / `requestAnimationFrame` / `addEventListener` global sin limpiar en todo
   el frontend. En un proyecto que lleva cinco commits seguidos peleando con esto, es
   ideología ya internalizada.

10. **Higiene de secretos local:** `keys/` está en `.gitignore:27` con 0 archivos
    rastreados por git, y el `README` lo documenta.

---

# Gaps de tests

La base es genuinamente buena: **97 módulos Rust con tests, 72 archivos `*.test.ts`,
3 archivos de test de integración.** Los tests formulan invariantes en vez de copiar la
implementación. Estos son los huecos que importan, en orden de valor:

1. **`lib/motion.ts` no tiene ningún test.** Es el módulo que los commits de rendimiento
   `102f5f1` y `c824f29` acaban de introducir, del que dependen *todas* las superficies
   animadas (`holdMotionClock`, `clockWhilePresent`, `prefersReducedMotion`, `ms`,
   `tabPanel`). Que un `clockHold` se desincronice no falla hoy por suerte.
   **Es el gap más grande del frontend.**

2. **Ciclo de vida de `CaptureSession` / parada de audio** (`crates/audio/src/lib.rs`).
   Todo el path de error de `control_loop` (`:996-1046`) está sin test: mic falla +
   system falla, mic falla + system ok, y sobre todo que `TrackWriter::finish` (`:890-897`)
   siempre finalice el WAV. `hound` deja un header inválido si no se finaliza, y un WAV
   truncado se manifestaría como transcripción corrupta mucho después, sin causa visible.
   **Es el hueco más caro de la lista.**

3. **`ping::drain()` con offset acumulado** — ver hallazgo 6.

4. **Integración de `ClaudeSession`/`CodexSession` con un binario fake.** No hay ningún
   test que spanee algo que se comporte como el CLI (emita N líneas JSON, espere, muera) y
   verifique que `stop()` no cuelgue, que `Drop` no deje el child sin `wait()`, y que un
   stdout cerrado sin `stopping` produzca `AgentDelta::Failed`
   (`claude_code.rs:195-201`) — que es exactamente lo que evita que la UI quede esperando
   para siempre.

5. **Cero tests de componente y cero de `apps/web`.** Los 72 archivos TS son todos de
   lógica pura, todos bajo `apps/desktop/src`. Habría atrapado automáticamente los
   hallazgos 5a y 5b.

6. **Cero tests de teclado/atajos.** Nada verifica el orden de los ~9 handlers `keydown`
   en captura que se registran en la ventana overlay (`PillSurface`, `AgentsBoard`,
   `ConsolePanel`, los cuatro floats, `Modal`), ni que el `Esc` llegue al dismiss correcto.
   `lib/core/hotkeys.test.ts` cubre solo el predicado puro, no el enrutado.

7. **`AticMark.svelte` sin test de la lógica de *parking* / frame-gap** — justo la
   regresión que evita el bucle a 180 Hz. `motion.test.ts` cubre la geometría
   (`rigidShift` / `clusterParts` / `unionAabb`), nada del reloj.

---

# Orden de trabajo sugerido

1. **La CI** (hallazgo 1) — es lo único que deja entrar bugs a `main` sin avisar.
2. **`confirmForce` + `restart()`** (hallazgo 5) — 5 minutos, dos botones inservibles.
3. **`hub.json`** (hallazgo 2) — cambiar a `write_atomic` + permisos en Windows. A 5
   líneas, y el patrón ya existe en el repo.
4. **`Drop` con `wait()`** (hallazgo 3) — una línea por backend de agente.
5. **Test de `motion.ts`** (gap 1) — antes de que alguien más toque las animaciones.

---

# Notas de trabajo para el agente que tome esto

- Lee `AGENTS.md` antes de actuar. El proyecto tiene reglas estrictas de iteración.
- **Hay una sesión de desarrollo abierta.** No la reinicies ni levantes otra instancia.
  Para cambios de Rust, deja que `tauri dev` recompile solo; para el sidecar MCP,
  `pnpm --dir apps/desktop mcp:build`.
- **No corras `pnpm verify`, `pnpm verify:all` ni la suite completa** entre cada ajuste.
  Valida de forma dirigida: `cargo check --locked -p <crate>`,
  `cargo test --locked -p <crate> <filtro>`, o
  `pnpm --dir apps/desktop exec vitest run <ruta-del-test>`.
- Los hallazgos 1-4 son cambios chicos y aislados. **Sugerencia: tómalos uno por uno y
  confirma cada uno con un test de regresión** donde tenga sentido (sobre todo el 3 y el
  6). Evita meterlos todos en un solo commit: son reviewables por separado.
- La revisión **no cubrió** `color_picker.rs` (1.452 líneas), `overlay.rs`,
  `capture_session.rs` ni `launcher.rs` con la misma profundidad — son los de mayor
  volumen con menor densidad de riesgo aparente, pero `color_picker` tiene acceso a
  píxeles de pantalla. Si quieres una segunda vuelta ahí, es el siguiente paso natural.
