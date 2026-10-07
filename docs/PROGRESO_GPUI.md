# Progreso: GPUI sin Tauri en Windows

Avance de `docs/PLAN_MIGRACION_GPUI.md` en la rama `gpui-standalone`
(worktree `%LOCALAPPDATA%\atic-wt`). Se actualiza al cerrar cada paso.

Validación:

- Workspace: `CARGO_TARGET_DIR=$LOCALAPPDATA/atic-ct cargo check -p <crate>`.
  El worktree necesita los sidecars en `apps/desktop/src-tauri/binaries/`
  (no se versionan): copiarlos del checkout principal.
- Pill: `cd prototypes/pill-gpui && CARGO_TARGET_DIR=$LOCALAPPDATA/atic-gpui cargo check`
  (no `cargo build`: el `pill-gpui.exe` de esa carpeta está corriendo).

## Fase 0: lista

| Paso | Estado | Commit |
| --- | --- | --- |
| Datos en `AppDirs` con migración desde `%LOCALAPPDATA%\atic-gpui` | listo | `64e2fbd` |
| Log a archivo con rotación y pánicos | listo | `a394c24` |
| Quitar los `.expect()` de `main()` | listo | `a394c24` |
| Instancia única | listo | `d335785` |
| Mecanismo de traducción es/en | listo | `d81f8f6` |
| Capa de plataforma | listo (falta `capture.rs`) | `83a7e3f` |

Detalles:

