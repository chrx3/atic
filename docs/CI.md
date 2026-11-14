# CI y releases en GitHub Actions

Tres piezas:

- **`.github/workflows/ci.yml`**: los tests, en cada push y PR a `main` y
  `dev`.
- **`.github/workflows/release.yml`**: el release de macOS y Windows, de
  punta a punta, como borrador. Reemplaza a `release-macos.yml`.
- **`PILL_SELFTEST=arranque`**: la autoprueba de arranque de la pill
  (`apps/atic/src/selftest.rs`), que el release corre con el paquete ya armado
  antes de publicar.

La forma es la del release de t3code (`pingdotgg/t3code`): resolver el
commit, preflight, tests, compilar en matriz con un «Verify» que abre lo
empaquetado, y publicar solo si todo pasó.

## CI (`ci.yml`)

Jobs en paralelo; un push nuevo a la misma rama cancela el anterior (cuando
lo llama el release, no se cancela). No corre si el cambio es solo de
`docs/`, `*.md` o `apps/web/`.

| Job | Runner | Qué hace |
| --- | --- | --- |
| Crates compartidos | `ubuntu-latest` | `cargo test --locked -p atic-core -p atic-calc -p atic-agents -p atic-mcp -p atic-sign` |
| atic-kvm | `ubuntu-latest` | `cargo test --locked -p atic-kvm` (en los tests la capa del sistema no hace nada, así que corre en Linux) |
| Sidecar y scripts | `ubuntu-latest` | `pnpm test` y `pnpm run build` del sidecar; `node --test scripts/*.test.mjs` |
| Pill (macOS) | `macos-26` | `cargo test --locked --manifest-path apps/atic/Cargo.toml -j 3` |
| Pill (Windows) | `windows-latest` | lo mismo con `-j 4`, después de cargar `scripts/win-env.ps1` (libclang para whisper-rs-sys) |

- Los jobs de la pill, que son los caros, corren en PRs, en pushes a `main` y
  desde el release; un push a `dev` corre solo los demás.

- La pill usa `macos-26` (no `macos-latest`) para compilar con el mismo
  Xcode que el release. Si `macos-latest` pasa a ser 26, da lo mismo.
- Los tests de la pill corren con `CARGO_PROFILE_DEV_DEBUG=0` e incremental
  apagado, y su target va fuera del repo con ruta corta (`../target-pill`,
  por LNK1104 en Windows). La caché es `Swatinem/rust-cache`.
- La matriz de la pill es `fail-fast`: si una plataforma falla, la otra se
  cancela para no gastar minutos de macOS.
- El test `mac_smoke` llama a las APIs nativas de macOS en el runner (Vision,
  Core Audio, NSWorkspace). Si un runner no tiene algo que el test espera
  (por ejemplo, ninguna app con ventanas), falla ahí y no en tu Mac.

### gpui-m3 fijado

La pill depende de `../gpui-m3` por ruta. En CI lo clona la acción
`.github/actions/gpui-m3` al lado del repo, en el commit que dice
**`apps/atic/gpui-m3.rev`** (una línea con el hash completo; las que empiezan
con `#` son comentarios). Así un build de hoy y uno de dentro de un mes usan
el mismo gpui-m3 aunque `main` avance.

Al cambiar gpui-m3:

```bash
git -C ../gpui-m3 push
git -C ../gpui-m3 rev-parse HEAD   # pegar en apps/atic/gpui-m3.rev
```

Si el commit no está en GitHub, el job falla con «¿Falta subir gpui-m3?». Si
está pero no en el `main` de gpui-m3 (por ejemplo, tras un force-push), falla
con «no está en su main»: un commit fuera de `main` puede desaparecer cuando
GitHub limpie el repo. Pon en `gpui-m3.rev` un commit de `main`.
Tu compilación local sigue usando lo que tengas en `../gpui-m3`, fijado o no.

### Versiones fijadas

Para que un build de hoy y uno de dentro de un mes salgan iguales:

| Qué | Dónde | Cómo subirla |
| --- | --- | --- |
| Rust | `RUST_TOOLCHAIN` en `ci.yml` y `release.yml` (hoy `1.100.0`, la de `rustc --version` en el Mac) | Cambiar las dos |
| Xcode | `XCODE_VERSION` en `release.yml` y en el paso «Xcode» de `ci.yml` (hoy `26`: el 26.x más nuevo del runner) | Una versión completa (`26.1`) lo fija del todo; si no está, el paso lista las que hay |
| NSIS | `NSIS_VERSION` en `release.yml` (`3.11`, por Chocolatey) | Cambiarla |
| Acciones | Por SHA, con la versión en un comentario (`# v4.4.0`) | `git ls-remote --tags https://github.com/<acción>.git` y pegar el SHA del tag |
| gpui-m3 | `apps/atic/gpui-m3.rev` | Ver arriba |

