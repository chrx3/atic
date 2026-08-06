# Atic

Asistente de escritorio que graba el audio del PC (micrófono + sistema) durante
llamadas y reuniones, para luego transcribirlo con Whisper local —o con Groq si
el usuario elige ese backend— y generar resúmenes que se pueden enviar por
correo. Multiplataforma (Windows y macOS), liviano y no intrusivo: barra
flotante ("pill"), atajo global y bandeja del sistema.

> Estado: **Fases 1–3 y 5** completas en Windows. **Fase 4** (macOS system
> audio) en andamiaje — requiere Mac. **Fase 6** documentada en
> [`docs/MOBILE.md`](docs/MOBILE.md).
>
> Catálogo de capacidades (hechas e ideas): **[`Features/`](Features/)**.
>
> En Ajustes: **modo parlantes**, pistas yo/otros, **autostart** y detección
> mejorada (Meet en navegador + mic en uso). Onboarding de primer uso con nota
> de consentimiento.

## Arquitectura

Workspace de Cargo con la lógica en crates reutilizables y la UI en una app
Tauri 2 + SvelteKit encima.

```
crates/
  core/        Dominio: grabaciones, storage SQLite, config, secretos (keyring)
  audio/       Captura mic + loopback (WASAPI en Windows; stub macOS fase 4)
  capture/     Capturas de pantalla (ventana / región / monitor) en Windows
  transcribe/  Whisper local + Groq opcional + live + import de audio
  summarize/   Resumen BYOK: Claude, Ollama, OpenAI-compat (OpenAI, OpenRouter, Groq, MiniMax, Custom)
  mailer/      Envío SMTP (lettre) o borrador mailto:
apps/desktop/  App Tauri 2: ventana principal + pill + tray + capturas + detección de llamadas
  src-tauri/src/agents/   Agentes de consola dentro de Atic (ver abajo)
```

### Agentes

Claude Code, OpenCode, Cursor y Codex conversando dentro de la app, con sus
herramientas, sus permisos y tu misma sesión del CLI. Atic no autentica nada:
se cuelga de la instalación que ya tenés.

```
agents/model.rs       Modelo canónico: hilo → turnos → items con id estable
agents/turns.rs       Emitir deltas y saber en qué turno se está
agents/claude_code.rs Adaptador de Claude Code (su `stream-json` propio)
agents/acp.rs         Adaptador ACP: OpenCode y Cursor con el mismo código
agents/exe.rs         Encontrar el ejecutable (shims de npm, PATHEXT, `cmd /C`)
agents/store.rs       Persistencia de conversaciones en `atic.db3`
agents/skills.rs      Descubrir skills en disco, con su descripción
agents/bridge.rs      Comandos de Tauri y registro de sesiones vivas
```

