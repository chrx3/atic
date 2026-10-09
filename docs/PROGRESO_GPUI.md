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
- **Instancia única:** mutex `Local\atic-pill-gpui`; la segunda pill le pide a
  la primera que abra Atic (salvo con `--from-atic`, que usa Tauri) y sale.
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
| Permisos de agentes en la bandeja (hooks en las consolas de la pill) | listo | `d766efd` |
| OpenCode y Cursor en «En curso» | listo | `86fb504` |
| Hub MCP y consolas de Tauri | para el final (decisión del usuario) | |
| `phone_sync` | para el final (decisión del usuario) | |

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
- `atic-pill.exe` lleva el ícono de Atic (`build.rs`).

## Agentes (decidido el 2026-10-07: lo justo y necesario)

Decisión del usuario: permisos en la bandeja y OpenCode/Cursor en «En
curso». El hub MCP (delegar entre agentes), las consolas de la ventana de
Tauri y el celular quedan para el final: no son lo principal.

Hecho:

- **Crate `atic-agents`** (`9647e2e`, `c17b05f`): hooks de las consolas
  (`hooks`), lo pendiente y las teclas que lo contestan (`prompts`), las
  sesiones de OpenCode (`opencode`) y Cursor (`cursor`) con un tipo neutro
  (`seen::Seen`). Tauri lo usa sin cambiar su comportamiento: sus consolas
  siguen con los archivos compartidos (`atic-agent-ping.jsonl`,
  `atic-codex-ping.jsonl`, `atic-kimi-*`).
- **Permisos en la bandeja de la pill** (`d766efd`): las consolas de Claude y
  Codex del espacio se lanzan con los hooks y una marca
  (`ATIC_CONSOLE_TOKEN`); sus hooks escriben en `atic-pill-<marca>.jsonl`. La
  pill lee esos archivos (`agent_prompts.rs`), muestra cada permiso como
  decisión y al permitir o negar teclea la respuesta en esa consola. Tauri no
  lee esos archivos, así que nada se contesta dos veces.
- **OpenCode y Cursor en «En curso»** (`86fb504`).

Queda:

- Las preguntas con opciones (`AskUserQuestion`) no salen en la bandeja: se
  contestan en la consola.
- Permitir «siempre» no está en la bandeja (solo Permitir / Negar).
- «Ver» sobre una fila de permiso intenta traer al frente una terminal externa;
  para las consolas de la pill debería traer el espacio y esa tarjeta.
- Sin hooks: OpenCode, Cursor y los demás CLIs lanzados desde la pill (sus
  permisos se contestan en la consola).
- Hub MCP, consolas de Tauri, `phone_sync` y si Tauri se cierra al cerrar su
  ventana: para el final.

## Sin WebView2 (2026-10-08)

Objetivo del usuario: que WebView2 no aparezca nunca. Hecho en el día:

| Paso | Commit |
| --- | --- |
| La bandeja abre Reuniones y Ajustes de la pill, nunca Tauri | `1722b68` |
| Ajustes: General (idioma, arranque, detectar reuniones, datos), Capturas, Lanzador | `72b86e6` |
| Ajustes: Atajos con editor que graba la combinación (pausa atajos, dictado y rueda) | `cfd2ef3` |
| Ajustes: Agentes, conexión MCP por CLI (`exe` y `mcp_install` a `atic-agents`) | `b17116b`, `f9b7c36` |
| Abrir `atic-desktop.exe` con la pill nativa abre la pill y sale (sin ventana ni WebView2); `--open` la muestra | `d05141b` |
| Tauri no crea la ventana de captura con la pill nativa (fallaba en WebView2) | `e32afdd` |
| Actualizador propio: mismo `latest.json`, instalador y firma minisign; verificado contra el release 0.4.44 publicado | `4599997` |
| Captura en pantallas con distinta escala (125 % + 100 %) | `8d38458` |
| Bienvenida la primera vez (Ajustes → General) | `545aa2b` |

Con esto, en el uso diario no corre WebView2: nada de la pill abre la ventana
de Tauri. `atic-desktop.exe` sigue en el instalador solo como puente (abre la
pill y sale).

Después, el mismo día:

