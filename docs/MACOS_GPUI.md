# Atic en macOS con GPUI

Guía para quien llega al repo con un Mac y quiere trabajar en la app actual.
Escrita el 2026-10-15.

## Estado hoy

- **La app es la pill GPUI** (`prototypes/pill-gpui`), en Rust puro. Todo
  trabajo nuevo va ahí. En Windows es lo que instala el instalador.
- **Tauri (`apps/desktop`) es legado.** No se le agregan funciones. La Atic
  de Mac ya sale de la pill (ver
  [Publicar en macOS](#publicar-en-macos-y-pasar-desde-tauri)); la carpeta
  queda por la versión, el ícono y la clave pública del updater.
- **La pill GPUI corre completa en macOS** desde el 2026-10-15: toda la capa
  `src/platform/macos/` tiene implementación nativa (ver
  [Avance del port](#avance-del-port)). Falta probar a fondo cada herramienta
  en uso real.
- **Los crates compartidos ya tienen macOS** (`crates/audio` con
  ScreenCaptureKit, `crates/capture` con CoreGraphics, `crates/clipboard`).

Ramas: se trabaja en `main` (por defecto en GitHub) y `dev`. Ambas tienen la
pill GPUI.

## 1. Preparar el Mac

```bash
xcode-select --install                      # compilador y SDK de macOS
curl https://sh.rustup.rs -sSf | sh         # Rust estable
```

No hace falta el compilador de Metal: Xcode 26 ya no lo trae y en macOS la
pill activa `runtime_shaders` de GPUI, que compila los shaders al arrancar.

## 2. Clonar los dos repos lado a lado

La pill usa `gpui-m3` (componentes Material 3) por ruta relativa
(`../../../gpui-m3` desde `prototypes/pill-gpui`), así que los dos repos van en
la misma carpeta:

```bash
mkdir atic-dev && cd atic-dev
git clone https://github.com/chrx3/atic.git
git clone https://github.com/chrx3/gpui-m3.git
```

Queda `atic-dev/atic` y `atic-dev/gpui-m3`.

## 3. Compilar y abrir la pill

```bash
bash scripts/pill-dev-mac.sh            # compila (debug), arma «Atic Pill.app», la firma y la abre
bash scripts/pill-dev-mac.sh --release  # optimizada
bash scripts/pill-dev-mac.sh --install  # además la copia a /Applications
```

El script compila en `~/Library/Caches/atic-gpui-target` (no choca con
`tauri dev`), empaqueta el agente de Atic Code (`sidecar/dist/agent.mjs`),
arma la `.app` con `prototypes/pill-gpui/macos/Info.plist` y la firma con la
identidad de `~/.tauri/atic-signing-identity` («Atic Dev», ver
[`MACOS.md`](MACOS.md)). Con esa firma estable macOS conserva los permisos
entre compilaciones; con firma ad-hoc hay que volver a darlos cada vez.

La primera compilación tarda ~20 min en un Mac de 8 GB (GPUI y todos los
crates); las siguientes, unos 4–5 min. `PILL_JOBS` cambia los trabajos de
cargo (4 por omisión, para no quedarse sin memoria).

También se puede correr el binario suelto
(`~/Library/Caches/atic-gpui-target/debug/pill-gpui`): funciona, pero los
permisos quedan a nombre de la terminal y los avisos no reciben el clic.

Tests de la pill:

```bash
cd prototypes/pill-gpui
CARGO_TARGET_DIR=~/Library/Caches/atic-gpui-target cargo test -j 4 <filtro>
```

## 4. Cómo portar la pill

El contrato está escrito en `prototypes/pill-gpui/src/platform/mod.rs`: cada
módulo de `win32/` (`win`, `glass`, `paste`, `hotkeys`, `drag`, `ocr`,
`privacy`, `running`, `autostart`, `tray_icon`, `screen_record`) necesita su
gemelo en `platform/macos/` con las mismas funciones públicas, elegido con
`#[cfg(target_os = "macos")]` en `platform/mod.rs`. El resto de la pill los usa
como `crate::win`, `crate::paste`, etc. y no debería cambiar.

Orden sugerido, de lo que destraba más a lo que menos:

1. **Que compile.** Crear `platform/macos/` con todo como stub (funciones que
   no hacen nada o devuelven `None`). Poner detrás de `#[cfg(windows)]` el
   código Win32 que hoy está fuera de `platform/` (por ejemplo `capture.rs`,
   `color.rs`, `media.rs`, `single_instance.rs`, `system/os.rs`,
   `code/backdrop.rs`, `meetings/detect.rs`; `grep -rln "windows::"
   prototypes/pill-gpui/src` da la lista completa). Meta: que la pill abra en
   Mac aunque falten cosas.
2. **Overlay (`win`).** Un `NSPanel` siempre encima, que no se activa, en todos
   los espacios y por monitor, y que deje pasar los clics fuera de la pill.
   Debe convivir con el notch físico de las MacBook.
3. **Atajos (`hotkeys`).** Grabar, dictar, traer la pill, la rueda.
4. **Pegar (`paste`).** Cmd+V con `CGEvent`; pide permiso de Accesibilidad.
5. **Reuniones y dictado.** `atic-audio` ya graba en Mac (micrófono y audio
   del sistema con ScreenCaptureKit). Revisar que la pill lo use igual.
6. **Vidrio (`glass`)**, con `NSVisualEffectView`.
7. **Bandeja (`tray_icon`)** como `NSStatusItem` y **autostart** como
   LaunchAgent.
8. **Capturas, OCR (Vision), medios y el resto.**

De la versión Tauri se puede copiar la lógica de Mac ya resuelta:
`apps/desktop/src-tauri/src/` (permisos TCC en `permissions.rs` y
`macos_notes.rs`, medios por `osascript` en `media.rs`, el overlay y la
geometría del notch).

Dos detalles de la pill que hoy dependen de `apps/desktop` y en Mac también
aplican: `build.rs` lee la versión de `apps/desktop/src-tauri/tauri.conf.json`,
y el ícono y los emojis se leen desde `apps/desktop`. No borres esa carpeta.

## Avance del port

| Pieza | Cómo va en macOS | Estado |
| --- | --- | --- |
| Overlay (`win`) | `NSPanel` de GPUI sin título, clics que atraviesan, fuera de las capturas; se pega al borde de arriba sobre la barra de menús, como el notch | listo |
| Atajos (`hotkeys`) | `RegisterEventHotKey` de Carbon (sin permisos); `CmdOrCtrl` es ⌘ | listo |
| Teclas mantenidas (dictado, rueda) | `CGEventSourceKeyState` con las teclas de Windows mapeadas (`keys.rs`); pide Monitoreo de entrada | listo |
| Pegar (`paste`) | Cmd+V con `CGEvent` a la app destino (Accesibilidad) | listo |
| Barra de menús (`tray_icon`) | `NSStatusItem` con la marca de Atic como plantilla y el mismo menú | listo |
| Avisos | `UNUserNotificationCenter` (clic abre la conversación); sin `.app`, `osascript` | listo |
| Instancia única | `flock` sobre `pill.lock` y un socket Unix para despertar a la que corre | listo |
| Vidrio (`glass`) | `NSVisualEffectView` en una ventana debajo del overlay; respeta «Reducir transparencia» | listo |
| OCR (`ocr`) | Vision (`VNRecognizeTextRequest`), con cajas por palabra para tapar datos | listo |
| Arrastrar (`drag`) | `NSDraggingSession` desde el overlay, con loop modal como `DoDragDrop` | listo |
| Apps abiertas (`running`) | `NSWorkspace.runningApplications`; cerrar es Cmd+Q (`terminate`) | listo |
| Privacidad | Micrófono por Core Audio (con la app, macOS 14.2+) y cámara por Core Media IO | listo |
| Grabar pantalla | `screencapture -v -R` con audio del sistema y micrófono; pausa por segmentos que se juntan en MP4 con AVFoundation | listo |
| Inicio con macOS | `SMAppService` con la `.app` (Ítems de inicio); LaunchAgent con el binario suelto | listo |
| Capturas, Pizarra, Flip, cuentagotas | Pantalla en puntos y cuadro en píxeles separados (`Frozen::density`) | listo |
| Medios | MediaRemote desde un `osascript` persistente (macOS 15.4+ cierra MediaRemote a terceros); controles por teclas multimedia; mover la canción por AppleScript en Spotify y Música | listo, sin nivel real (la onda es un vaivén) |
| Sistema | Volumen, salidas y micrófono por Core Audio; brillo por DisplayServices; café con `caffeinate`; batería con `pmset`; procesos por app con `ps` | listo, sin volumen por app |
| Lanzador | Apps de Spotlight (`mdfind`) con su nombre del Finder e ícono; `open`; bloquear, suspender, silenciar, vaciar papelera | listo |
| Agentes | La terminal que aloja al agente (por proceso padre) se trae al frente | listo |
| Atic Code | Semáforos nativos (GPUI parcheado agranda su contenedor, ver `vendor/PATCHES.md`), doble clic en la barra según Ajustes del Sistema, ⌘ en los atajos, shell del usuario (`zsh -l`), `Mostrar en Finder`, sidecar dentro de la `.app` | listo |
| Permisos | Pantalla de permisos al arrancar mientras falte alguno (`permission_setup.rs`, «Pedir todos» de a uno); sección «Permisos» en Ajustes | listo |
| Dock | Sin ícono con solo el notch; aparece con una ventana normal abierta | listo |
| Menús de la app (`app_menu.rs`) | Atic, Edición (manda ⌘C/⌘V… a la ventana del frente) y Ventana: minimizar (⌘M), acercar y pantalla completa (⌃⌘F, también en la paleta de Atic Code) | listo |
| Updater | `.app.tar.gz` del release, firma minisign, cambia la `.app` en su lugar y la reabre (`updater.rs`); solo la `.app` publicada (`com.ciat.atic`) | listo |

Decisiones:

- «Físico» en macOS son puntos con el origen arriba a la izquierda de la
  pantalla principal, lo mismo que `atic-capture` en Mac. Lo que en Windows
  multiplica lógicos por la escala no debe hacerlo en macOS. El cuadro que
  devuelve `atic-capture` sí va en píxeles: `Frozen::density` dice cuántos por
  punto y `Frozen::to_screen` vuelve a puntos.
- Mover o cambiar la máscara de la ventana va diferido con `dispatch_async`
  a la cola principal: AppKit le avisa a GPUI en el acto y, a media
  actualización, da `RefCell already borrowed` (`main_thread.rs`).
- Las letras: `src/fonts.rs` (San Francisco y Menlo en Mac); la Pizarra dibuja
  con Helvetica Neue porque `ttf-parser` no lee los contornos de la SF
  variable sin elegir un peso.
- La app arranca con el PATH del shell de inicio de sesión (`platform::startup`):
  abierta desde el Finder no encontraría `node`, `claude` ni `git`.
- Foco de ventanas: nada de AppKit que cambie el foco o mueva ventanas corre
  dentro de un cuadro de GPUI (va con `later`, la cola principal): AppKit le
  avisa a GPUI en el acto y GPUI lo pierde («RefCell already borrowed»).
  Además GPUI se traba si recibe «se volvió la ventana activa» sin serlo
  (`window_did_change_key_status` llama a `resignKeyWindow` con su candado
  tomado): `win::make_key` da el foco con el delegado de GPUI desconectado y
  le avisa después. Las ventanas normales se abren sin foco
  (`FOCUS_ON_OPEN`) y `bring_to_front` activa la app y espera a que esté
  activa. El notch es un panel que no activa la app (como Spotlight): toma el
  teclado sin activarla.
- El bucle de la pill (`tick`) no se corta por una vuelta fallida: antes eso
  dejaba sin atajos ni dictado (el micrófono abierto) hasta reiniciar. El
  dictado además se corta solo a los 5 minutos.
- Varias pantallas: una ventana de macOS no se ve en dos pantallas a la vez
  (cada una tiene sus espacios), así que la mira de Capturas va a la pantalla
  del cursor en vez de estirarse a todo el escritorio. En una pantalla con
  notch físico la pill va justo debajo de la cámara.
- Atajos dentro de las ventanas: ⌘ es la tecla principal (`keymap::primary`,
  `keymap::primary_held`); en las terminales ⌃ queda para el shell.

## Publicar en macOS y pasar desde Tauri

`bash scripts/release-macos.sh` arma la Atic de Mac sin Tauri, como
`release-windows.ps1` en Windows:

1. `scripts/build-pill-mac.sh` compila la pill en release para Apple Silicon e
   Intel (una a la vez), la junta con `lipo` y arma `target/pill-mac/Atic.app`
   con `atic-mcp` y el agente de Atic Code. Lleva el identificador de la app de
   Tauri (`com.ciat.atic`) y se firma con «Atic Dev».
2. Empaqueta `Atic.app.tar.gz`, lo firma con la clave del updater (la misma de
   Windows) y arma el `.dmg` y `latest.json` (`darwin-aarch64` y
   `darwin-x86_64`), sumando lo que ya tenga el release publicado.
3. `--publish` sube todo al release del tag (lo crea si no existe).

Cómo pasa la gente que tiene la Atic de Tauri: su updater lee el mismo
`latest.json`, baja el `.app.tar.gz`, cambia el contenido de
`/Applications/Atic.app` por la pill y abre el ejecutable que dice el
`Info.plist` nuevo. Con el mismo identificador y la misma firma macOS conserva
los permisos ya dados; los datos están en la misma carpeta. Al arrancar, la
pill borra el LaunchAgent de Tauri (`Atic.plist`) y se registra ella como
ítem de inicio si `autostart` está encendido.

Después, la pill se actualiza sola desde el mismo `latest.json`. La de
desarrollo (`Atic Pill.app`, `com.ciat.atic.pill`) no se actualiza.

## 5. Datos y llaves

La pill usa la misma configuración y llavero que Atic (`atic-core`). En Mac la
configuración vive en `~/Library/Application Support/com.ciat.atic/` (ver
`crates/core/src/paths.rs`) y las llaves (Groq, Anthropic…) en el Llavero de
macOS. En Mac el dictado y Reuniones transcriben por omisión con la voz de
macOS, sin llave; Groq queda para elegir en Ajustes (y para los subtítulos en
vivo). La llave gratis de Groq: <https://console.groq.com/keys>.

### Voz de macOS y Apple Intelligence

- **El ayudante `atic-apple`** (`prototypes/pill-gpui/macos/apple/main.swift`)
  existe porque SpeechAnalyzer y Foundation Models solo tienen API en Swift.
  `pill-dev-mac.sh` y `build-pill-mac.sh` lo compilan con `swiftc` (macOS
  26+), lo copian a `Contents/MacOS` y lo firman. La pill lo lanza por cada
  pedido (`src/platform/macos/apple.rs`) y lee JSON por línea:
  `status`, `install`, `transcribe` y `polish`.
- **Transcribir** usa `SpeechTranscriber` en el equipo, con el idioma de la
  configuración, y «Detectar» usa el del sistema (`es-CL`). El transcriptor
  de dictado de Apple no pone puntuación, por eso no se usa. Una nota de 7 s
  tarda unos 0.5 s y una reunión de 55 s, cerca de 1 s.
- **Si la voz de macOS falla** y hay llave de Groq, el dictado se hace con
  Groq.
- **Pulir con Apple Intelligence** es opcional (`dictation_polish`, apagado
  por omisión) y suma cerca de 1 s (más la primera vez). Si el modelo
  devuelve otro texto (lo respondió o lo resumió), se pega el original.
- **En Ajustes:** Dictado y Reuniones muestran el motor y el estado.
  Permisos tiene la tarjeta «Funciones de Apple», con botones para bajar el
  idioma o abrir Ajustes del Sistema → Apple Intelligence y Siri
  (`apple_settings.rs`).
- **En un macOS anterior al 26** el ayudante no abre y la pill lo muestra
  como «No disponible».

## 6. Reglas del repo

- Lee [`AGENTS.md`](../AGENTS.md) y [`CONTRIBUTING.md`](../CONTRIBUTING.md).
  Partes de ellos todavía describen el flujo de Tauri (`pnpm dev`); para la
  pill vale lo de esta guía.
- Comentarios y textos en español de Chile.
- Cambios chicos y probados en la app antes de subir.
