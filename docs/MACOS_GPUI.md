# Atic en macOS con GPUI

Guía para quien llega al repo con un Mac y quiere trabajar en la app actual.
Escrita el 2026-10-15.

## Estado hoy

- **La app es la pill GPUI** (`prototypes/pill-gpui`), en Rust puro. Es lo que
  instala el instalador de Windows. Todo trabajo nuevo va ahí.
- **Tauri (`apps/desktop`) es legado.** Solo se mantiene porque la versión de
  Mac todavía sale de ahí ([`MACOS.md`](MACOS.md)). No se le agregan
  funciones. Se borra cuando la pill GPUI corra en Mac.
- **La pill GPUI compila y arranca en macOS** desde el 2026-10-15, con una
  primera capa de plataforma (`src/platform/macos/`): el overlay, pegar,
  inicio con macOS y la lista de ventanas funcionan; vidrio, bandeja, atajos,
  arrastre, OCR y grabar todavía son vacíos. Lo que falta está en
  [Avance del port](#avance-del-port).
- **Los crates compartidos ya tienen macOS** (`crates/audio` con
  ScreenCaptureKit, `crates/capture` con CoreGraphics, `crates/clipboard`):
  vienen de la versión Tauri y se pueden reusar tal cual.

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

## 3. Comprobar lo que ya funciona

```bash
cd atic
cargo test --locked -p atic-core          # configuración, llavero, rutas
cargo test --locked -p atic-transcribe    # transcripción con Groq
```

La pill tiene su propio `Cargo.lock` y su propio workspace (no es miembro del
de la raíz). Compilarla y abrirla:

```bash
cd prototypes/pill-gpui
cargo build
"${CARGO_TARGET_DIR:-target}/debug/pill-gpui"
```

La primera compilación tarda unos minutos (GPUI); las siguientes, alrededor de
un minuto. Usar un `CARGO_TARGET_DIR` propio (por ejemplo
`~/Library/Caches/atic-gpui-target`) evita chocar con `tauri dev`.

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

| Pieza | Estado |
| --- | --- |
| Compilar: `platform/macos/`, `cfg` en lo Win32 de fuera, `runtime_shaders` | listo |
| `win`: overlay sin título (si no, AppKit lo baja de la barra de menús), clics que atraviesan, fuera de las capturas, foco, monitores y cursor | listo |
| `paste`: Cmd+V con `CGEvent` (pide Accesibilidad), destino por pid | listo, sin probar en uso |
| `autostart`: LaunchAgent `com.ciat.atic.pill` | listo |
| `window_list`: ventanas de Quartz (pegar, flip, reuniones) | listo |
| Portapapeles: imagen como PNG, archivos como URLs (`clip_image_macos.rs`) | listo |
| Flip: lo nativo en `flip/win32.rs` y `flip/macos.rs` (esconde la app, no la ventana) | compila |
| Updater | apagado en macOS (baja el setup de Windows) |
| `hotkeys`, `tray_icon` (`NSStatusItem`), `glass` (`NSVisualEffectView`) | pendiente: vacíos |
| `drag`, `ocr` (Vision), `screen_record` (ScreenCaptureKit), `privacy`, `running`, instancia única | pendiente: vacíos |
| Capturas y Flip: mezclan lógicos×escala con físicos | pendiente |

Decisiones:

- «Físico» en macOS son puntos con el origen arriba a la izquierda de la
  pantalla principal, lo mismo que `atic-capture` en Mac. Lo que en Windows
  multiplica lógicos por la escala no debe hacerlo en macOS.
- Mover o cambiar la máscara de la ventana va diferido con `dispatch_async`
  a la cola principal: AppKit le avisa a GPUI en el acto y, a media
  actualización, da `RefCell already borrowed`.
- `space::changes` y `space::workspaces` tienen pruebas con rutas de Windows
  escritas a mano que fallan en macOS.

## 5. Datos y llaves

La pill usa la misma configuración y llavero que Atic (`atic-core`). En Mac la
configuración vive en `~/Library/Application Support/com.ciat.atic/` (ver
`crates/core/src/paths.rs`) y las llaves (Groq, Anthropic…) en el Llavero de
macOS. Para transcribir basta una llave gratis de Groq
(<https://console.groq.com/keys>).

## 6. Reglas del repo

- Lee [`AGENTS.md`](../AGENTS.md) y [`CONTRIBUTING.md`](../CONTRIBUTING.md).
  Partes de ellos todavía describen el flujo de Tauri (`pnpm dev`); para la
  pill vale lo de esta guía.
- Comentarios y textos en español de Chile.
- Cambios chicos y probados en la app antes de subir.
