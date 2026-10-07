# Plan: GPUI como pill principal y retiro de WebView2

Objetivo: que Atic funcione sin WebView2. Primero la pill y sus herramientas
(el overlay, que está siempre visible y redibujando), después la ventana
principal, y al final retirar Tauri. En Windows primero y en macOS después.

## Decisiones tomadas (2026-10-06)

- **macOS se mantiene.** Mientras GPUI no corra en Mac, la versión de Mac
  sigue siendo la de Tauri. Cada fase deja la capa de plataforma separada para
  no cerrarle la puerta a Mac. El usuario prueba en Mac cada opción.
- **Inglés es necesario**, no urgente. Desde la primera UI nueva, todo texto va
  por claves de traducción, para no tener que reescribirlo después.
- **Transcripción: Groq es lo recomendado y la opción por omisión.** Whisper
  local queda como opción, con baja prioridad (`atic-transcribe` ya lo tiene
  detrás de la feature `local`).

## Dónde estamos

- **Herramientas:** el prototipo (`prototypes/pill-gpui`, ~61 mil líneas,
  314 tests) ya tiene casi todas, varias mejores que en Tauri: Sistema, Ahora
  suena con letra, Reuniones (ventana propia), Pizarra, Color, Flip con
  tablero, uso y cupos, Apps, rueda y personalizar.
- **No es independiente.** Lee lo que escribe Atic-Tauri: el historial del
  portapapeles, `snippets.json` y `fx-rates.json`. Los permisos de agentes
  dependen de los hooks y del hub de Tauri. Sin Tauri corriendo, el
  portapapeles no captura y la bandeja no muestra permisos.
- **No tiene el cascarón de producto:** ajustes generales, onboarding, ícono
  de bandeja, instancia única, inicio automático, actualizador, instalador,
  log a archivo e idiomas.
- **Datos:** usa su propia carpeta (`%LOCALAPPDATA%\atic-gpui`), que además es
  su `CARGO_TARGET_DIR`.
- **Ventaja:** la lógica del backend de Tauri es en su mayoría Rust puro
  (`agents/` salvo `bridge` y `console`, `system_control/*`, `calc`, `notes`,
  `fx`, `mouse_bindings`), y los crates de `crates/` no dependen de Tauri. Se
  pueden mover y compartir sin reescribirlos.

## Principios

1. **Lógica en `crates/`, UI en cada app.** Lo que hoy vive en `src-tauri/src`
   y no toca ventanas se mueve a crates que usan las dos apps durante la
   transición. Nada se copia a mano (hoy hay copias: `quota/`, `drag.rs`,
   `flip_export.rs`, `calc.rs` por `#[path]`).
2. **Una sola carpeta de datos y una sola configuración:**
   `%APPDATA%\ciat\atic\data` (`AppDirs`) y `config.json` (`atic_core::Config`).
   Nada de archivos «locales» paralelos.
3. **Un solo dueño a la vez.** Nunca dos pills ni dos capturadores de
   portapapeles corriendo. El proceso que es dueño lo decide la instancia
   única.
4. **El hub HTTP local (127.0.0.1 con token) es el canal entre procesos**
   mientras convivan GPUI y la ventana principal de Tauri. No se inventa otro.
5. **Plataforma detrás de una interfaz.** `win.rs`, `glass.rs`, `paste.rs` y
   `capture.rs` pasan a tener su versión de Windows y un lugar claro para la
   de Mac.
6. **Textos traducibles desde el día uno** en toda UI nueva o que se toque.

## Fases

### Fase 0: base (Windows)

- Mover los datos de `atic-gpui` a `AppDirs`, con migración de los archivos
  propios: `local.json`, `snippets-local.json`, `launcher.json`,
  `colors.json`, `tools.json`, `usage.txt`, `media.txt`, `home.txt` y
  `captures/`. Separar el `CARGO_TARGET_DIR` de los datos.
- Log a archivo con rotación y captura de pánicos (reutilizar el esquema de
  `diagnostics.rs`). Quitar los `.expect()` de `main()`.
- Instancia única.
- Mecanismo de traducción (es/en). Ver la sección «Inglés».
- Capa de plataforma: definir la interfaz de overlay, vidrio, pegar, capturar
  y atajos, aunque al principio solo exista la de Windows.

**Listo cuando:** GPUI arranca sin variables de entorno, guarda todo en la
carpeta de Atic y deja log.

### Fase 1: GPUI es el proceso principal en Windows (sin overlay WebView2)

- **Portapapeles:** mover el watcher de `clipboard_history.rs` a un crate.
  GPUI pasa a ser dueño de `history.json`, respetando los marcadores de los
  gestores de contraseñas. También `paste_queue`.
- **Agentes:** mover a crates el hub MCP (`agents/hub`), los hooks de
  permisos, los watchers (Claude, Codex, OpenCode, Cursor), `mcp_install` y
  `presence`/`resume`. GPUI los ejecuta. La bandeja y el Mando muestran
  permisos reales. Los sidecars `atic-mcp` y `atic-unix` siguen igual.
