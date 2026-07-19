# Atic

Asistente de escritorio que graba el audio del PC (micrófono + sistema) durante
llamadas y reuniones, para luego transcribirlo localmente y generar resúmenes
que se pueden enviar por correo. Multiplataforma (Windows y macOS), liviano y no
intrusivo: barra flotante ("pill"), atajo global y bandeja del sistema.

> Estado: **Fases 1–3 y 5** completas en Windows. **Fase 4** (macOS system
> audio) en andamiaje — requiere Mac. **Fase 6** documentada en
> [`docs/MOBILE.md`](docs/MOBILE.md).
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
  transcribe/  Whisper local + live + import de audio
  summarize/   Resumen BYOK: Claude, Ollama, OpenAI-compat (OpenAI, OpenRouter, Groq, MiniMax, Custom)
  mailer/      Envío SMTP (lettre) o borrador mailto:
apps/desktop/  App Tauri 2: ventana principal + pill + tray + capturas + detección de llamadas
```

## Requisitos de desarrollo

### Windows

- Rust (toolchain `stable-x86_64-pc-windows-msvc`)
- Visual Studio Build Tools con el workload "Desktop development with C++"
- **CMake** y **LLVM/libclang** — los necesita `whisper-rs`, que compila
  whisper.cpp. En Windows, si `libclang.dll` no está en el PATH, exporta
  `LIBCLANG_PATH` apuntando a `…\LLVM\bin`.
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

`https://github.com/ciat/atic/releases/latest/download/latest.json`

En Ajustes hay un botón **Buscar actualizaciones**. Al arrancar la app también
hace un chequeo discreto (solo un aviso si hay update).

## Validación

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Dónde se guardan los datos

- **Windows:** `%APPDATA%\ciat\atic\data\`
- **macOS:** `~/Library/Application Support/ciat/atic/data/`

Contenido típico:

- `recordings/<id>/mic.wav` y `system.wav` — pistas de cada grabación
- `recordings/<id>/transcript.json` — transcripción
- `recordings/<id>/summary.json` — resumen editable
- `captures/` — capturas de pantalla temporales
- `atic.db3` — índice de grabaciones (SQLite)
- `config.json` — preferencias (no contiene secretos)
- API keys de proveedores (Claude, OpenAI, OpenRouter, Groq, MiniMax, Custom)
  y contraseña SMTP → llavero del sistema (`keyring`)

## Aviso legal

Grabar llamadas puede requerir el consentimiento de los participantes según la
jurisdicción. La app nunca graba de forma automática: siempre requiere una
acción explícita del usuario. La detección de llamadas solo sugiere grabar.