No hay `rust-toolchain.toml`: fijaría también la versión de los equipos de
desarrollo (y bajaría otra toolchain en cada uno).

## Release (`release.yml`)

```
resolver ─► preflight ─► tests (ci.yml) ─┬─► macos (build + Verify arm64) ─► macos-x64 (Verify Intel) ─┐
                                         └─► windows (build + Verify x64) ─────────────────────────────┴─► publicar
```

| Job | Runner | Qué hace |
| --- | --- | --- |
| resolver | ubuntu | Commit (`github.sha`, el que usan todos los checkouts), versión de `apps/atic/Cargo.toml`, tag `vX.Y.Z` y si se publica (solo con un push de tag) |
| preflight | ubuntu | Tag = versión; el commit del tag está en `main`; el release del tag no está publicado y, si hay borrador, su `latest.json` sirve para fusionar; la versión tiene su entrada en `changelog.json`; gpui-m3 en la revisión fijada existe y está en su `main`; tests de los scripts de release |
| tests | — | Llama a `ci.yml` (los mismos jobs de arriba) |
| macos | `macos-26` | Firma con «Atic Dev» (`.p12` de los secrets), `build-pill-mac.sh` (universal), `package-mac.sh` (`.app.tar.gz` y `.dmg`). **Verify**: desempaqueta el `.tar.gz`, `codesign --verify --deep --strict`, `hdiutil verify`, comprueba que el `.app` trae `Resources/sidecar/agent.mjs`, `MacOS/atic-mcp` y `MacOS/atic-apple`, que los tres binarios son universales (`lipo -verify_arch arm64 x86_64`), corre `scripts/selftest.sh` y anota el sha256 de lo verificado |
| macos-x64 | `macos-15-intel` | **Verify** del mismo `.app.tar.gz` en un Mac Intel de verdad (la pill pide macOS 13 o más nuevo). Hace checkout solo de `scripts/`, así la autoprueba no puede usar el `agent.mjs` del repo |
| windows | `windows-latest` | `win-env.ps1` y `build-installer.ps1` (NSIS en la versión fijada, por Chocolatey). **Verify**: instala en silencio (`/S /NORUN /D=…`; `/NORUN` no abre la pill) en una carpeta aparte esperando solo al instalador (5 min como mucho), comprueba los archivos, corre `scripts/selftest.ps1` con la pill instalada y anota el sha256 del instalador |
| publicar | ubuntu; environment `release` solo con un tag | Comprueba que los archivos son los del Verify (sha256), compila `atic-sign` antes de que la clave toque el disco, firma para el updater el `.app.tar.gz` (`darwin`) y el instalador (`windows-x86_64`), verifica cada `.sig` (`atic-sign --verify`) contra la clave pública de la app (`PUBKEY` de `updater.rs`), arma `latest.json` con `darwin-aarch64`, `darwin-x86_64` y `windows-x86_64`, y con un tag deja todo en un release **borrador** |

Lo que conserva de `release-macos.yml` y de los commits `1237d65` y `6634ffb`:

- **Firma estable obligatoria**: sin el `.p12` el job de macOS se detiene antes
  de compilar; nunca firma ad-hoc (invalidaría los permisos al actualizar).
- **Revisión antes de publicar**: solo borradores. El updater lee
  `releases/latest`, así que nadie recibe nada hasta que lo publiques a mano.
  Si el release del tag ya está publicado, el preflight falla antes de
  compilar; si alguien lo publica mientras se compila, publicar falla sin
  tocarlo (los archivos quedan en el artefacto `atic-release`). El environment
  `release` pide la aprobación de su revisor antes de firmar.
- **La clave del updater solo con un tag**: vive en el environment `release`
  (solo tags `v*` y `main`). *Run workflow* no entra a ese environment: firma
  con una clave de prueba creada en el job (`atic-sign --nueva-clave`), que
  muere con el runner; lo firmado no lo acepta ninguna instalación.
- **No perder plataformas del updater**: si el release ya existe, parte de su
  `latest.json` (`scripts/updater-manifest.mjs`, con tests). Si GitHub falla
  al listar los releases o al bajar ese archivo, se detiene en vez de generar
  un manifiesto con menos plataformas.

Lo nuevo: una sola corrida arma macOS **y** Windows, y `latest.json` sale de
una vez con las tres plataformas, sin fusionar entre dos máquinas.