- **Servicios de fondo:** `retention`, `fx` (tasas), `mouse_bindings`,
  detector de reuniones (GPUI ya tiene `meetings/detect.rs`: elegir uno) y
  `phone_sync`.
- **Textos:** una sola fuente (`snippets.json`). Eliminar
  `snippets-local.json` migrando lo que tenga.
- **Atajos globales:** los 12 de `config.json`, registrados de verdad
  (`RegisterHotKey`), incluidos Alt+Z y el dictado. Nada de sondear
  `GetAsyncKeyState`.
- **Ícono de bandeja** con las entradas de `tray.rs`, e **inicio automático**.
- **Tauri en modo «solo ventana principal»:** sin overlay, sin watchers ni
  hub. GPUI lo abre bajo demanda para la biblioteca, los ajustes y el
  workspace de agentes, y se comunican por el hub.
- **Instalador:** el mismo NSIS empaqueta el exe de GPUI como principal y el
  de Tauri como ventana secundaria.

**Listo cuando:** en Windows no hay WebView2 corriendo mientras la ventana
principal está cerrada, y nada del día a día depende de abrirla. Medir CPU y
RAM contra la versión actual: pill en reposo, con música y con agentes
trabajando.

### Fase 2: cascarón nativo y brechas de herramientas

- **Ajustes nativos:** las 12 secciones de `settingsSections.ts` sobre
  `config.json`, con editor de atajos (que pause los atajos mientras se graba
  uno nuevo).
- **Onboarding.**
- **Actualizador propio** compatible con el `latest.json` y la firma minisign
  actuales, para no romper las instalaciones que ya existen.
- **Multi-monitor:** el overlay en el monitor de la pill; Capturas y Flip en
  cualquier monitor (hoy solo el principal, `capture.rs:55`, `flip.rs:191`).
  Agregar captura de ventana tapada (`PrintWindow`).
- **Espacio de consolas:** guardar y restaurar entre ejecuciones, seleccionar
  y copiar con el mouse, IME, y agentes que sobrevivan al cierre (proceso
  aparte).
- **Flip:** lo que el README marca como faltante respecto de Atic.

**Listo cuando:** la ventana de Tauri solo se abre para la biblioteca de
reuniones y el workspace de agentes.

### Fase 3: inglés

- Completar la traducción de toda la UI de GPUI. Reutilizar las ~1.960 claves
  y traducciones de `apps/desktop/src/lib/core/i18n/{es,en}.ts` (generarlas, no
  copiarlas a mano).
- `ui_language` (system/es/en) igual que hoy.

### Fase 4: ventana principal en GPUI y retiro de Tauri en Windows

- Biblioteca de reuniones (la ventana de Reuniones de GPUI ya cubre la mayor
  parte): búsqueda global, importar, exportar y mail.
- Workspace de agentes: chat estructurado, árbol/minimapa, hosts SSH, MCP y
  cupos.
- Quitar Tauri del instalador de Windows.

### Fase 5: macOS

GPUI corre nativo en Mac (Metal). Hay que portar la capa de plataforma:

- **Overlay:** panel siempre encima, sin activarse, por espacios y por
  monitor. Convivir con el notch físico (ver la nota pendiente).
- **Vidrio:** `NSVisualEffectView`.
- **Pegar:** Cmd+V con CGEvent y permisos de accesibilidad.
- **Capturas:** `atic-capture` ya tiene CoreGraphics; audio del sistema con
  ScreenCaptureKit (ya está en `atic-audio`).
- **Medios:** MediaRemote vía `osascript` (ya resuelto en `media.rs` de Tauri).
- **Permisos TCC:** portar `permissions.rs` y `macos_notes.rs`.
- **Atajos:** su equivalente en Mac. LaunchAgent para el inicio automático.
- **Empaquetado:** DMG con la firma actual.

El usuario prueba en Mac cada herramienta a medida que se porta. Mientras
tanto, la versión de Mac es la de Tauri.

## Transcripción

- Groq queda por omisión en dictado, reuniones y subtítulos en vivo.
- Whisper local queda como opción (feature `local`). Se activa en GPUI cuando
  el resto esté estable: descarga de modelos bajo demanda, igual que en Tauri.

## Riesgos

- **GPUI 0.2** es joven y su API cambia. Fijar la versión y aislar lo que
  dependa de sus detalles internos.
- **Comportamientos de ventana de Windows**, como la pill que salía de
  «siempre visible» al cerrar Fotos (`Overlay::keep_topmost`). Probar en la
  máquina real después de cada cambio de ventanas.
- **Convivencia de dos procesos en la Fase 1:** evitar dos dueños del
  portapapeles, de los atajos o del hub.
- **Migración de datos** de usuarios ya instalados: siempre con respaldo y sin
  borrar el original hasta confirmar.
- **El exe no tiene firma Authenticode:** SmartScreen va a avisar igual que
  hoy.
