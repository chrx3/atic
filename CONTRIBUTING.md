# Contribuir a Atic

Gracias por interesarte en el proyecto. Este documento resume cómo montar el entorno, validar cambios y abrir un PR.

Al participar, aceptas el [Código de Conducta](CODE_OF_CONDUCT.md).

## Español

El producto y el equipo son de **Chile**. UI, docs, errores, comentarios y
prompts van en español de Chile (**tuteo**: tú, tienes, graba, elige).

No uses voseo rioplatense (`vos`, `tenés`, `podés`, `grabá`, `elegí`, `usá`).

## Flujo

El desarrollo activo está en `main`. Las versiones publicadas son tags `v*`.

1. Haz fork (si no tienes acceso de escritura) o clona el repo.
2. Crea una rama desde `main` (`feat/…`, `fix/…`, `docs/…`).
3. Haz commits pequeños y claros.
4. Abre un pull request hacia `main`.

Antes de pedir revisión, asegúrate de que las comprobaciones locales pasen.
CI corre `frontend` y `rustfmt` en cada PR; los jobs pesados (clippy + tests
completos, build) siguen siendo `workflow_dispatch`, y los push a `main` no
compilan instaladores.

## Requisitos

Los detalles de toolchain están en [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)
(Windows) y en [`docs/MACOS.md`](docs/MACOS.md) (macOS). Resumen:

- Rust stable
- Node.js 22+ y pnpm 10+
- CMake y LLVM/libclang (los necesita `whisper-rs` / whisper.cpp)
- En Windows: Visual Studio Build Tools (C++) y WebView2

## Desarrollo

```bash
cd apps/desktop
pnpm install
pnpm tauri dev
```

## Validación local

El ciclo rápido (HMR, `cargo ck`, tests del crate que tocaste) está en
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) → «Ciclo rápido». Antes de subir,
esto tiene que estar limpio:

Desde la raíz del repo, según el área que cambió:

```bash
pnpm verify:desktop
pnpm verify:web
pnpm verify:rust
```

Para validar frontend desktop, sitio web y formato de Rust juntos:

```bash
pnpm verify
```

Antes de un release, usa `pnpm verify:all`: también prepara los sidecars y
ejecuta clippy y tests del workspace Rust. Requiere las dependencias de ambas
apps y el entorno nativo descrito en las guías de desarrollo.

## PRs

- Describe el problema y la solución en pocas líneas.
- Si el cambio es grande, abre un issue primero.
- Incluye capturas o pasos de reproducción si el cambio es de UI.
- No subas secretos, claves, `.env`, ni artefactos de build.
- No hace falta regenerar lockfiles salvo que el cambio lo requiera.

## Licencia

Al contribuir, aceptas que tus aportes se licencien bajo la [MIT License](LICENSE) del proyecto (copyright chrx3).