| Paso | Commit |
| --- | --- |
| Instalador NSIS propio sin Tauri (`installer/atic.nsi`, `scripts/build-installer.ps1`); el release de Windows lo usa | `fdc3bf1` |
| Celular en la pill: vincular con QR, portapapeles compartido, música del PC, detener grabación | `cc587dc` |

Decisión del usuario: **los agentes no le interesan** (hub MCP, chat con
agentes); el celular sí. El hub MCP queda sin migrar: delegar entre agentes
no funciona sin Tauri.

En Windows ya no queda Tauri: el instalador trae `atic-pill.exe`,
`atic-mcp.exe` y `atic-unix.exe`. macOS sigue con Tauri.

Pendiente: ver «Ajustes del 2026-10-08, tarde», más abajo.

## Ajustes del 2026-10-08, tarde

Para probar: `powershell -File scripts/pill-dev.ps1` compila la pill de
desarrollo y la deja corriendo en lugar de la instalada (no toca el inicio con
Windows).

| Cambio | Commit |
| --- | --- |
| Arrastrar una captura del estante desde cualquier parte de la foto (los botones tapaban el centro) | `62ee701` |
| Sondeo del cursor lento en calma (50 ms) y rápido cerca de la pill | `42396e7` |
| Release con `lto = "fat"` y `codegen-units = 1`: 49,3 → 41,9 MB, 11 min de compilación | `2cf0124` |
| La mira cubre pantallas de resolución muy distinta (se ignora el tamaño sugerido de `WM_DPICHANGED`) | `6e95676` |
| Sin «respiración»: los ojos siguen al mouse y miran a un costado con el cursor quieto | `b063ff5` |
| El atajo de captura la cancela; la ayuda va en la pantalla del cursor; P muestra la pill también desde en vivo | `49784a3` |
| Favoritos del portapapeles al final (el filtro ☆ los muestra solos) | `4859684` |
| Barras de Apariencia en escala 0–100 % | `50b7762` |
| Agentes y sus permisos en el celular | `783ef16` |
| Ajustes, Reuniones y el espacio se traen al frente | `7a37d32` |
| Bandeja: clic izquierdo abre Ajustes; menú oscuro, con versión y atajo, sin «Traer pill» | `1f3ef28` |

Medido: CPU en reposo con GPU 0,82 % → 0,4 % (de 16 núcleos). Por software
(WARP, sin GPU) 5,6 %. El piso que queda es el hilo `VSyncProvider` de GPUI,
que invalida todas las ventanas en cada refresco; bajarlo exige una copia
propia de GPUI (decidido: después).

Pendiente:

1. Hecho después: la música del celular como una fuente más (`535956d`) y el
   lado, el tiempo del estante y el puntero en la captura (`1e63716`).
2. **Ajustes que la pill no usa todavía:** temas, sonidos de la UI, alertas
   del sistema.
3. **Atic Code** (el espacio de consolas): al usuario no le gusta cómo está;
   ver con él qué cambiar.
4. **Notificaciones de Android en el PC** con el celular vinculado. Idea del
   usuario; necesita la app del celular (no está en este equipo): leerlas con
   un `NotificationListenerService` y mandarlas por `atic-sync` como un evento
   nuevo, y que la pill las muestre.
5. Revisar el `RefCell already borrowed` que deja en el log cada arrastre
   (GPUI reentra durante el loop modal de `DoDragDrop`).
6. Sin probar en pantalla: la mira entre resoluciones distintas, los agentes
   en el celular, una actualización completa con el actualizador propio.

Decisiones tomadas sin el usuario:

- Retención en Ajustes con opciones fijas (7/30/90/365 días, 24 h/3/7/30 días
  para capturas, o «Siempre») en vez de un campo numérico.
- La versión de la pill sale de `tauri.conf.json` (`build.rs`): una sola
  fuente, el script de release no cambia.
- Instalar una actualización corre el instalador con `/P` y cierra la pill; el
  hook del instalador la vuelve a abrir.

## Grabar la pantalla (2026-10-09)

Mantener el atajo de Capturas (400 ms) pasa la mira a grabar; un toque la
detiene. Captura de Windows + Media Foundation (H.264), en `Videos\Atic`.
Maqueta aprobada por el usuario: https://claude.ai/artifact/7bALYtYdh58rDCjYothjxz

