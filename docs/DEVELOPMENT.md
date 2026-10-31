# Desarrollo

Cómo clonar, compilar y empaquetar Atic. La cara de producto está en el
[README](../README.md). En Mac, empieza por [`MACOS.md`](MACOS.md).

## Arquitectura

Atic es una app de Rust puro con GPUI (`apps/atic`, con su propio workspace y
`Cargo.lock`) más un workspace de Cargo en la raíz con la lógica compartida.
El mapa completo y las reglas de trabajo están en [`AGENTS.md`](../AGENTS.md).

```
apps/
  atic/        La app: pill, notch, herramientas, Ajustes, Reuniones, Atic Code…
    src/platform/   Lo que depende del sistema (win32/ y macos/)
    sidecar/        El agente de Atic Code en Node (agent.mjs)
    assets/         Íconos, i18n, emojis y changelog.json (las Novedades)
  web/         El sitio web (no es parte de la app)
crates/
  core/        Dominio: grabaciones, SQLite, config, secretos (keyring)
  audio/       Captura mic + loopback (WASAPI en Windows; ScreenCaptureKit en macOS)
  capture/     Capturas de pantalla (GDI en Windows; Core Graphics en macOS)
  transcribe/  Transcripción (Groq, ONNX local; Whisper local opcional) + import de audio
  summarize/   Resumen BYOK: Claude, Ollama, OpenAI-compat
  mailer/      Envío SMTP (lettre) o borrador mailto:
  atic-mcp/    Servidor MCP de Atic
  sign/        atic-sign: firma minisign de los artefactos del updater
  …            (y más: clipboard, agents, sync, atic-unix)
../gpui-m3     Componentes Material 3 (repo aparte, clonado al lado de `atic`)
```

La versión de Atic es el `version` de `apps/atic/Cargo.toml`: la leen `build.rs`
(`ATIC_VERSION`), los scripts de release, el updater y la ventana de Novedades.

## Requisitos

### Windows

