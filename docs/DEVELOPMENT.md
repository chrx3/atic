# Desarrollo

Cómo clonar, compilar y empaquetar Atic. La cara de producto está en el
[README](../README.md). En Mac, empezá por [`MACOS.md`](MACOS.md).

## Arquitectura

Workspace de Cargo con la lógica en crates y la UI en Tauri 2 + SvelteKit.

```
crates/
  core/        Dominio: grabaciones, SQLite, config, secretos (keyring)
  audio/       Captura mic + loopback (WASAPI en Windows; stub macOS)
  capture/     Capturas de pantalla (ventana / región / monitor) en Windows
  transcribe/  Whisper local + Groq opcional + live + import de audio
  summarize/   Resumen BYOK: Claude, Ollama, OpenAI-compat
  mailer/      Envío SMTP (lettre) o borrador mailto:
apps/desktop/  App Tauri 2: ventana principal + overlay (pill) + tray
```

## Requisitos

### Windows

- Rust (`stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools con el workload "Desktop development with C++"
- **CMake** y **LLVM/libclang** — los necesita `whisper-rs` / whisper.cpp.
  Si `libclang.dll` no está en el PATH, exportá `LIBCLANG_PATH` a `…\LLVM\bin`.
- **`CPATH` con las cabeceras de clang Y las de MSVC** si compilás fuera del
  Developer PowerShell for VS. `libclang` **no** lee `INCLUDE`. El síntoma
  engaña: `fatal error: 'stdio.h' file not found`, seguido de un
  `attempt to compute 12_usize - 16_usize` en bindings de Linux que
  `whisper-rs-sys` usa de reserva.

  Desde LLVM 19 hace falta el directorio de cabeceras propias de clang
  (`lib\clang\<mayor>\include`). Sin él el error pasa a `'stdbool.h' file not
  found`. Ajustá el número de versión al de tu instalación.

  ```powershell
  $vc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
  & cmd.exe /c "`"$vc`" > nul & set" |
    ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2]) } }
  $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
  $env:CPATH = "C:\Program Files\LLVM\lib\clang\22\include;" + $env:INCLUDE
  ```

  Si ya hubo un intento fallido, cargo cachea los bindings malos. Borrá el
  directorio del build script:

  ```powershell
  Remove-Item -Recurse -Force target\debug\build\whisper-rs-sys-*
  ```
- Node.js 22+ y pnpm 10+
- WebView2 (incluido en Windows 11)

### macOS

Xcode CLT, Rust stable, CMake, Node 22+, pnpm 10+. Guía completa:
[`MACOS.md`](MACOS.md). En Mac, por ahora solo se graba el **micrófono**.

## Cómo ejecutar

```bash
cd apps/desktop
pnpm install
pnpm tauri dev      # desarrollo con recarga en caliente
pnpm tauri build    # instalador de producción
```

## Validación

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Frontend:

```bash
cd apps/desktop
pnpm verify
```

## Empaquetado

| Plataforma | Formato | Salida local |
|---|---|---|
| Windows | NSIS (`*-setup.exe`) | `target/release/bundle/nsis/` |
| macOS | DMG + `.app.tar.gz` | `target/release/bundle/dmg/` y `macos/` |

Por defecto Whisper corre en **CPU** (CI y release). GPU opcional en builds
locales:

| Feature | Plataforma | Requisitos |
|---|---|---|
| `gpu-metal` | macOS | Xcode / Metal |
| `gpu-cuda` | Windows / Linux NVIDIA | CUDA Toolkit |
| `gpu-vulkan` | Windows / Linux AMD/Intel | Vulkan SDK |

```bash
cd apps/desktop
pnpm tauri build -- --features gpu-metal    # macOS
pnpm tauri build -- --features gpu-cuda     # NVIDIA
pnpm tauri build -- --features gpu-vulkan   # AMD/Intel
```

Un tag `v*` dispara [`.github/workflows/release.yml`](../.github/workflows/release.yml):

```bash
git tag v0.3.3
git push origin v0.3.3
```

Sin firma Authenticode / notarization de Apple, Windows puede mostrar
SmartScreen y macOS Gatekeeper pedirá Abrir desde Ajustes. Los modelos Whisper
se descargan en el primer uso; no van en el instalador.

### Auto-updater

La app busca updates en GitHub Releases (`latest.json`). Hace falta un par
minisign; la clave privada **nunca** va al repo.

1. `cd apps/desktop && pnpm tauri signer generate`
2. Pegar la clave **pública** en
   `apps/desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`
3. Secrets de Actions: `TAURI_SIGNING_PRIVATE_KEY` y
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Endpoint: `https://github.com/chrx3/atic/releases/latest/download/latest.json`

## Depurar la pill y el pegado

```bash
# Geometría: cada línea es UNA escritura de posición o tamaño.
RUST_LOG=info,pill_geo=debug pnpm tauri dev

# Pegado: destino elegido, foco del control y modificadores hundidos.
RUST_LOG=info,paste_geo=debug pnpm tauri dev
```

En PowerShell: `$env:RUST_LOG = "info,pill_geo=debug"` antes de `pnpm tauri dev`.

## Datos locales

- Windows: `%APPDATA%\ciat\atic\data\`
- macOS: `~/Library/Application Support/ciat/atic/data/`

Para borrar todo y arrancar de cero en Windows: cerrar desde la bandeja,
desinstalar, borrar `%APPDATA%\ciat\atic` y `%LOCALAPPDATA%\com.ciat.atic` si
existe. Las API keys del llavero no se van con esas carpetas.
