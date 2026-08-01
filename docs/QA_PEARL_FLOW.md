# QA de cierre — Pearl Flow

Fecha de validación: 2026-07-31

Rama: `codex/floating-ui-v2`

Binario validado: `target/release/atic-desktop.exe`

Comando de build: `pnpm --dir apps/desktop tauri build --no-bundle`

Este documento registra el cierre técnico y visual de Pearl Flow. La interfaz
anterior se mantiene como rollback hasta que Christian complete su propia QA.

## Matriz del goal

| # | Requisito | Resultado y evidencia |
|---|---|---|
| 1 | Pill circular en reposo y extensible | La pill usa 64×64 px en reposo, puede configurarse como circular o compacta y conserva los estados extendidos para actividad, progreso y confirmaciones. La opción se expone en Ajustes. |
| 2 | Herramientas persistentes nacidas de la pill | `FloatingWorld` mantiene una ventana por herramienta, la coloca alrededor de la pill, permite arrastrarla y restaura solo las superficies visibles desde `floating-world.json`. Abrir otra herramienta no cierra las anteriores. |
| 3 | Proximidad y grupos líquidos | La atracción nativa une superficies al quedar a 26 px lógicos o menos y deja un gap visual de 7 px. Un grupo se mueve como conjunto. `Separar del grupo` y Alt+arrastrar desprenden un miembro y persisten esa decisión. Las pruebas Rust cubren merge, movimiento, separación y reagrupación. |
| 4 | Agentes autosuficiente | `FloatingAgents` crea y alterna sesiones, descubre proveedor/agente/modelo/esfuerzo, envía el primer turno y los siguientes, gestiona permisos, adjuntos, capturas, drop y dictado, muestra streaming/errores y permite cancelar o reintentar. El smoke WebView2 ya recibió `ATIC_OK` y `ATIC_CONTINUA` en una misma sesión sin abrir `/agents`. Launcher y portapapeles priorizan `pearl-tool-agents`. |
| 5 | Aplicación principal como centro de control | La ventana principal contiene Inicio, Reuniones, Textos, Capturas, Agentes y Ajustes. Inicio presenta estado y accesos directos; Textos reúne guardados, portapapeles y dictado; Agentes monta el mismo componente funcional de la superficie flotante. |
| 6 | Navegación Liquid UI adaptable | `AppShell` usa `LiquidSkin`: rail lateral en espacios anchos y dock inferior hasta 51.999 rem. El binario release se verificó en su ancho inicial de 820 px con dock inferior, selección clara y Ajustes sin recorte horizontal. |
| 7 | Pearl Flow, temas y accesibilidad | Los WebViews reciben el tema desde la configuración nativa y `config-changed`; claro, oscuro y sistema cambian en vivo en la principal, herramientas y mundos. Hay foco visible, tabs con roving tabindex, teclado, nombres accesibles y rutas rápidas para `prefers-reduced-motion`. |
| 8 | Compositor WebView2 | Se validó el compositor híbrido real en WebView2: una `LiquidWorld` click-through y no activable por monitor, ventanas DOM interactivas encima, foco estable, z-order correcto, dos monitores, 125 % DPI y SDF muy por debajo de 20 ms p95. |
| 9 | Preservación y rollback | No se eliminó `/pill`, `/agents`, la ventana Tauri `agents` ni `show_agents_window`. No hay migración destructiva de datos. La ruta principal de producción es `/pill-v2`, pero puede volver a `/pill` con un cambio de configuración. |
| 10 | Dynamic Island posterior sin renderer duplicado | La futura colocación en borde debe añadirse como adaptador de posición sobre `FloatingWorld`, `LiquidSkin` y el renderer SDF existentes. No se creó un segundo compositor. |

## Evidencia del compositor WebView2

La validación se realizó sobre el ejecutable release exacto, sin servidor de
desarrollo ni HMR.