- Rust (`stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools con el workload "Desktop development with C++"
- **CMake** y **LLVM/libclang** — solo si activas el Whisper local
  (`--features local` en `atic-transcribe`; por omisión no se compila). Los
  necesita `whisper-rs` / whisper.cpp. Si `libclang.dll` no está en el PATH, exporta `LIBCLANG_PATH` a `…\LLVM\bin`.
- **Cabeceras para bindgen** si compilas fuera del Developer PowerShell for
  VS. `libclang` **no** lee `INCLUDE`. El síntoma engaña: `fatal error:
'stdio.h' file not found`, seguido de un `attempt to compute 12_usize -
16_usize` en bindings de Linux que `whisper-rs-sys` usa de reserva.

  Para no exportar variables a mano, cargá el helper (detecta LLVM, MSVC y el
  Windows SDK —incluidas las cabeceras propias de clang— y arma
  `LIBCLANG_PATH` + `BINDGEN_EXTRA_CLANG_ARGS`):

  ```powershell
  . .\scripts\win-env.ps1
  ```

  `scripts/release-windows.ps1` ya lo carga solo. El equivalente manual con
  `vcvars64.bat` y `CPATH` (ajustá `22` a tu versión de clang):

  ```powershell
  $vc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
  & cmd.exe /c "`"$vc`" > nul & set" |
    ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2]) } }
  $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
  $env:CPATH = "C:\Program Files\LLVM\lib\clang\22\include;" + $env:INCLUDE
  ```

  Si ya hubo un intento fallido, cargo cachea los bindings malos. Borra el
  directorio del build script (debug y/o release):

  ```powershell
  Remove-Item -Recurse -Force target\debug\build\whisper-rs-sys-*, target\release\build\whisper-rs-sys-*
  ```

- Node.js 22+ y pnpm 10+ (el agente de Atic Code y el sitio web)
- `../gpui-m3` clonado al lado del repo:
  `git clone https://github.com/chrx3/gpui-m3.git ../gpui-m3`

### macOS

Xcode CLT, Rust stable, CMake, Node 22+, pnpm 10+. Guía completa:
[`MACOS.md`](MACOS.md). En Mac se graba el **micrófono**; las capturas de
pantalla y la pizarra usan Core Graphics (piden Grabación de pantalla).

## Cómo ejecutar

Desde la raíz del repo:

```powershell
# Windows
powershell -File scripts/pill-dev.ps1          # compila (dev), cierra la Atic abierta y abre la nueva
powershell -File scripts/pill-dev.ps1 -Watch   # recompila y reabre al guardar (~20 s un cambio chico)
```

```bash
# macOS
bash scripts/pill-dev-mac.sh            # compila (debug), arma y firma «Atic Pill.app» y la abre
bash scripts/pill-dev-mac.sh --watch    # recompila y reabre al guardar
bash scripts/pill-dev-mac.sh --release  # optimizada
bash scripts/pill-dev-mac.sh --install  # además la copia a /Applications
```

`pill-dev.ps1` compila en `%LOCALAPPDATA%\atic-gpui`; `pill-dev-mac.sh`, en
`~/Library/Caches/atic-gpui-target` (`PILL_JOBS` cambia los trabajos de cargo,
4 por omisión). La Atic de dev usa los mismos datos y configuración que la
instalada. Con `-Watch` / `--watch` se vigilan la app, `crates/` y `../gpui-m3`.

`scripts/mac-env.sh` (espejo de `scripts/win-env.ps1`) deja el entorno de
compilación consistente; cárgalo con `. scripts/mac-env.sh` si compilas a mano.

El agente de Atic Code se empaqueta aparte, cuando lo cambias:

```bash
pnpm --dir apps/atic/sidecar run build
```

Para afinar colores o números sin compilar, usa `apps/atic/tweaks.json`
(ignorado por git; formato en `src/tweak.rs`).

## Ciclo rápido

El costo del día a día no es escribir código: es compilar y verificar.
Compilar la app toma entre 4 y 10 minutos en frío y mucha memoria; con el
modo `-Watch` / `--watch` un cambio chico baja a unos 20 s. No lances una
compilación mientras otra corre, y no corras `cargo clean` ni mezcles perfiles
(`--release` recompila todo el grafo).

**Aviso sobre sccache e incremental.** Si `~/.cargo/config.toml` tiene
`rustc-wrapper = "sccache"`, **no actives `CARGO_INCREMENTAL`**: sccache lo
prohíbe y el build falla con «incremental compilation is prohibited». En
debug, `pill-dev-mac.sh` compila sin sccache para usar el incremental.

**Tests de la app**, reusando el target de dev (es rápido una vez compilado):

```powershell
cargo test --manifest-path apps/atic/Cargo.toml --target-dir "$env:LOCALAPPDATA\atic-gpui" meetings::
```

En Mac, usa `--target-dir ~/Library/Caches/atic-gpui-target`. Sin filtro corre
todos. Ni `cargo test` ni `cargo check` actualizan el ejecutable: para ver el
cambio en la app usa el script de desarrollo.

**Tests de un crate compartido** (desde la raíz):

```bash
cargo test --locked -p atic-core
```

Si en Windows el enlazado falla con LNK1104 por ruta larga, usa un
`--target-dir` corto (`$env:LOCALAPPDATA\atic-ct`).

**No corras `cargo fmt`** sobre la app ni sobre varios crates: no están
formateados con rustfmt y reescribiría miles de líneas ajenas. Sigue el estilo
del archivo que tocas.

**Antes de subir.** Desde la raíz, según lo que cambió:

```bash
pnpm check:rust   # cargo check del workspace de la raíz
pnpm verify:rust  # formato, clippy estricto y tests del workspace de la raíz
pnpm verify       # sitio web (tipos y build) y formato de Rust
```

Estos comandos cubren el workspace de la raíz y el sitio web, **no** la app
(`apps/atic` tiene su propio workspace): para ella valen sus tests de arriba y
probarla en la app. No hay CI: estas comprobaciones locales son las únicas.
Lo que no se puede simular (los CLIs de agentes reales, los permisos de macOS y
el audio) queda para probarlo a mano.

## Empaquetado y releases

No hay GitHub Actions: todo se compila y se publica en local. No uses estos
scripts para iterar (para eso está el dev loop de arriba).

| Plataforma | Formato              | Script y salida                                                |
| ---------- | -------------------- | -------------------------------------------------------------- |
| Windows    | NSIS (`*-setup.exe`) | `scripts/build-installer.ps1` → `target/installer/`            |
| macOS      | DMG + `.app.tar.gz`  | `scripts/release-macos.sh` → `target/pill-mac/`                |

- `scripts/build-pill.ps1` compila la app en release (`target\pill-gpui`);
  `scripts/build-installer.ps1` la une con los sidecars (`atic-mcp`,
  `atic-unix`) en el instalador NSIS propio (`installer/atic.nsi`) y deja
  `Atic_<versión>_x64-setup.exe`. La versión sale de `apps/atic/Cargo.toml`.
- `scripts/build-pill-mac.sh` arma `target/pill-mac/Atic.app` universal
  (Intel + Apple Silicon); `release-macos.sh` la empaqueta en el `.dmg` y el
  `.app.tar.gz`, los firma para el updater y arma `latest.json`.

**Cada release necesita su entrada** en `apps/atic/assets/changelog.json` (las
Novedades que la app muestra al actualizar); los dos scripts de release fallan
si falta. Formato y criterios en [`AGENTS.md`](../AGENTS.md) («Novedades»).

Desde la raíz del repo:

```powershell
powershell -File scripts/release-windows.ps1
powershell -File scripts/release-windows.ps1 -Publish
```

```bash
bash scripts/release-macos.sh
bash scripts/release-macos.sh --publish
```

Sin `-Publish` / `--publish` solo se arma y firma todo en local. Con él se crea
el tag `v<versión>`, se hace push y se sube el instalador + `.sig` +
`latest.json` al release Latest (cada script fusiona `latest.json` con las
plataformas que ya estaban publicadas). Los releases los hace el mantenedor;
no hacen falta para un PR. Los scripts no exigen validación previa: corre tus
comprobaciones antes.

**Los artefactos del updater se firman en local**: las claves viven en tu disco,
no en CI. Esa firma no equivale a Authenticode en Windows ni a notarización de
Apple. El script de macOS usa una identidad local si está configurada o firma
ad-hoc. Sin Authenticode / notarización, Windows puede mostrar SmartScreen y
macOS Gatekeeper pedirá Abrir desde Ajustes. Los modelos de transcripción se
descargan en el primer uso; no van en el instalador.

El `.exe` solo no alcanza: el updater necesita `.sig` y `latest.json`.

### Auto-updater

La app busca updates en GitHub Releases (`latest.json`) y verifica la firma
minisign con la clave pública fija en `apps/atic/src/updater.rs` (`PUBKEY`). La
clave privada **nunca** va al repo.

La clave privada y la contraseña viven solo en disco, no en git (la carpeta
`.tauri` se conserva por compatibilidad con las claves ya generadas):

- `~/.tauri/atic-updater.key` (en Windows, `%USERPROFILE%\.tauri\atic-updater.key`)
- `~/.tauri/atic-updater.password`

Los scripts de release firman con `atic-sign` (`crates/sign`, mismo formato
minisign de siempre):

```bash
cargo run -q -p atic-sign --release -- <archivo> --key ~/.tauri/atic-updater.key --password-file ~/.tauri/atic-updater.password
```

Si se pierden, hay que generar un par nuevo con minisign (por ejemplo
`minisign -G` o rsign2, con contraseña), pegar la **pública** (el texto del
archivo `.pub` en base64) en `PUBKEY` de `apps/atic/src/updater.rs` y en el test
de `crates/sign`. Las Atic ya instaladas **no** van a aceptar actualizaciones
firmadas con la clave nueva: hay que instalar el instalador a mano una vez.

Endpoint: `https://github.com/chrx3/atic/releases/latest/download/latest.json`

En la app: Ajustes → Información → Buscar actualizaciones. Si el `version` del
manifest es más nuevo, aparece **Actualizar**.

## Depurar y ver el log

El log de la app queda en:

- Windows: `%APPDATA%\ciat\atic\data\logs\pill.<fecha>.log`
- macOS: `~/Library/Application Support/com.ciat.atic/logs/pill.<fecha>.log`

`RUST_LOG` se respeta (si no está, la app usa su filtro por omisión). Fíjalo
antes de correr el script de desarrollo:

```bash
RUST_LOG=debug bash scripts/pill-dev-mac.sh
```

En PowerShell: `$env:RUST_LOG = "debug"` y luego `scripts/pill-dev.ps1`.
Puedes acotarlo por módulo, por ejemplo `RUST_LOG=info,pill_gpui::meetings=debug`.

## Datos locales

- Windows: `%APPDATA%\ciat\atic\data\`
- macOS: `~/Library/Application Support/ciat/atic/data/`

Para borrar todo y arrancar de cero en Windows: cerrar desde la bandeja,
desinstalar, borrar `%APPDATA%\ciat\atic` y `%LOCALAPPDATA%\com.ciat.atic` si
existe. Las API keys del llavero no se van con esas carpetas.
