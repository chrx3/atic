# HANDOFF · atic — 2026-11-13

- Repo: `chrx3/atic` · Mac: `~/dev/atic` (con `~/dev/gpui-m3` al lado: la pill lo usa por ruta relativa)
- Base: `dev` (= `origin/dev`, `43b1ff3`, «docs: novedades de la 0.6.1»; tag `v0.6.1`). `main` va 4 commits atrás.
- WIP: `wip/migracion-mac` (1 commit `f557015` «Save work in progress before Mac migration», 20 archivos, +2251/−95). Nada de esto está probado en Mac.
- Rama aparte: `wip/migracion-mac-floating-ui-v2` (ver «Decisiones»).

## En qué estaba
Último chat de trabajo (2026-11-11, «Modo no molestar del notch») juntó varios frentes sin commit:
1. **Notch cerrado configurable** (`tab_size.rs`, nuevo): modo «No molestar» (el tab queda en los ojos de la marca; al pasar el cursor se abre como siempre), editor del tab cerrado (ancho/alto arrastrando esquinas, reordenar o sacar reloj/música/cara/avisos/onda a la fila «Fuera», botón «Listo»). Accesos en Ajustes y como acciones rápidas de la cara («No molestar», «Editar notch»). Con No molestar los avisos de agentes/celular no abren el notch: punto rojo parpadeando junto a los ojos y punto en Agentes/Celular de la tira.
2. **Atic Code `/clear`**: el chat se vacía y empieza conversación nueva en la misma pestaña (antes se duplicaba en la barra); el primer mensaje tras `/clear` le pone título a la sesión nueva.
3. **Subagentes tras reiniciar la pill**: la delegación guarda también la sesión de Claude de la madre (`parent_session`), se reabre la madre si falta y se re-enganchan las hijas (`relink_children`, `missing_parents`). Se recupera el nombre puesto a mano (`customTitle`).
4. **Una sesión de Claude = un proceso** (bug de historial partido): la pill engancha el chat al proceso vivo al reconectar (`twin_plan`, `live_twin`, `hand_over`) y `sidecar/agent.mjs` rechaza/cierra el gemelo.
5. **Delegación desde fuera**: `atic-mcp` (`hub_client.rs`) ya no corta a los 30 s cuando pide con el token de `hub.json`; plazo alineado con `hub.rs` (600+30 s, hay test que lo amarra).
6. **Vista previa**: HTML locales `aticfile.localhost` fallaban en Windows por la barra en `/C:/…` (`preview.rs`).
7. `scripts/pill-dev.ps1`: compila y copia el `agent.mjs` y `atic-mcp.exe` junto a la pill de dev (si quedaban viejos, los subagentes salían sin madre).

## Estado
- **Hecho (compiló y pasaron tests en Windows):** todo lo anterior. Según el chat: 1.152 tests de la pill, 353 de Atic Code, 11 del agente (`daemon.test.mjs`).
- **Sin verificar en la app real:**
  - Reinicio con subagente trabajando: debe seguir en el chip «Agentes» de la madre, despertarla al terminar y mantener el nombre (log: «subagente unido de nuevo a su conversación»).
  - Reinicio con una conversación trabajando y escribirle: un solo chat en la barra, historial completo.
  - «Listo» al editar con No molestar activo vuelve a los ojos; edición con el tab en un costado.
  - Pedir 2–3 subagentes en conversación nueva: sin error y en el chip.
- **Pregunta abierta:** con No molestar, ¿una llamada entrante del celular debe abrir el notch? Hoy no (solo punto rojo).
- **Ideas sin empezar** (chat 2026-10-30, usuario dijo «no aún»): aviso de contexto ~200k en Atic Code con relevo a chat nuevo (lo que más ahorra cuota); herramientas propias de Atic vía `createSdkMcpServer`; hooks del SDK (`PostToolUse`, `Stop`); timeout/ping en `Sidecar::call`; concatenar `appendSystemPrompt` en vez de reemplazarlo.

## Archivos clave
- `apps/atic/src/tab_size.rs` (nuevo, ~1.050 líneas) · `main.rs`, `face.rs`, `peeks.rs`, `pill_settings.rs`, `tray.rs` (bandeja de avisos/contadores, no el system tray), `screen_recording.rs` (hit-test del reloj)
- `apps/atic/src/code/{mod,chat,delegate,hub,preview,sidebar,view}.rs`
- `apps/atic/sidecar/agent.mjs` + `daemon.test.mjs` · `crates/atic-mcp/src/hub_client.rs`
- `apps/atic/assets/i18n/pill.{es,en}.json` (claves `pill.settings.quiet*`, `pill.settings.tabSize*`, `pill.tabSize.hint`, `pill.closed.*`, `pill.peeks.faceQuiet*/faceEditNotch*`)
- Datos: el tamaño/orden del notch se guarda en `<datos de Atic>/pill/tab.json`
- Docs: `AGENTS.md`, `docs/MACOS_GPUI.md`, `docs/DELEGACION_SUBAGENTES.md`

