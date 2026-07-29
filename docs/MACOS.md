# Guía macOS — clonar y echar a andar Atic

Documentación para desarrollar y probar en **macOS** (Apple Silicon o Intel).
El instalable (DMG firmado) viene después; aquí solo el flujo de desarrollo.

> **Limitación actual (fase 4):** en macOS la app graba **solo micrófono**.
> El audio del sistema («Otros» / loopback) aún no está implementado
> (ScreenCaptureKit). En Windows sí funciona mic + sistema.

---

## 1. Requisitos

| Herramienta | Versión / notas |
|---|---|
| macOS | 11.0+ (recomendado 13+ / Ventura o superior) |
| Xcode Command Line Tools | Obligatorios (clang, SDK) |
| Rust | `stable` (rustup) |
| CMake | Para compilar whisper.cpp vía `whisper-rs` |
| Node.js | 22+ |
| pnpm | 10+ |

Opcional pero útil: [Homebrew](https://brew.sh).

### Instalar herramientas

```bash
# Xcode CLT (si no los tienes)
xcode-select --install

# Homebrew (si no lo tienes)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Dependencias de build
brew install cmake rust node pnpm

# Si rustup no quedó en el PATH tras brew:
# curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# source "$HOME/.cargo/env"
rustup default stable
```

Comprueba:

```bash
rustc --version
cargo --version
cmake --version
node --version    # >= 22
pnpm --version    # >= 10
xcode-select -p
```

---

## 2. Clonar el repo

```bash
git clone https://github.com/chrx3/atic.git
cd atic
```

Si trabajáis en una rama distinta de `main`, haz `git checkout <rama>` después.

---

## 3. Instalar dependencias del frontend

```bash
cd apps/desktop
pnpm install
```

La primera vez `pnpm tauri dev` / `cargo` descargará crates y compilará
`whisper-rs` (whisper.cpp). En Mac puede tardar **varios minutos**.

---

## 4. Arrancar en desarrollo

Desde `apps/desktop`:

```bash
pnpm tauri dev
```

Deberías ver:

1. Vite en `http://localhost:1420/`
2. Compilación de `atic-desktop`
3. Ventana principal **Atic** + pill flotante

### Permisos de macOS (primera ejecución)

Al grabar o dictar, el sistema pedirá acceso al **micrófono**. Acepta.

Rutas típicas si lo denegaste por error:

- **Ajustes del Sistema → Privacidad y seguridad → Micrófono** → habilita Atic / Terminal / Cursor (según desde dónde lances `tauri dev`).

El `Info.plist` del proyecto ya declara textos TCC para micrófono y (futuro)
captura de audio/pantalla. La captura de sistema aún no está activa.

### Atajos globales

Por defecto (configurables en la barra de grabación o en Ajustes):

| Acción | Atajo |
|---|---|
| Grabar / detener reunión | `Cmd+Shift+R` (en config: `CmdOrCtrl+Shift+R`) |
| Dictado | `Cmd+Shift+D` |

macOS puede pedir permiso de **Accesibilidad** / **Supervisión de entrada**
para atajos globales. Si el atajo no responde:

- **Ajustes → Privacidad y seguridad → Accesibilidad** (y/o **Supervisión de entrada**)
  → permite la app o el terminal desde el que corres `tauri dev`.

---

## 5. Qué probar en Mac (checklist)

1. **Onboarding** (primera vez): idioma, modelo Whisper, proveedor de resumen.
2. **Descargar modelo Whisper** en Ajustes (recomendado para comenzar: **base**). Sin modelo no hay transcripción ni dictado.
3. **Grabar** con el botón o el atajo → solo pista **Yo** (mic). «Otros» no tendrá audio de sistema todavía.
4. **Transcribir** una grabación y abrir el transcript.
5. **Resumir** (necesitas API key del proveedor o Ollama local).
6. **Dictado** (atajo o botón de la pill): hablar → soltar / toggle → texto pegado en el campo activo.
7. **Pill**: que no se corte al dictar/transcribir; arrastrar posición.
8. **Entrada / salida** en la barra de grabación (mic y altavoces listados).

Datos locales en Mac:

```text
~/Library/Application Support/tsg/atic/data/
```

(o el path que use el bundle id `com.ciat.atic` / organización `ciat`).

---

## 6. Problemas frecuentes

### Fallo al compilar `whisper-rs` / whisper.cpp

- Instala CMake: `brew install cmake`
- Asegura Xcode CLT: `xcode-select --install`
- Limpia y recompila:

```bash
cd apps/desktop
cargo clean -p atic-transcribe
pnpm tauri dev
```

### Puerto 1420 ocupado

```bash
lsof -i :1420
kill <pid>
```

### Solo se graba micrófono / error en «Otros»

Esperado en macOS hasta implementar fase 4. Usa pistas **mic** o **both**
(both = solo mic útil por ahora). Evita modo parlantes / solo sistema.

### Dictado no pega el texto

En Mac el pegado automático vía simulación de teclas puede estar limitado
respecto a Windows. Si falla, el texto debería quedar en el portapapeles:
pega con `Cmd+V`.

### Autostart / bandeja

Puede mostrar avisos en desarrollo; no bloquea el uso normal.

### CPU alto al transcribir

Whisper en CPU (sin GPU) es el **default** de CI/release y de `pnpm tauri build`
sin features. Empieza con **base**; usa **small** si necesitas más precisión y
tu equipo tiene margen de CPU/RAM. Evita medium/large para pruebas.

En Mac puedes compilar con **Metal** (aceleración GPU) para live local y batch:

```bash
cd apps/desktop
pnpm tauri build -- --features gpu-metal
# o en desarrollo:
pnpm tauri dev -- --features gpu-metal
```

`gpu-metal` solo aplica en macOS. En Windows/Linux usa `gpu-cuda` (NVIDIA +
CUDA Toolkit) o `gpu-vulkan` (AMD/Intel + Vulkan SDK). Sin feature GPU, Whisper
sigue en CPU.

---

## 7. Validación rápida (opcional)

Desde la raíz del repo:

```bash
cargo test --workspace
cd apps/desktop && pnpm check
```

---

## 8. Build de desarrollo vs instalable

| Objetivo | Comando | Notas |
|---|---|---|
| Probar en caliente | `pnpm tauri dev` | Esto es lo que necesitas ahora (CPU) |
| Dev con Metal | `pnpm tauri dev -- --features gpu-metal` | Opcional; solo macOS |
| Empaquetar DMG local | `pnpm tauri build` | Sin firma: Gatekeeper avisará (CPU) |
| DMG con Metal | `pnpm tauri build -- --features gpu-metal` | Opcional; usuarios avanzados |
| Release firmado | Tag `v*` + CI | Lo haremos cuando toque el instalable (CPU) |

Salida típica del build:

```text
target/release/bundle/dmg/
```

Sin notarización, en Mac: clic derecho → **Abrir** la primera vez, o
Ajustes → Privacidad → permitir apps de desarrolladores no identificados.

---

## 9. Contacto / contexto del proyecto

- Repo: https://github.com/chrx3/atic
- Arquitectura y estado general: [`README.md`](../README.md)
- Notas técnicas macOS (fase 4): `apps/desktop/src-tauri/src/macos_notes.rs`
- Permisos TCC: `apps/desktop/src-tauri/Info.plist`