| Prueba | Resultado observado |
|---|---|
| Ventanas de mundo | Dos mundos: `monitor-0` en `-1536,0,0,816` y `monitor-1` en `0,0,1536,816`. |
| DPI | Ambos monitores reportaron 120 DPI, equivalentes a 125 %. La geometría visual y las ventanas interactivas permanecieron alineadas. |
| Estilos nativos | Ambos mundos conservaron `WS_EX_TRANSPARENT`, `WS_EX_LAYERED` y `WS_EX_NOACTIVATE`, además de topmost. |
| Hit testing | Puntos dentro de herramientas llegaron a su WebView. Puntos vacíos atravesaron el mundo y llegaron al navegador o escritorio subyacente. Ninguna zona vacía devolvió la ventana del compositor. |
| Foco | Mover o actualizar la piel no activó el compositor. El input de Agentes conservó foco durante movimientos de otras superficies. |
| Z-order | Pill y herramientas permanecieron delante de los mundos tras ciclos de foco y movimiento. |
| Cruce de monitor | Una herramienta apareció una sola vez en el mundo destino y desapareció del contorno del origen. |
| Rendimiento SDF | `monitor-0`: media 0.01–0.02 ms, p95 0.10 ms. `monitor-1`: media 1.17–1.63 ms, p95 2.20–3.20 ms. Con Agentes: media 0.96 ms, p95 2.80 ms. |
| CPU en reposo | Sin actividad apreciable en el muestreo de varios segundos. |

### Memoria

El proceso completo ronda 2.05 GB después del arranque porque Atic precarga dos
modelos Whisper ya instalados: `ggml-large-v3-turbo.bin` (~1549.3 MB) y
`ggml-small.bin` (~465 MB). `VirtualQueryEx` mostró dos regiones privadas de
aproximadamente 1548.7 MB y 464.4 MB, coincidentes con esos archivos. Antes de
esa precarga el proceso usaba ~17.6 MB privados y después de crear el primer
mundo ~24.5 MB. Por tanto, el compositor añade pocos MB; no se atribuye la
memoria de los modelos a Pearl Flow.

## QA visual del release

- Inicio abre con jerarquía clara y dock Liquid inferior en el tamaño inicial.
- Ajustes ocupa el ancho disponible, conserva scroll y no se recorta detrás del
  dock.
- Cambiar Sistema → Claro actualiza a la vez la principal, Agentes, Reuniones y
  la piel SDF; Claro → Oscuro → Sistema también se sincroniza sin reiniciar.
- La forma configurada en Ajustes se refleja en la pill; reposo circular sigue
  siendo el valor predeterminado.
- Agentes y Reuniones coexistieron abiertos alrededor de la pill y con contenido
  interactivo separado de la piel.

## Verificación automatizada

Comandos ejecutados sobre esta rama:

```powershell
pnpm --dir apps/desktop check
cargo fmt --all --check
cargo test -p atic-desktop floating_world -- --nocapture
npx --yes tsx apps/desktop/src/lib/floating-v2/liquid/engine.test.ts
npx --yes tsx apps/desktop/src/lib/floating-v2/world.test.ts
git diff --check
pnpm --dir apps/desktop tauri build --no-bundle
```

Resultados:

- Svelte: 0 errores y 0 warnings.
- Autofixer Svelte: todos los componentes modificados sin incidencias.
- Rust fmt: correcto.
- `floating_world`: 3 pruebas aprobadas, 0 fallidas.
- Engine SDF y modelo de mundo TypeScript: aprobados.
- Diff: sin errores de whitespace.
- Build Tauri release: correcto.

## Rollback seguro

La migración es reversible y no elimina datos ni interfaces:

1. Cambiar la URL de la ventana `pill` en
   `apps/desktop/src-tauri/tauri.conf.json` de `/pill-v2` a `/pill` restaura la
   pill anterior.
2. La ruta `/agents`, la ventana oculta `agents` y el comando
   `show_agents_window` permanecen compilados para recuperar la consola previa.
3. `floating-world.json` solo guarda geometría, visibilidad y grupos. Eliminar
   ese archivo, con la aplicación cerrada y tras respaldo, restablece únicamente
   la disposición flotante; no toca reuniones, textos, capturas ni sesiones.
4. Los commits de la migración están separados por capacidad y pueden revertirse
   de forma selectiva. No retirar las rutas antiguas antes de la QA funcional y
   visual del usuario.

Commits base de Pearl Flow, en orden:

```text
7ea8e6c  pill circular
9b17f7b  herramientas persistentes
58c5a73  agrupación por proximidad
fc8439f  Agentes autosuficiente
c762bec  centro de control principal
```

## Etapa posterior: modo de borde

El modo tipo Dynamic Island queda expresamente fuera de este cierre. Debe
implementar solo una estrategia adicional de anclaje y nacimiento desde un
borde. La escena, el registro, la agrupación, el SDF, el tema y la accesibilidad
se reutilizan sin bifurcar el renderer.
