# Desarrollo

Cómo clonar, compilar y empaquetar Atic. La cara de producto está en el
[README](../README.md). En Mac, empieza por [`MACOS.md`](MACOS.md).

## Arquitectura

Workspace de Cargo con la lógica en crates y la UI en Tauri 2 + SvelteKit.

```
crates/
  core/        Dominio: grabaciones, SQLite, config, secretos (keyring)
  audio/       Captura mic + loopback (WASAPI en Windows; ScreenCaptureKit en macOS)
  capture/     Capturas de pantalla (GDI en Windows; Core Graphics en macOS)
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
  Si `libclang.dll` no está en el PATH, exporta `LIBCLANG_PATH` a `…\LLVM\bin`.
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

- Node.js 22+ y pnpm 10+
- WebView2 (incluido en Windows 11)

### macOS

Xcode CLT, Rust stable, CMake, Node 22+, pnpm 10+. Guía completa:
[`MACOS.md`](MACOS.md). En Mac se graba el **micrófono**; las capturas de
pantalla y la pizarra usan Core Graphics (piden Grabación de pantalla).

## Cómo ejecutar

```bash
cd apps/desktop
pnpm install
pnpm mcp:build    # sidecar MCP (solo la primera vez; después va solo en cada build)
pnpm tauri dev      # desarrollo con recarga en caliente
pnpm tauri build    # instalador de producción
```

En macOS también hay `apps/desktop/dev-atic.sh`: carga el entorno
(`scripts/mac-env.sh`: wrapper de rustc y `libclang` de Homebrew si hace falta)
y arranca `tauri dev`. Acepta `--trace` para las trazas de geometría.

## Ciclo rápido

El costo del día a día no es escribir código: es compilar y verificar. La
regla es escalonar, no saltear.

**Ciclo interno (segundos).** El frontend recarga con HMR dentro de
`pnpm tauri dev`. Para Rust, comprobar sin linkear es una fracción de un
build, y los tests ya compilados corren en menos de un segundo:

```bash
cargo ck                      # alias: check -p atic-desktop, sin linkear
cargo ckt agents::mcp_install # alias: los tests de la lib, con filtro
cargo ckw                     # alias: todo el workspace, sin linkear
```

Los alias viven en `.cargo/config.toml`.

**Aviso sobre sccache e incremental.** Si `~/.cargo/config.toml` tiene
`rustc-wrapper = "sccache"` (como en el equipo de desarrollo), **no actives
`CARGO_INCREMENTAL`**: sccache lo prohíbe y el build falla con «incremental
compilation is prohibited». Con sccache el ciclo interno de Rust es `cargo ck`
(sin linkear) y un build completo cuando necesitas ver la app. El script
`scripts/mac-env.sh` detecta el caso y deja el entorno consistente.

**El crate caro es `atic-desktop`.** Ahí vive la app entera (unas 68 mil
líneas): cualquier cambio en su `src/` recompila ese crate completo (sccache no
puede cachearlo tal como está configurado, por sus crate-types) y enlaza el
binario. Los crates del workspace y las dependencias solo se recompilan si los
tocas, y eso sccache lo cubre. Por eso: cambios de UI → HMR; cambios de lógica
→ `cargo ck` primero, build completo solo para probar la app.

**Antes de commitear.** Formato de lo tocado, clippy y los tests del crate que
cambió:

```bash
cargo fmt --all
cargo clippy -p atic-desktop --all-targets
cargo test -p atic-desktop --lib
```

**Antes de subir.** Desde la raíz, valida según lo que cambió:

```bash
pnpm verify:desktop # tipos, lint, estilos, formato y tests del frontend desktop
pnpm verify:web     # tipos y build de producción del sitio
pnpm verify:rust    # sidecars, formato, clippy estricto y tests del workspace
```

`pnpm verify` combina desktop, web y formato de Rust, sin compilar Rust.
`pnpm verify:all` agrega la validación completa de Rust; úsalo antes de un
release. Los comandos se detienen si falla una etapa y no publican nada.
La CI automática actual cubre solo el frontend desktop y el formato de Rust;
no reemplaza estas comprobaciones locales.

Instala primero las dependencias:

```bash
pnpm --dir apps/desktop install --frozen-lockfile
pnpm --dir apps/web install --frozen-lockfile
```

La validación de Rust requiere el entorno nativo de tu plataforma (en Windows,
carga `scripts/win-env.ps1`; en macOS, `scripts/mac-env.sh`).
`pnpm prepare:sidecars` prepara ambos binarios auxiliares; ejecútalo antes de
`pnpm check:rust` o `pnpm dev` en un clon nuevo.

Los tests no se saltean, se acotan: correr `cargo test -p atic-desktop --lib`
sobre el crate ya compilado cuesta menos que el build que de todos modos haces
para ver la app. Lo que sí queda para el final —o para CI— es lo que no se
puede simular: los CLIs de agentes reales, los permisos de macOS y el audio.

Dos cosas que ahorran minutos: no corras `cargo clean` (el `target/` es
grande y nada de él es basura) y no mezcles perfiles (`--release` recompila
todo el grafo).

## Validación

El workflow `.github/workflows/ci.yml` corre `frontend` y `rustfmt` en cada
push a `main` y en cada PR. Los jobs pesados (`rust`, `build`) siguen siendo
manuales desde Actions → CI → Run workflow: en un repo público los minutos de
runners estándar son gratis, pero un job de Windows/macOS tarda bastante más
que la alternativa local.

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

| Plataforma | Formato              | Salida local                            |
| ---------- | -------------------- | --------------------------------------- |
| Windows    | NSIS (`*-setup.exe`) | `target/release/bundle/nsis/`           |
| macOS      | DMG + `.app.tar.gz`  | `target/release/bundle/dmg/` y `macos/` |

Por defecto Whisper corre en **CPU** (CI y release). GPU opcional en builds
locales:

| Feature      | Plataforma                | Requisitos    |
| ------------ | ------------------------- | ------------- |
| `gpu-metal`  | macOS                     | Xcode / Metal |
| `gpu-cuda`   | Windows / Linux NVIDIA    | CUDA Toolkit  |
| `gpu-vulkan` | Windows / Linux AMD/Intel | Vulkan SDK    |

```bash
cd apps/desktop
pnpm tauri build -- --features gpu-metal    # macOS
pnpm tauri build -- --features gpu-cuda     # NVIDIA
pnpm tauri build -- --features gpu-vulkan   # AMD/Intel
```

PRs hacia `main`. **Los artefactos del updater se firman en local**:
las claves viven en tu disco, no en CI. Esa firma no equivale a Authenticode
en Windows ni a notarización de Apple. El script de macOS usa una identidad
local si está configurada o firma ad-hoc. No hay workflow de release;
crear un tag `v*` no publica instaladores.

Antes de empaquetar, ejecuta `pnpm verify:all` desde la raíz. Por ahora los
scripts de release no exigen esta validación automáticamente.

Desde la raíz del repo:

```powershell
powershell -File scripts/release-windows.ps1
powershell -File scripts/release-windows.ps1 -Publish
```

El script carga la clave de `%USERPROFILE%\.tauri\atic-updater.key` y la
contraseña de `atic-updater.password`, corre `pnpm tauri build --bundles nsis`,
escribe `latest.json` y, con `-Publish`, sube exe + `.sig` + `latest.json` al
release Latest.

El `.exe` solo no alcanza: el updater necesita `.sig` y `latest.json`.

Sin firma Authenticode / notarization de Apple, Windows puede mostrar
SmartScreen y macOS Gatekeeper pedirá Abrir desde Ajustes. Los modelos Whisper
se descargan en el primer uso; no van en el instalador.

### Auto-updater

La app busca updates en GitHub Releases (`latest.json`). Hace falta un par
minisign; la clave privada **nunca** va al repo.

La clave privada y la contraseña viven solo en disco, no en git:

- `%USERPROFILE%\.tauri\atic-updater.key`
- `%USERPROFILE%\.tauri\atic-updater.password`

`scripts/release-windows.ps1` las exporta a `TAURI_SIGNING_*` durante el build.
Si se pierden, hay que generar un par nuevo (`pnpm tauri signer generate`),
pegar la **pública** en `tauri.conf.json` → `plugins.updater.pubkey`, y la
versión anterior **no** va a poder actualizarse sola (hay que instalar el
`.exe` a mano una vez).

Endpoint: `https://github.com/chrx3/atic/releases/latest/download/latest.json`

En la app: Ajustes → Información → Buscar actualizaciones. Si el `version` del
manifest es más nuevo, aparece **Actualizar**.

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