### Arquitecturas

- **macOS**: un solo `.app` universal (Apple Silicon + Intel), compilado en
  `macos-26` (arm64, con `lipo`), como hasta ahora. Las dos entradas del
  updater (`darwin-aarch64` y `darwin-x86_64`) apuntan al mismo `.tar.gz`. No
  se compila aparte para Intel: duplicaría la compilación más cara (LTO
  completo) y el universal ya lo cubre. Sí se *verifica* en un runner Intel.
- **Windows x64**: el instalador de siempre.
- **Windows arm64**: todavía no. El instalador NSIS y `latest.json` son solo
  x64, el updater no tiene la entrada `windows-aarch64`, y los runners
  `windows-11-arm` en repos privados son de pago. La versión x64 corre
  emulada en Windows on ARM. Para sumarlo: compilar con
  `--target aarch64-pc-windows-msvc`, un instalador propio y la entrada en
  `latest.json`.

### Cómo correrlo

- **Release**: sube la versión en `apps/atic/Cargo.toml`, agrega sus Novedades
  y, si cambió gpui-m3, actualiza `apps/atic/gpui-m3.rev`. Luego:

  ```bash
  git tag v0.6.2 && git push origin v0.6.2
  ```

  Queda un borrador en Releases con `Atic.app.tar.gz` y su `.sig`, el `.dmg`,
  `Atic_<versión>_x64-setup.exe` y su `.sig`, y `latest.json`. Revísalo y
  publícalo a mano (marcándolo como *latest*).
- **Probar sin release**: Actions → *Release* → *Run workflow*. Hace todo
  menos tocar releases y firma con una clave de prueba; los archivos quedan
  en el artefacto `atic-prueba` (7 días), que **no** sirve para publicar. El
  workflow tiene que estar en la rama por defecto para que aparezca el botón.
- **Release manual o CI, no los dos.** `scripts/release-macos.sh` y
  `scripts/release-windows.ps1` siguen sirviendo para publicar desde una
  máquina, pero son la alternativa al workflow, no un complemento. Ojo: los
  dos suben el tag, y un tag `v*` dispara `release.yml`:
  - si el script ya publicó el release, el preflight del workflow falla
    («ya está publicado») y no toca nada; es esperable;
  - si el workflow llega antes que la publicación del script, deja un
    borrador con su propio `latest.json`; el script después sube encima
    (`--clobber`) y fusiona con `scripts/updater-manifest.mjs`. Revisa el
    release antes de darlo por bueno, o cancela el run en Actions.

  Para que el workflow publique, sube solo el tag
  (`git push origin vX.Y.Z`) y no corras los scripts con `-Publish`.

### Secrets y variables

| Secret | Dónde | Qué es | Lo usa |
| --- | --- | --- | --- |
| `ATIC_UPDATER_KEY` | environment `release` | Contenido de `~/.tauri/atic-updater.key` | publicar (solo con un tag) |
| `ATIC_UPDATER_PASSWORD` | environment `release` | Contenido de `~/.tauri/atic-updater.password` | publicar (solo con un tag) |
| `MACOS_SIGNING_P12` | repo | El certificado «Atic Dev» exportado como `.p12`, en base64 | macos |
| `MACOS_SIGNING_P12_PASSWORD` | repo | La contraseña de ese `.p12` | macos |
| `MACOS_SIGNING_IDENTITY` | repo | Opcional: el nombre del certificado si no es «Atic Dev» | macos |

| Variable (Settings → Variables) | Qué hace |
| --- | --- |
| `SELFTEST_OMITE_WINDOWS` | `ventana`: en el runner de Windows de GitHub la ventana de GPUI no termina de abrir y la autoprueba vencía su plazo de 120 s (se comprueba el resto) |
| `SELFTEST_OMITE_INTEL` | Opcional. Lo que no hace la autoprueba en el Mac Intel; `ventana` si ese runner no tiene Metal (gpui termina el proceso al abrir la ventana) |