| Paso | Estado |
| --- | --- |
| Grabar ventana, zona o pantalla a MP4, sin audio | Hecho, `484f583` |
| Audio: micrófono y sistema en el MP4 (AAC, alineado al reloj del video: ~30 ms medidos) | Hecho |
| Controles en la pill: pausar, detener, descartar; cuenta regresiva; mic/audio en la mira | Pendiente |
| Al terminar, al estante en vez de abrir el reproductor | Pendiente |
| Mini editor: recortar, cortar tramos, exportar | Pendiente |
| Extras: subtítulos, seguir la ventana, cámara, GIF, zoom al cursor | Pendiente |
| macOS (ScreenCaptureKit) | Pendiente |

Pedidos del usuario para meter entre medio:

1. **Grabar la pill y la UI de Atic**, o que sea configurable. Hoy la ventana
   del overlay se excluye de la captura (`WDA_EXCLUDEFROMCAPTURE`) mientras
   graba.
2. **Captura en vivo con la pill**: al sacar un screenshot, poder estar en el
   modo en movimiento (en vivo) y que la pill salga en la foto. Hoy P solo
   alterna la pill sobre la foto congelada del arranque.

## Estado (2026-10-07, tarde)

- Tests: pill 327 ok; `atic-core` 56, `atic-clipboard` 8, `atic-calc` 23 y
  `atic-agents` 39 ok; Tauri 490 de 491. El que falla
  (`agents::console::tests::un_comando_simple_no_pasa_por_la_shell`) no es de
  esta rama: `console.rs` no cambió; desde Git Bash encuentra el `echo.exe`
  de Git y el comando pasa por `cmd /K`.
- `cargo check` de Tauri y de la pill sin avisos nuevos. La pill compila en
  release (subsistema GUI). No se probó nada en pantalla.
- **No verificado:** la build de macOS de Tauri. Se movió código con partes
  `cfg(target_os = "macos")` (contenido sensible del portapapeles, `fx`,
  `calc`) sin poder compilarlo aquí.
- Decidido: con `native_pill`, cerrar la ventana de Tauri termina el proceso
  salvo que queden consolas o sesiones de agentes (`cd8eaf8`). El exe de la
  pill lleva el ícono de Atic con `tauri-winres`, el mismo crate que usa
  Tauri (`24c9a81`).

## Para probar a mano

Compilar la pill del worktree y abrirla cerrando antes la que corre
(`cd prototypes/pill-gpui && CARGO_TARGET_DIR=... cargo build` con la pill vieja
cerrada, o con otro `CARGO_TARGET_DIR`). Ojo: con `autostart` encendido, la
pill reescribe la entrada `Atic` de `Run` para que apunte a **su** exe; si
pruebas con un exe de desarrollo, después vuelve a apuntarla al instalado.

1. **Datos:** aparece donde estaba (`home.txt` migrado), con tus colores,
   herramientas y apariencia. Existe `%APPDATA%\ciat\atic\data\pill\`.
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
9. **Permisos de agentes:** abrir Claude o Codex desde Agentes → Nuevo, pedirle
   algo que necesite permiso (un comando): aparece la decisión en la bandeja;
   Permitir lo deja seguir y Negar lo rechaza en la consola.
10. **OpenCode y Cursor:** con uno trabajando, aparece en «En curso».

## Trabas y decisiones

- **Cerrar la ventana de Tauri** termina el proceso con la pill nativa, salvo
  que tenga consolas o sesiones de agentes vivas: entonces solo se oculta.
- **Borrar y fijar en el portapapeles de la pill:** con `native_pill` van a
  `history.json` (`clipboard_owner`); sin ella, a su `local.json` como antes.
  Lo que ya estaba en `local.json` se sigue respetando.
- **«Mostrar / ocultar pill»** no está en el menú de la bandeja: ocultar el
  overlay con `ShowWindow` rompe `hide()`/`set_focus()` (ver memoria).
- La pill vieja que sigue corriendo escribe en `%LOCALAPPDATA%\atic-gpui`.
  Lo que cambies ahí después de la primera vez que arranque la nueva no se
  copia (la migración corre una sola vez).
- `phone_sync` y el portapapeles: con la pill como dueña, lo copiado ya no
  viaja al celular hasta mover `phone_sync`.
