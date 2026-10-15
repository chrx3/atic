# Contribuir a Atic

Gracias por interesarte en el proyecto. Este documento resume cómo montar el entorno, validar cambios y abrir un PR.

Al participar, aceptas el [Código de Conducta](CODE_OF_CONDUCT.md).

## Español

El producto y el equipo son de **Chile**. UI, docs, errores, comentarios y
prompts van en español de Chile (**tuteo**: tú, tienes, graba, elige).

No uses voseo rioplatense (`vos`, `tenés`, `podés`, `grabá`, `elegí`, `usá`).

## Qué es la app

La app es la **pill GPUI** (`prototypes/pill-gpui`), escrita en Rust con
[GPUI](https://crates.io/crates/gpui). `apps/desktop` (Tauri) es legado: solo
se mantiene para el build de Mac mientras la pill se porta, y no recibe
funciones nuevas. El mapa del repo y las reglas de trabajo están en
[`AGENTS.md`](AGENTS.md).

## Flujo

El desarrollo activo está en `main`. Las versiones publicadas son tags `v*`.

1. Haz fork (si no tienes acceso de escritura) o clona el repo.
2. Crea una rama desde `main` (`feat/…`, `fix/…`, `docs/…`).
3. Haz commits pequeños y claros.
4. Abre un pull request hacia `main`.

## Requisitos

- Rust stable.
- **Windows:** Visual Studio Build Tools con C++.
- **macOS:** Xcode. La pill todavía no compila en Mac; ver
  [`docs/MACOS_GPUI.md`](docs/MACOS_GPUI.md).
- Node.js 22+ y pnpm 10+, solo para el agente de Atic Code
  (`prototypes/pill-gpui/sidecar`) y el sitio web (`apps/web`).
- El repo [`gpui-m3`](https://github.com/chrx3/gpui-m3) clonado **al lado**
  de `atic` (la pill lo usa por ruta relativa):

  ```bash
  git clone https://github.com/chrx3/atic.git
  git clone https://github.com/chrx3/gpui-m3.git
  ```

## Desarrollo

En Windows, desde la raíz del repo:

```powershell
powershell -File scripts/pill-dev.ps1
```

Compila la pill en modo dev, cierra la que esté corriendo (también la
instalada) y abre la nueva con tus datos de siempre. Para probar otro cambio,
córrelo de nuevo. Para volver a la instalada, abre «Atic» desde el menú Inicio.

Si cambias el agente de Atic Code:

```bash
pnpm --dir prototypes/pill-gpui/sidecar install
pnpm --dir prototypes/pill-gpui/sidecar run build
```

## Validación local

Antes de subir, según el área que cambió:

```powershell
# La pill (reusa el target de pill-dev.ps1)
cargo test --manifest-path prototypes/pill-gpui/Cargo.toml --target-dir "$env:LOCALAPPDATA\atic-gpui"

# Un crate compartido
cargo test --locked -p atic-core
```

- Prueba el cambio en la app, no solo los tests. Si es de UI, revísalo en
  tema claro y oscuro.
- Ni la pill ni varios crates están formateados con rustfmt: no corras
  `cargo fmt` sobre ellos (reescribiría miles de líneas ajenas). Sigue el
  estilo del archivo que tocas.
- Si cambias `gpui-m3`, sube ese repo también.

El CI de GitHub todavía valida el frontend de Tauri y `rustfmt` del workspace
de la raíz (hoy en rojo por lo anterior); no compila la pill.

## Instalador y releases

`scripts/build-installer.ps1` arma el instalador de Windows (pill, sidecars y
NSIS) y `scripts/release-windows.ps1` lo firma y publica. Los releases los
hace el mantenedor; no hace falta correrlos para un PR.

## PRs

- Describe el problema y la solución en pocas líneas.
- Si el cambio es grande, abre un issue primero.
- Incluye capturas o pasos de reproducción si el cambio es de UI.
- No subas secretos, claves, `.env`, ni artefactos de build.
- No hace falta regenerar lockfiles salvo que el cambio lo requiera.

## Licencia

Al contribuir, aceptas que tus aportes se licencien bajo la [MIT License](LICENSE) del proyecto (copyright chrx3).