La clave del updater solo entra al job `publicar` de un tag, con la
aprobación del revisor; el certificado de Mac, solo al job `macos`. Cómo
exportar el `.p12`: [`MACOS.md` §4](MACOS.md#4-release-desde-github-actions).

### Configurar en GitHub (una vez)

1. **Environment `release`** (Settings → Environments → *New environment*):
   revisor requerido `chrx3`; *Deployment branches and tags* → *Selected
   branches and tags* → la rama `main` y los tags `v*`.

   ```bash
   id="$(gh api users/chrx3 --jq .id)"
   gh api -X PUT repos/chrx3/atic/environments/release --input - <<JSON
   {
     "reviewers": [{ "type": "User", "id": $id }],
     "deployment_branch_policy": { "protected_branches": false, "custom_branch_policies": true }
   }
   JSON
   gh api -X POST repos/chrx3/atic/environments/release/deployment-branch-policies -f name=main -f type=branch
   gh api -X POST repos/chrx3/atic/environments/release/deployment-branch-policies -f name='v*' -f type=tag
   ```

2. **La clave del updater, al environment** (y fuera del repo):

   ```bash
   gh secret set ATIC_UPDATER_KEY -R chrx3/atic --env release < ~/.tauri/atic-updater.key
   gh secret set ATIC_UPDATER_PASSWORD -R chrx3/atic --env release < ~/.tauri/atic-updater.password
   gh secret delete ATIC_UPDATER_KEY -R chrx3/atic
   gh secret delete ATIC_UPDATER_PASSWORD -R chrx3/atic
   ```

3. **Borrar los secrets viejos de Tauri**, que ya no usa nada:

   ```bash
   gh secret delete TAURI_SIGNING_PRIVATE_KEY -R chrx3/atic
   gh secret delete TAURI_SIGNING_PRIVATE_KEY_PASSWORD -R chrx3/atic
   ```

4. **Certificado de Mac** (quedan en el repo):

   ```bash
   base64 -i AticDev.p12 | gh secret set MACOS_SIGNING_P12 -R chrx3/atic
   gh secret set MACOS_SIGNING_P12_PASSWORD -R chrx3/atic   # pide la contraseña
   ```

   *Pendiente opcional*: cuando exportes el `.p12`, moverlos a un environment
   propio (p. ej. `firma-mac`, solo `main` y tags `v*`, sin revisor para no
   aprobar dos veces) y agregar `environment: firma-mac` al job `macos` de
   `release.yml`. Hoy ese job no declara environment.

5. **Comprobar**: `gh api repos/chrx3/atic/environments/release` muestra el
   revisor y las reglas; `gh secret list -R chrx3/atic` ya no lista
   `ATIC_UPDATER_*` ni `TAURI_*`, y `gh secret list -R chrx3/atic --env release`
   lista los dos del updater.
6. **Primera corrida**: *Run workflow* desde `main` (no pide aprobación ni usa
   la clave). Confirma ahí que `publicar` corre **sin** environment: el job
   usa `environment: ${{ publish && 'release' || '' }}` y un nombre vacío
   debería dejarlo sin environment; si GitHub lo rechaza, hay que separar
   publicar en dos jobs.

## Autoprueba de arranque (`PILL_SELFTEST=arranque`)

```bash
# macOS, con la .app empaquetada (también la de dev)
bash scripts/selftest.sh "$HOME/Library/Caches/atic-gpui-target/debug/Atic Pill.app/Contents/MacOS/atic-pill"
```

```powershell
# Windows, con la pill instalada
powershell -File scripts/selftest.ps1 -Exe "$env:LOCALAPPDATA\Atic\atic-pill.exe"
```

La pill arranca, comprueba lo esencial, escribe un JSON y termina: código 0
si todo salió bien, 1 si algo falló, 2 si se pasó del plazo.

| Comprobación | Qué mira |
| --- | --- |
| `config` | Carpetas de datos y `config.json` (solo lee; si está roto falla, porque la pill arrancaría con los valores por defecto sin avisar) |
| `i18n` | Los textos en español e inglés y un ícono embebido |
| `novedades` | La entrada de esta versión en `changelog.json` |
| `nativo: …` | macOS: las de `mac_smoke.rs` (apps e íconos, apariencia y permisos, sistema, privacidad, Vision). Windows: íconos, ventanas abiertas, apariencia, sistema, privacidad y OCR (este último solo avisa: depende del idioma del equipo) |
| `sidecar` | Un agente de Atic Code propio, por stdio, que conteste `ping`. Si está exigida, tiene que ser el `agent.mjs` empaquetado (junto al exe o en `Contents/Resources`), no el del repo |
| `mcp` | `atic-mcp --editor` que se presente y liste herramientas |
| `overlay`, `ventana` | Una ventana chica y transparente con el overlay nativo, que dibuje |

Variables:

- `PILL_SELFTEST_SALIDA`: dónde va el JSON (por omisión, `selftest.json` en
  la carpeta de logs de Atic). También sale por stdout.
- `PILL_SELFTEST_PLAZO`: segundos antes de rendirse (120).
- `PILL_SELFTEST_EXIGE`: lo opcional que debe estar (`sidecar,mcp,ventana`):
  sin eso, «no hay Node» o «no hay atic-mcp» queda omitido. Los scripts lo
  exigen por omisión.
- `PILL_SELFTEST_OMITE`: lo que no se hace. Si un runner no puede abrir
  ventanas (Windows sin GPU, o un Mac Intel sin Metal, donde gpui termina el
  proceso con `exit`), `PILL_SELFTEST_OMITE=ventana` en el Verify es la salida
  mientras se resuelve. En el Mac Intel se pone sin tocar el workflow, con la
  variable `SELFTEST_OMITE_INTEL=ventana` del repo.

El JSON se escribe también antes de cada comprobación, con `"en_curso"` y
`"ok": false`: si la pill muere sin terminar, queda lo que alcanzó y qué
estaba haciendo, y los scripts fallan diciendo que el informe es parcial.

Por qué puede correr al lado de la pill abierta: sale antes del log, del
candado de instancia única y de `prepare_dev_start` (no cierra la instalada),
no escribe `config.json`, no registra atajos ni abre la bandeja, no pide
permisos (solo consulta su estado), y el agente y `atic-mcp` que levanta son
suyos, por stdio (no toca `code-agent.json` ni el agente de la pill). La
única huella visible es una ventana transparente de 220×40 en la esquina
durante menos de un segundo.

## Siguiente paso: pruebas de humo de interfaz en equipos propios

*Diseño, sin implementar.* La autoprueba demuestra que la pill arranca, no
que se vea bien ni que responda al mouse. Eso necesita una sesión de usuario
real con permisos, que los runners de GitHub no tienen: Accesibilidad,
Grabación de pantalla y Monitoreo de entrada en macOS, y una GPU y un
escritorio interactivo en Windows.

**Runners.** Dos self-hosted, registrados en el repo con etiquetas propias:

- `atic-mac`: este Mac, con el runner corriendo como **LaunchAgent** del
  usuario (no LaunchDaemon), así hereda la sesión gráfica y los permisos TCC
  ya dados a «Atic Pill» (firma estable «Atic Dev»).
- `atic-win`: el PC Windows, con el runner como **tarea programada al iniciar
  sesión** del usuario (no como servicio: un servicio corre en la sesión 0,
  sin escritorio).

Los dos con un usuario dedicado o, si es el del día a día, con la regla de
no correr mientras se trabaja (ver abajo).

**Disparo.** Un workflow `ui-smoke.yml` con `workflow_dispatch` y
`workflow_run` después de `release.yml` (sobre el artefacto ya firmado), en
`runs-on: [self-hosted, atic-mac]` y `[self-hosted, atic-win]`. Nunca en PRs
de forks: un runner propio ejecuta código del PR con los datos del usuario.

**Aislamiento.** Correr la pill con un perfil de datos aparte (una variable
como `ATIC_DATA_DIR` en `atic_core::AppDirs`, que hoy no existe) para no tocar
el `config.json`, el historial ni el llavero del usuario, y con `PILL_DEV=1`
para que conviva con la pill instalada (candado propio, sin atajos).

**Qué prueba.** Reusar lo que ya hay para revisar sin compilar:
`PILL_OPEN=settings|meetings|novedades|clipboard`, `PILL_OPEN_SEQ` (abre las
herramientas del notch una tras otra), `CODE_ALONE=1 CODE_DEMO=…` para Atic
Code, y `PILL_FPS` para medir. Por cada pantalla:

1. Abrirla, esperar a que el log diga que dibujó y capturarla (macOS:
   `screencapture -l <id de ventana>`; Windows: `PrintWindow` con
   `SetProcessDPIAware`, como en las capturas de Atic Code).
2. Comparar contra capturas de referencia guardadas en el repo, con tolerancia
   (diferencia de píxeles por región), en tema claro y oscuro y en los estilos
   de Apariencia.
3. Para interacción: un modo `PILL_SELFTEST=interfaz` que exponga un canal
   local (como el hub de Atic Code) para pedir «abre X», «haz clic en el
   elemento con id Y» y «dime qué hay en pantalla», en vez de mover el mouse
   real.

**Resultado.** Subir las capturas y las diferencias como artefacto, y un
resumen en el job. Falla solo con diferencias sobre el umbral; las capturas de
referencia se actualizan a mano con un `workflow_dispatch` que las regenera.

**Riesgos.** El equipo tiene que estar encendido, con la sesión abierta y sin
nadie usándolo (el job puede esperar a que no haya actividad de teclado o
mouse en N minutos). Una actualización de macOS o de Windows puede cambiar
las capturas sin que Atic cambie.
