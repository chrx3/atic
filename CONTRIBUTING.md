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
CI y el workflow de instaladores son solo `workflow_dispatch` (los tags y
los push a `main` no disparan builds).

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

Rust (desde la raíz del repo):

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

## PRs

- Describe el problema y la solución en pocas líneas.
- Si el cambio es grande, abre un issue primero.
- Incluye capturas o pasos de reproducción si el cambio es de UI.
- No subas secretos, claves, `.env`, ni artefactos de build.
- No hace falta regenerar lockfiles salvo que el cambio lo requiera.

## Licencia

Al contribuir, aceptas que tus aportes se licencien bajo la [MIT License](LICENSE) del proyecto (copyright chrx3).
