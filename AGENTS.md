# Trabajo con agentes en Atic

## Qué es la app

- **La app es `apps/atic`:** la pill GPUI, Rust puro con
  [GPUI](https://crates.io/crates/gpui). Es lo que instalan el instalador de
  Windows (como `atic-pill.exe`) y la `.app` de macOS
  (`scripts/release-macos.sh`). Todo trabajo va ahí.
- **Ya no hay Tauri.** La versión de Atic es la del `Cargo.toml` de
  `apps/atic`; el ícono y los emojis están en `apps/atic/assets/`; los
  artefactos del updater los firma `crates/sign` (`atic-sign`, minisign).
  Las claves siguen en `~/.tauri/` por compatibilidad con lo ya publicado.
- **macOS:** la pill corre en Mac con su capa nativa (`platform/macos/`). Cómo
  compilarla, empaquetarla y qué falta: [`docs/MACOS_GPUI.md`](docs/MACOS_GPUI.md).
- `apps/web` es el sitio web; no es parte de la app.

## Mapa

| Ruta | Qué hay |
| --- | --- |
| `apps/atic/src` | La app: pill, notch, herramientas, Ajustes, Reuniones, Atic Code (`code/`), Flip, Pizarra… |
| `apps/atic/src/platform` | Lo que depende del sistema operativo: `win32/` y `macos/`, con el contrato en `platform/mod.rs` |
| `apps/atic/sidecar` | El agente de Atic Code en Node (`agent.mjs`), empaquetado con `pnpm run build` |
| `crates/*` | Lógica compartida: `core` (config, llavero, rutas), `audio`, `transcribe`, `summarize`, `capture`, `clipboard`, `agents`, `atic-mcp`, `atic-unix`… |
| `../gpui-m3` | Componentes Material 3 de Atic Code. **Repo aparte**, clonado al lado de `atic`; la pill lo usa por ruta relativa |
| `installer/`, `scripts/` | Instalador NSIS propio y scripts de compilación y release |

La pill tiene su **propio workspace y `Cargo.lock`**: no es miembro del
`Cargo.toml` de la raíz. Los comandos de `cargo` de la pill llevan
`--manifest-path apps/atic/Cargo.toml`.

## Prioridad: probar cambios pronto

El usuario itera sobre la pill y sus herramientas. Reduce el tiempo entre
editar y ver el resultado, pero sin compilar de más: **compilar la pill toma
entre 4 y 10 minutos y mucha memoria.**

1. Identifica qué cambia: la pill, un crate compartido, el sidecar MCP o el
   agente de Atic Code.
2. Junta los cambios relacionados y compila una sola vez. No encadenes
   compilaciones ni lances una mientras otra corre (una vez tumbó el PC).
3. Antes de compilar la pill, avisa qué necesita compilarse y por qué; si el
   usuario pidió que le preguntes, pregunta.
4. Deja el resultado corriendo para que el usuario lo pruebe y dile qué mirar.
5. Valida el área tocada; reserva las comprobaciones amplias para el cierre.

## Qué ejecutar según el cambio

| Área modificada | Ver el resultado | Validación dirigida |
| --- | --- | --- |
| Pill (`apps/atic/src`) | `scripts/pill-dev.ps1` | Tests de la pill con filtro del módulo |
| Crate compartido (`crates/*`) que usa la pill | `scripts/pill-dev.ps1` (recompila el crate) | `cargo test --locked -p <crate> <filtro>` |
| `crates/atic-mcp` | `cargo build -p atic-mcp --release` y reconectar el cliente | Tests de `atic-mcp`; probar la herramienta cambiada |
| Agente de Atic Code (`sidecar/`) | `pnpm --dir apps/atic/sidecar run build` | Probar un chat en Atic Code |
| `../gpui-m3` | `scripts/pill-dev.ps1` | Mirar el componente en la app; el ejemplo `gallery` de gpui-m3 |
| `apps/web` | Servidor web con HMR | Validaciones web al cerrar |
| Solo documentación | Revisar contenido y diff | Nada que compilar |

Los tests no actualizan el ejecutable y `cargo check` tampoco. No confundas
"compila", "pasaron los tests" y "lo probé en la app".

## Sesión de desarrollo (Windows)

- `scripts/pill-dev.ps1` compila la pill en modo dev (target
  `%LOCALAPPDATA%\atic-gpui`), cierra la pill que esté corriendo (la instalada
  o la de dev anterior) y abre la nueva. Llámalo con su ruta absoluta. La pill
  de dev usa los mismos datos y configuración que la instalada.
- `scripts/pill-dev.ps1 -Watch` recompila y reabre la pill al guardar (un
  cambio chico tarda ~20 s). Es para la terminal del usuario: si ya corre, no
  compiles la pill aparte, que el watch lo hace solo.
- Para afinar colores o números sin compilar: `apps/atic/tweaks.json`
  (ignorado por git), que la pill de dev relee al guardar. Formato y alcance en
  `src/tweak.rs`. Lo que quede bien se pasa al código y se borra del archivo.
- Log de la pill: `%APPDATA%\ciat\atic\data\logs\pill.<fecha>.log`.
- Antes de decir que algo "ya está corriendo", mira el proceso: la abierta
  puede ser la instalada (`atic-pill.exe`) y no la de dev (`pill-gpui.exe`).
- Puede haber otras sesiones de agentes trabajando en el mismo árbol.
  `pill-dev.ps1` cierra la pill de dev de ellas también; avisa cuando lo hagas.
- No uses el instalador ni scripts de release para iterar. No borres
  `target/` ni ejecutes `cargo clean` sin un problema concreto.

## Sesión de desarrollo (macOS)

- `bash scripts/pill-dev-mac.sh` compila la pill (target
  `~/Library/Caches/atic-gpui-target`), arma y firma `Atic Pill.app`, cierra
  la anterior y la abre. Un Mac de 8 GB aguanta `-j 4` (el script lo usa).
- `bash scripts/pill-dev-mac.sh --watch` recompila y reabre la pill al
  guardar. En debug compila sin sccache para usar el incremental (con sccache
  no se puede); si ya corre en la terminal del usuario, no compiles aparte.
- Log de la pill: `~/Library/Application Support/com.ciat.atic/logs/pill.<fecha>.log`.
- Los permisos (Accesibilidad, Monitoreo de entrada, Pantalla, Micrófono)
  quedan a nombre de la `.app` si se firma con la identidad estable de
  `~/.tauri/atic-signing-identity`; en Ajustes de la pill → Permisos se ven y
  se piden.

## Validación proporcional

- Tests de la pill, reusando el target de dev:

  ```powershell
  cargo test --manifest-path apps/atic/Cargo.toml --target-dir "$env:LOCALAPPDATA\atic-gpui" <filtro>
  ```

  Ejemplo: `meetings::`. Sin filtro corre todos (~560, menos de un segundo
  después de compilar).
- Crates: `cargo test --locked -p <crate> <filtro>`. Si el enlazado falla con
  LNK1104 por ruta larga, usa un `--target-dir` corto
  (`$env:LOCALAPPDATA\atic-ct`).
- **No corras `cargo fmt`:** ni la pill ni varios crates están formateados con
  rustfmt, y reescribiría miles de líneas ajenas. Sigue el estilo del archivo
  que tocas.
- Al corregir lógica, agrega o usa una prueba de regresión. Para cambios
  visuales, no inventes tests que copien la implementación: revisa en la app.
- Amplía las pruebas si cambias contratos compartidos: `crates/core/src/config.rs`
  es el `config.json` de todos los usuarios; cambiar un valor por defecto solo
  afecta a instalaciones nuevas o a campos que faltan.

## Diseño

- Revisa cada cambio de UI en tema claro y oscuro, y con los estilos de
  ventana de Apariencia (Atic, Material 3 Expressive, Liquid Glass). Opciones no elegidas,
  switches apagados y textos secundarios deben leerse.
- Toda vista raíz de ventana que dibuje componentes de gpui-m3 llama a
  `crate::theme::scope_m3(cx)` en su `render` y agrega `.children(m3_scope)`
  al final; si no, toma los colores de Atic Code.
- Reutiliza lo que ya existe: `theme.rs`, `hover.rs`, `anim.rs`, los helpers de
  `settings.rs` y los componentes de gpui-m3.
- Antes de tocar una animación o una transición entre vistas (vistazo → panel,
  la tira, el notch, la letra), lee la skill
  [`transiciones-fluidas`](.claude/skills/transiciones-fluidas/SKILL.md): sus
  reglas (un solo dueño por elemento, relevo con copias idénticas, progreso
  desde el estado, revisar los cuadros intermedios) y cómo revisar el resultado.

## Git y releases

- Se trabaja en `main` y `dev` (GitHub tiene `main` por defecto).
- Si cambias `../gpui-m3`, sube ese repo junto con `atic`: si no, nadie más
  puede compilar la pill.
- El instalador sale de `scripts/build-installer.ps1` y el release de
  `scripts/release-windows.ps1`. No los ejecutes ni publiques releases salvo
  que el usuario lo pida.

## Novedades (changelog)

Cada versión que se publica, para macOS o Windows, lleva sus **Novedades**:
la app las muestra en una ventana la primera vez que se abre después de
actualizar (`src/whats_new.rs`), y también desde «Novedades» en el ícono de la
barra de menús o la bandeja.

- Viven en `apps/atic/assets/changelog.json`, la versión más nueva
  primero. Cada entrada: `version`, `date` (`AAAA-MM-DD`) e `items` con
  `kind` (`new`, `improved` o `fixed`), `es` y `en`.
- Al subir la versión en `apps/atic/Cargo.toml`, agrega su entrada. Revisa los
  commits desde el último tag (`git log --format=%s vX.Y.Z..HEAD`) y escribe
  lo que nota el usuario, no el código: una frase corta, en español de Chile
  con tuteo y su par en inglés, sin punto final, a lo más ~6 por versión.
  Omite lo interno (chore, docs, tests, refactor).
- Mientras se trabaja, lo que va a salir puede ir anotándose en una entrada
  de la versión siguiente: la app no muestra versiones más nuevas que la
  instalada.
- Un test (`whats_new::tests::la_version_actual_tiene_sus_novedades`) y los
  dos scripts de release fallan si falta la entrada de la versión actual.
- Para revisar la ventana: `PILL_OPEN=novedades`. En la pill de dev no se
  abre sola.

## Comunicación y cuidado del trabajo

- Usa español de Chile (tuteo) en la UI, los comentarios y los mensajes.
- Escribe los archivos en UTF-8; PowerShell 5.1 deja mojibake en los acentos.
- Avisa cuando el cambio ya esté disponible para probar, aunque falten
  validaciones. Solo afirma haberlo visto funcionar si lo comprobaste.
- Al cerrar, resume qué cambió, qué comprobaste y qué quedó sin validar.
- Conserva cambios ajenos. No hagas push ni publiques releases salvo que el
  usuario lo haya pedido.