La clave está en `model.rs`: la interfaz **solo** conoce el modelo canónico, y
cada backend traduce hacia él. Ese modelo tiene a propósito la forma de
[ACP](https://agentclientprotocol.com/), así que sumar un agente que ya hable
ACP es cambiar una constante.

Estado, plan y traspaso: **[`docs/PLAN_AGENTES.md`](docs/PLAN_AGENTES.md)**.

Para probar un adaptador sin abrir la interfaz:

```bash
cargo run -p atic-desktop --example agente_real -- opencode "lee README.md y di de que trata"
```

## Requisitos de desarrollo

### Windows

- Rust (toolchain `stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools con el workload "Desktop development with C++"
- **CMake** y **LLVM/libclang** — los necesita `whisper-rs`, que compila
  whisper.cpp. En Windows, si `libclang.dll` no está en el PATH, exporta
  `LIBCLANG_PATH` apuntando a `…\LLVM\bin`.
- **`CPATH` con las cabeceras de clang Y las de MSVC**, si compilas fuera del
  «Developer PowerShell for VS». `libclang` **no** lee `INCLUDE` —la variable
  que define MSVC— así que encuentra su propia DLL pero no las cabeceras del
  sistema. El síntoma engaña: `fatal error: 'stdio.h' file not found`, seguido
  de un `attempt to compute 12_usize - 16_usize` en unos bindings de **Linux**
  que `whisper-rs-sys` usa de reserva. No es un problema de Rust ni del proyecto.

  Desde LLVM 19 hace falta además el directorio de cabeceras *propias* de clang
  (`lib\clang\<mayor>\include`). Sin él el error cambia a `'stdbool.h' file not
  found` —un header que provee el compilador, no el sistema— y agregar `INCLUDE`
  no alcanza. Ajusta el número de versión al de tu instalación.

  ```powershell
  $vc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
  & cmd.exe /c "`"$vc`" > nul & set" |
    ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2]) } }
  $env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
  $env:CPATH = "C:\Program Files\LLVM\lib\clang\22\include;" + $env:INCLUDE
  ```

  Si ya hubo un intento fallido, cargo **cachea** los bindings malos: el error
  vuelve idéntico aunque el entorno ya esté bien. Hay que borrar el directorio
  del build script y dejar que se regenere:

  ```powershell
  Remove-Item -Recurse -Force target\debug\build\whisper-rs-sys-*
  ```
- Node.js 22+ y pnpm 10+
- WebView2 (incluido en Windows 11)

### macOS

Guía completa para clonar y probar en Mac (herramientas, permisos, límites
de audio del sistema, checklist):

**[`docs/MACOS.md`](docs/MACOS.md)**

Resumen: Xcode CLT, Rust stable, CMake, Node 22+, pnpm 10+. En Mac, por ahora
solo se graba el **micrófono** (fase 4 pendiente).

## Cómo ejecutar

```bash
cd apps/desktop
pnpm install
pnpm tauri dev      # desarrollo con recarga en caliente
pnpm tauri build    # instalador de producción (más adelante)
```

En Mac, sigue el paso a paso de [`docs/MACOS.md`](docs/MACOS.md) antes del
primer `pnpm tauri dev`.

## Empaquetado e instaladores

Los builds de producción generan instaladores y, si hay claves de firma
configuradas, artefactos firmados para el auto-updater:

| Plataforma | Formato | Salida local |
|---|---|---|
| Windows | NSIS (`*-setup.exe`) + `.sig` | `target/release/bundle/nsis/` |
| macOS | DMG + `.app.tar.gz` + `.sig` | `target/release/bundle/dmg/` y `macos/` |

Build local (en la plataforma correspondiente):

```bash
cd apps/desktop
pnpm tauri build
```

Por defecto Whisper corre en **CPU** (así van CI y release). Aceleración GPU
opcional para builds locales avanzados (requiere toolchain del backend):

| Feature | Plataforma | Requisitos |
|---|---|---|
| `gpu-metal` | macOS | Xcode / Metal (solo Mac) |
| `gpu-cuda` | Windows / Linux NVIDIA | CUDA Toolkit |
| `gpu-vulkan` | Windows / Linux AMD/Intel | Vulkan SDK |

```bash
cd apps/desktop
# macOS (Metal)
pnpm tauri build -- --features gpu-metal
# Windows/Linux NVIDIA (CUDA)
pnpm tauri build -- --features gpu-cuda
# Windows/Linux AMD/Intel (Vulkan)
pnpm tauri build -- --features gpu-vulkan
```

Release automático: al publicar un tag `v*` (por ejemplo `v0.1.0`), el workflow
[`.github/workflows/release.yml`](.github/workflows/release.yml) construye
Windows + macOS en GitHub Actions, firma los artefactos del updater y sube
instaladores + `.sig` + `latest.json` al Release del tag.

```bash
git tag v0.1.0
git push origin v0.1.0
```

Sin firma de código de plataforma (Authenticode / Apple notarization), Windows
puede mostrar SmartScreen y macOS Gatekeeper pedirá abrir la app desde Ajustes /
clic derecho → Abrir. Los modelos Whisper se descargan en el primer uso; no van
dentro del instalador.

### Auto-updater (firma Tauri)

La app puede buscar e instalar actualizaciones desde GitHub Releases
(`latest.json`). Requiere un par de claves minisign; **sin ellas el release
firmado y el updater no funcionan** (esperado hasta configurarlas).

1. Generar el par de claves (guarda la privada fuera del repo):

```bash
cd apps/desktop
pnpm tauri signer generate
```

2. Pegar la **clave pública** en
   `apps/desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`
   (reemplaza el placeholder `REEMPLAZAR_CON_TAURI_PUBLIC_KEY`).

3. En GitHub → Settings → Secrets and variables → Actions, crear:

| Secret | Contenido |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Contenido completo de la clave privada |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Contraseña de la clave (vacío si no usaste una) |

Nunca subas la clave privada al repositorio. El endpoint del updater apunta a:

`https://github.com/chrx3/atic/releases/latest/download/latest.json`

En Ajustes hay un botón **Buscar actualizaciones**. Al arrancar la app también
hace un chequeo discreto (solo un aviso si hay update).

## Validación

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Depurar la pill y el pegado

Dos subsistemas son difíciles de depurar leyendo el código, porque el estado
real vive fuera del proceso: la **geometría** de las ventanas flotantes (varios
escritores compiten por la posición: el reconciliador, los tweens y el clamp del
monitor) y el **destino del pegado** (depende de qué ventana tiene el foco en el
SO y de qué control tiene el cursor dentro de ella).

Los dos tienen trazas dedicadas, apagadas por defecto:

```bash
# Geometría: cada línea es UNA escritura de posición o tamaño.
RUST_LOG=info,pill_geo=debug pnpm tauri dev

# Pegado: destino elegido, foco del control y modificadores hundidos.
RUST_LOG=info,paste_geo=debug pnpm tauri dev
```

En PowerShell: `$env:RUST_LOG = "info,pill_geo=debug"` antes de `pnpm tauri dev`.

Leídas en orden, las líneas de `pill_geo` reconstruyen el recorrido completo de
la ventana y dicen quién la movió. `SendInput` no informa si alguien atendió la
tecla, así que un pegado fallido se ve igual que uno exitoso: `paste_geo` es lo
que permite distinguirlos.

## Dónde se guardan los datos

- **Windows:** `%APPDATA%\ciat\atic\data\`
- **macOS:** `~/Library/Application Support/ciat/atic/data/`

Contenido típico:

- `recordings/<id>/mic.wav` y `system.wav` — pistas de cada grabación
- `recordings/<id>/transcript.json` — transcripción
- `recordings/<id>/summary.json` — resumen editable
- `captures/` — capturas de pantalla temporales
- `clipboard/history.json` — historial del portapapeles, en texto plano. Se
  apaga desde Ajustes, y nunca archiva lo que un gestor de contraseñas marcó
  como efímero (formatos `ExcludeClipboardContentFromMonitorProcessing` y
  `CanIncludeInClipboardHistory` de Windows)
- `logs/atic.YYYY-MM-DD.log` — registro de la app, 7 días. Incluye los pánicos
  con su traza; es lo que hay que adjuntar en un reporte. Hay un botón para
  abrir la carpeta en Ajustes
- `atic.db3` — índice de grabaciones (SQLite)
- `config.json` — preferencias (no contiene secretos)
- API keys de proveedores (Claude, OpenAI, OpenRouter, Groq, MiniMax, Custom)
  y contraseña SMTP → llavero del sistema (`keyring`)

## Aviso legal

Grabar llamadas puede requerir el consentimiento de los participantes según la
jurisdicción. La app nunca graba de forma automática: siempre requiere una
acción explícita del usuario. La detección de llamadas solo sugiere grabar.