- **Datos:** `src/paths.rs`. Lo de la pill va en `%APPDATA%\ciat\atic\data\pill\`
  y las capturas en `data\captures\` (las mismas de Atic). La migración copia
  una sola vez lo que falte (marca `.migrated-from-atic-gpui`) y no borra el
  original.
- **Log:** `diagnostics.rs` pasó de Tauri a `atic-core` (feature
  `diagnostics`). Tauri escribe `atic.<fecha>.log` y la pill
  `pill.<fecha>.log` en `data\logs\`. Los avisos de GPUI entran al log; ya no
  hace falta `PILL_DEBUG` para verlos (sigue sirviendo para las trazas
  `debug()`).
- **Instancia única:** mutex `Local\atic-pill-gpui`; la segunda pill registra
  «ya hay una pill corriendo» y sale. Todavía no le avisa a la primera.
- **Traducción:** `src/i18n.rs`, `t("clave")` y `tf("clave", &[("var", &valor)])`.
  `assets/i18n/atic.{es,en}.json` se generan con
  `node prototypes/pill-gpui/scripts/i18n-export.mjs` (1.678 claves) y
  `pill.{es,en}.json` son a mano. Solo 2 textos de la pill pasaron a claves;
  el resto es la Fase 3.
- **Plataforma:** `src/platform/` con el contrato documentado y lo de Windows
  en `platform/win32/` (overlay, vidrio, pegar, atajos, arrastre, OCR,
  privacidad, apps abiertas).

## Fase 1

| Paso | Estado | Commit |
| --- | --- | --- |
| Portapapeles a un crate y la pill como dueña | listo | `ddf5da7`, `75f7134` |
| `paste_queue` («pegar después») | listo | `464ae5d` |
| Estados huérfanos, retención y limpieza de capturas | listo | `7d95678` |
| Detector de reuniones (la pill ya tenía uno) | listo: Tauri no lo corre con `native_pill` | `7d95678` |
| `fx` (tasas) y `calc` en `atic-calc` | listo | `cefe867` |
| Inicio automático | listo | `54b2227` |
| Ícono de bandeja | listo | `bb4618a` |
| Tauri bajo demanda (`--open`, sin bandeja) | parcial | `b326753` |
| Botones laterales del mouse (en la pill, sin tocar `mouse_bindings` de Tauri) | listo | `82d8a28` |
| Instalador (configuración y hooks, sin generar) | listo | `ad2de80` |
| `hub.json`: no borrar el de otro proceso | listo | `430f30b` |
| Hub MCP, hooks y watchers de agentes | **necesita decisión** (ver abajo) | |
| `phone_sync` | pendiente (depende de agentes) | |

Cómo queda el reparto con `native_pill: true`:

- **La pill hace:** historial del portapapeles (`history.json`), «pegar
  después», arreglar estados al arrancar, retención de grabaciones y capturas,
  detector de reuniones, tasas de cambio, inicio con Windows (la entrada `Atic`
  de `Run` apunta a su exe), ícono de bandeja.
- **Tauri deja de hacer lo mismo** (se comprueba al arrancar con
  `config_watch::native_pill()`): no corre el watcher del portapapeles, ni la
  limpieza al arrancar, ni el detector de reuniones, ni toca el inicio
  automático, ni crea su ícono. Si `native_pill` se enciende con Tauri abierto,
  su watcher deja de capturar en el acto; lo demás, al reiniciar.
- **Tauri sigue haciendo:** el hub MCP, hooks y watchers de agentes, sus
  consolas, la biblioteca, los Ajustes completos y `phone_sync`.

Crates nuevos: `atic-clipboard` (modelo, guardado, candado, contenido
sensible, watcher y cola de pegado), `atic-calc` (calculadora y `fx`) y los
módulos `atic_core::diagnostics` (feature) y `atic_core::housekeeping`.

## Instalador con la pill

`pnpm --dir apps/desktop tauri:build:pill` (= `tauri build --config
src-tauri/tauri.pill.conf.json`). No se generó ninguno.

- `pnpm pill:build` (`scripts/build-pill.ps1`) compila la pill en release en
  `target/pill-gpui` y la copia a `binaries/atic-pill-<triple>.exe`.
- `tauri.pill.conf.json` la suma a `externalBin` (queda como
  `atic-pill.exe` junto a `atic-desktop.exe`) y agrega
  `windows/pill-hooks.nsh`. Es aparte para que `pnpm dev` y el build normal
  no necesiten compilar la pill ni existan en macOS.
- Hooks: cierran la pill antes de instalar o desinstalar (si no, su exe no se
  puede reemplazar), apuntan el acceso del menú Inicio a `atic-pill.exe
  --native` y la abren al terminar. `--native` enciende `native_pill` en
  `config.json`. Al desinstalar se borra el valor `Atic` de `Run`.
- **Sin verificar:** el orden en que la plantilla NSIS de Tauri crea su acceso
  del menú Inicio respecto de `NSIS_HOOK_POSTINSTALL` (si lo crea después, el
  acceso vuelve a apuntar a Tauri). Revisar al generar el primero.
- El actualizador relanza `atic-desktop.exe`: con `native_pill`, Tauri abre
  `atic-pill.exe` si está junto a su exe (la instancia única evita una
  segunda pill).
- Falta el ícono embebido en `atic-pill.exe` (necesita un `build.rs` con un
  crate de recursos: hay que aprobar la dependencia).

## Agentes: plan y decisiones pendientes

Hay cuatro caminos de permisos (mapa completo en la conversación del
2026-10-07):

1. **Sesiones de chat que lanza Atic** (`bridge.rs`): Claude por stream-json,
   Codex por app-server, OpenCode/Cursor/Grok por ACP. La UI contesta con
   `agent_permission`. Es lo de la ventana de agentes de Tauri.
2. **Consolas PTY de Atic** con hooks inyectados (`ping.rs`: los hooks anexan
   JSON a `%TEMP%tic-*-ping.jsonl`; `watch_claude` los vacía y
   `console_prompts` los cruza con la consola). Hoy solo se contestan desde
   el celular, escribiendo teclas en el PTY.
3. **CLIs en una terminal externa**: solo presencia.
4. **Hub y `atic-mcp`**: un agente padre contesta el permiso de un hijo.

Frontera propuesta para un crate `atic-agents`: mover tal cual `model`,
`turns`, `hub/{api,graph,wait}`, `ping`, `console_prompts`,
`console_opencode`, `mcp_servers`, `mcp_install`, los adaptadores
(`claude_code`, `codex`, `acp`, …), los `tick` de `watch_*`, `resume`,
`presence` y `store`; `hub::server` sin el emit. Lo de Tauri (`AppHandle`,
eventos `agent-event`, `agent-presence`, `agents-permission-resolved`,
`console-output`) pasa a un trait `AgentsHost`. `console.rs` es lo más
acoplado.

**Decisiones que necesito antes de seguir:**

- **¿Quién es dueño de las consolas y las sesiones?** La pill ya tiene su
  espacio de consolas (`space/`). Si las consolas de agentes pasan a la pill,
  el camino 2 se mueve con ellas y Tauri deja de vaciar los pings (dos
  procesos leyendo los mismos `%TEMP%\*.jsonl` contestarían dos veces). Si
  siguen en Tauri, la pill solo muestra lo que Tauri le pase (habría que
  exponer presencia y eventos en el hub, que hoy no los tiene).
- **¿Tauri se cierra al cerrar su ventana?** Es lo que pide la Fase 1 (sin
  WebView2 con la ventana cerrada), pero hoy mataría las consolas y sesiones
  abiertas ahí.
- La pill duplica la vigilancia de JSONL de Claude y Codex
  (`pill-gpui/src/agents.rs`): al adoptar el crate hay que quedarse con una.

## Para probar a mano

Compilar la pill del worktree y abrirla cerrando antes la que corre
(`cd prototypes/pill-gpui && CARGO_TARGET_DIR=... cargo build` con la pill vieja
cerrada, o con otro `CARGO_TARGET_DIR`). Ojo: con `autostart` encendido, la
pill reescribe la entrada `Atic` de `Run` para que apunte a **su** exe; si
pruebas con un exe de desarrollo, después vuelve a apuntarla al instalado.

1. **Datos:** aparece donde estaba (`home.txt` migrado), con tus colores,
   herramientas y apariencia. Existe `%APPDATA%\ciattic\data\pill\`.
2. **Log:** `data\logs\pill.<fecha>.log` con «log iniciado».
3. **Instancia única:** abrir una segunda pill no crea otra.
4. **Idioma:** `ui_language: en` → «yesterday» en el portapapeles, y el menú
   de la bandeja en inglés (sin reiniciar).
5. **Portapapeles con Tauri cerrado:** copiar texto e imagen; aparecen en el
   panel. Copiar desde un gestor de contraseñas: no aparece.
6. **Pegar después:** dictar con el foco en la pill (sin otra app al frente):
   se pega al hacer clic en otra app.
7. **Bandeja:** clic izquierdo abre la ventana de Atic; el menú abre
   consolas, captura, trae la pill y sale.
8. **Tauri del worktree con `native_pill`:** sin ícono propio, sin capturar
   el portapapeles, y `atic-desktop.exe --open` muestra la ventana.

## Trabas y decisiones

- **Cerrar la ventana de Tauri no termina el proceso** (sigue oculta, con
  WebView2 en memoria). Hacer que salga al cerrar mataría las consolas de
  agentes abiertas en Tauri. Decidir cuando las consolas y el hub vivan en la
  pill.
- **Pegar desde el historial de la pill lo vuelve a subir** al tope: Tauri
  usaba `suppress_until` al pegar y la pill todavía no. Lo mismo para borrar y
  fijar: la pill los guarda en su `local.json`, no en `history.json`.
- **«Mostrar / ocultar pill»** no está en el menú de la bandeja: ocultar el
  overlay con `ShowWindow` rompe `hide()`/`set_focus()` (ver memoria).
- La pill vieja que sigue corriendo escribe en `%LOCALAPPDATA%tic-gpui`.
  Lo que cambies ahí después de la primera vez que arranque la nueva no se
  copia (la migración corre una sola vez).
- El exe de la pill todavía es de consola (sin `windows_subsystem`) y sin
  ícono embebido; se resuelve con el instalador.
- `phone_sync` y el portapapeles: con la pill como dueña, lo copiado ya no
  viaja al celular hasta mover `phone_sync`.