## Próximos pasos
1. En el Mac: `git fetch && git checkout dev && git pull`, luego revisar el wip (`git diff dev..origin/wip/migracion-mac`).
2. Compilar en Mac (`bash scripts/pill-dev-mac.sh`); antes `cargo build -p atic-mcp --release` (ver Notas macOS).
3. Probar el notch: No molestar, editar en arriba y en costado, y con el notch físico del MacBook.
4. Hacer las pruebas «sin verificar» de arriba (reinicio + subagentes, `/clear`, una sesión = un proceso).
5. Decidir lo de llamadas en No molestar; integrar a `dev` en commits por tema; agregar entradas a `apps/atic/assets/changelog.json` para la próxima versión.

## Decisiones y contexto
- No molestar no se edita: «Editar notch» siempre edita el modo normal; ancho/alto no afectan a No molestar.
- Regla nueva: una sesión de Claude Code = un solo proceso vivo. Si el gemelo trabaja, se rechaza con «esta conversación sigue trabajando en otra pestaña» (no se corta el turno).
- Los subagentes delegados **antes** de este cambio no traen `parent_session`: pueden seguir saliendo sueltos.
- El arreglo del agente (`agent.mjs`) solo entra cuando se retira el proceso viejo del agente (queda libre). El proceso del agente sobrevive a reinicios de la pill.
- Compilar la pill tarda 4–10 min y mucha RAM: juntar cambios, `-j 4`, no `cargo fmt` (reglas en `AGENTS.md`).
- **`wip/migracion-mac-floating-ui-v2`**: 20 commits «Pearl Flow» (2026-07-31/08-03) de un checkout separado (`atic-codex-floating-ui-v2`), base `f1a8da3` de julio, 53 archivos, con host **Tauri**. `dev` ya eliminó Tauri, así que es un experimento histórico: solo como referencia (`docs/PLAN_UI_FLOTANTE_V2.md`, `docs/QA_PEARL_FLOW.md`). No integrar sin decisión explícita.
- Un token de crates.io se pegó en un chat de gpui-m3: confirmar que se revocó.

## Notas para macOS
- **Windows-only en el wip:** `scripts/pill-dev.ps1` (función `Update-Helpers`). `scripts/pill-dev-mac.sh` ya recompila `agent.mjs`, pero **solo copia `atic-mcp` si ya está compilado** (`target/release/atic-mcp` o el de `/Applications/Atic.app`): correr `cargo build -p atic-mcp --release` o portar ese paso al script. Si no, vuelve el bug de subagentes sin madre.
- `preview.rs`: el arreglo `/C:/…` es de Windows; el test también cubre `/tmp/…`. Probar un HTML local en la vista previa del Mac.
- `tray.rs` y `screen_recording.rs` del wip son UI multiplataforma (no tocan APIs win32), igual hay que mirarlos en Mac.
- `tab_size.rs`: «fundido con el notch de la cámara, ahí manda él» → en MacBook con notch físico el orden propio no aplica; probar ese caso y monitores externos.
- Ya en `dev` y solo Windows: «Servidores locales olvidados» (`system/servers.rs`, `GetExtendedTcpTable`; en Mac hay stub `cfg(not(windows))`) — candidato a portar con `lsof`. «Liberar espacio» existe en ambos (`space.rs` Mac, `space_win.rs`).
- Ruta de Claude Code: el sondeo de modelos mira la fecha del ejecutable `claude` (en Windows `~/.local/bin/claude.exe`); verificar que en Mac detecte la actualización.
- Compilación en Mac: target `~/Library/Caches/atic-gpui-target`, `PILL_JOBS` (4 por omisión), firma «Atic Dev» desde `~/.tauri/atic-signing-identity` (copiar del Windows si hace falta; no subir a git).

## Cómo integrar
```bash
git checkout dev && git pull
git merge --no-ff origin/wip/migracion-mac   # o cherry-pick y partir en commits por tema
cd apps/atic && CARGO_TARGET_DIR=~/Library/Caches/atic-gpui-target cargo test -j 4 code::
pnpm --dir apps/atic/sidecar test             # node --test (daemon.test.mjs)
```
Commits sugeridos: notch (tab_size + i18n), `/clear`, subagentes tras reinicio + una sesión/un proceso, plazo de atic-mcp, vista previa, script de dev. Borrar `wip/migracion-mac` cuando esté en `dev`.
