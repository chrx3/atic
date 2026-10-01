# Rendimiento — por qué la pizarra de agentes va lenta al mover y arrastrar

Diagnóstico de la ventana «Consolas de agentes» (`AgentsBoard.svelte`):
correr la vista arrastrando el fondo, hacer zoom, y arrastrar o agrandar una
consola se sienten pesados, y empeora con más consolas abiertas y con agentes
trabajando.

> **Estado (2026-10-01):** análisis de código más una medición en `pnpm dev`
> (sección 6). Con la app recién reiniciada **no se reprodujo**: la pizarra va
> a la frecuencia de la pantalla en todos los gestos. La lentitud reportada
> venía de una sesión larga de la app instalada y desapareció al reiniciar, así
> que lo más probable es algo que se acumula con el uso (sección 7). Las causas
> de la sección 2 siguen siendo costos reales, pero hoy no saturan.
>
> No cubre el float de agentes de la pill (`AgentsFloat.svelte`, en el
> overlay): ese arrastre va por otro camino (`bubbleDrag.ts`, cursor de Rust).

## 1. Qué pasa en cada cuadro

| Gesto | Qué cambia por `pointermove` | Qué tiene que repintar el navegador |
| --- | --- | --- |
| Correr la vista (fondo) | `cam` → `transform` del plano + `background-position` del fondo | Todo el fondo, todo el plano y el desenfoque de los 4 paneles flotantes |
| Zoom (Ctrl+rueda) | `cam.zoom` → `scale()` del plano + `background-size` | Lo mismo, y además re-rasterizar todo el texto DOM a la escala nueva |
| Arrastrar una consola | `drafts` → `left/top` de la tarjeta | Layout de la tarjeta, su sombra grande, y el desenfoque de lo que tenga encima |
| Agrandar una consola | `drafts` → `width/height` | Lo anterior + `ResizeObserver` + `fit()` del xterm (con debounce de 90 ms) |

Lo importante: **nada de esto se queda en el compositor**. Cada cuadro es
pintura, no solo composición, y la pintura cae sobre capas caras (WebGL,
`backdrop-filter`, sombras difusas).

## 2. Causas, de más a menos probable

### 2.1 `backdrop-filter: blur(18px)` sobre una pizarra que nunca está quieta

Cinco paneles flotan sobre la pizarra con desenfoque de fondo:

- `BoardList.svelte:285` — la lista, 232 px de ancho y casi todo el alto.
- `BoardComposer.svelte:167` — la entrada de abajo.
- `BoardMinimap.svelte:109` — el minimapa (sigue montado con `opacity: 0`
  cuando está quieto, `AgentsBoard.svelte:1906`).
- `BoardZoom.svelte:155` — los controles de zoom.
- `SpaceSwitcher.svelte:143` — cuando está abierto.

Un `backdrop-filter` obliga a volver a leer y desenfocar lo que hay debajo
**cada vez que eso cambia**. Al correr la vista o arrastrar una consola, lo de
debajo cambia en todos los cuadros; y como debajo hay canvas WebGL de xterm,
el navegador tiene que leer esas texturas para desenfocarlas. Además, con
agentes trabajando la salida de las terminales cambia sola: el desenfoque se
recalcula aunque no toques nada.

Es el sospechoso principal porque su costo escala con el área (la lista es
alta), con el radio (18 px es mucho) y con cuántas cosas animadas tenga
debajo.

### 2.2 El plano no es una capa propia: correr la vista repinta todo

`.plane` (`AgentsBoard.svelte:1443`, estilos en ~1872) lleva
`transform: translate(...) scale(...)` en línea y **no** tiene
`will-change: transform`. Sin esa pista, Chromium no garantiza tratar el
cambio de transform como una operación del compositor: el contenido del plano
(fondos de tarjeta, barras, sombras, texto, hilos SVG) se vuelve a pintar en
cada `pointermove`.

En el zoom pasa lo mismo y peor: cada paso de escala re-rasteriza el texto DOM
de todas las tarjetas (barras, chats de sub-agentes, `FileView`).

### 2.3 El fondo de puntos se repinta entero en cada movimiento

El fondo de la pizarra es un `radial-gradient` en mosaico
(`AgentsBoard.svelte:1839`) que sigue a la cámara con `background-position` y
`background-size` en línea (`AgentsBoard.svelte:1433-1434`). Cambiar
cualquiera de los dos invalida **toda** la superficie del `.board` (la ventana
completa, ~1120×760 o más maximizada): un gradiente re-rasterizado por cuadro.
En modo «grilla» son dos `linear-gradient`.

### 2.4 Mover una consola es layout, no transform

`BoardCard.svelte:177` posiciona con `left/top/width/height`. Arrastrar cambia
`left/top` en cada cuadro: layout de la tarjeta y repintado de su área vieja y
nueva. La tarjeta tiene:

- una sombra de 48 px de difuminado (`BoardCard.svelte:254`, 60 px si está
  activa), que se pinta de nuevo en ambas posiciones;
- `overflow: hidden` + `border-radius: 12px` con un canvas WebGL adentro: el
  recorte redondeado sobre una capa compuesta necesita una máscara extra.

Con la tarjeta pasando por debajo de la lista o del minimapa, se suma 2.1.

### 2.5 Cada cuadro de arrastre recalcula toda la pizarra en Svelte

`onRect` (`AgentsBoard.svelte:792`) crea un objeto `drafts` nuevo por
`pointermove`. Todo lo que lee `drafts` se vuelve a evaluar:

- `rectOf()` de **todas** las consolas (`:507`), no solo la que se mueve;
- `childRects` (`:249`), que recorre sub-agentes en hasta 4 pasadas;
- `threads` (`:277`) → `BoardThreads` regenera los `d` de cada curva;
- el arreglo `cards` del minimapa se arma de nuevo en la plantilla
  (`:1728`), así que `world`, `map` y la posición de cada rectángulo del
  minimapa se recalculan.

Correr la vista hace algo parecido con `cam`: `setCamera` (`:535`) reemplaza
el objeto, reprograma tres timers (`saveCamera`, `viewIdle`, `flight`), cambia
`viewBusy` (que a su vez cambia las clases de los paneles flotantes y su
`opacity` con transición), y vuelve a evaluar el `zoom` de cada `BoardCard`,
`visibleArea()` del minimapa y `BoardZoom`.

Por sí solo es trabajo chico de JS; suma porque cae en el mismo cuadro que la
pintura de 2.1–2.4.

### 2.6 Un contexto WebGL por consola

`terminalRenderer.ts` abre un `WebglAddon` por terminal. Chromium limita los
contextos WebGL vivos por página (en general 16); al pasarse pierde los más
viejos, y esas consolas caen al renderer DOM de xterm, que es mucho más lento
—y bajo `scale()` lo es todavía más. Con consolas + sub-agentes con TUI real
(`TerminalView` también para hijos) es posible llegar al límite. Si una
consola se pone lenta de golpe y no se recupera, puede ser esto.

### 2.7 Animaciones continuas dentro del plano

- Hilos de sub-agentes: `stroke-dashoffset` animado en bucle
  (`BoardThreads.svelte:85-90`) mientras el sub-agente trabaja o tiene algo
  sin leer. Es una animación de pintura (no de compositor), dentro del plano y
  debajo de los paneles desenfocados.
- Punto de estado: `opacity` en bucle (`ConsoleStateDot.svelte:58`). Este sí
  puede ir por el compositor, pero mantiene vivo el ciclo de cuadros.

No explican el tirón al arrastrar, pero hacen que la pizarra nunca esté
quieta y que 2.1 se pague también en reposo.

### 2.8 Menores

- `attachScaledMouse` (`xtermScale.ts`) pone 3 listeners en captura sobre
  `window` **por terminal**. Cada `mousemove` de la ventana hace un
  `querySelector('.xterm-screen')` por consola, y con zoom ≠ 1 y el puntero
  sobre una terminal, un `getBoundingClientRect()` que puede forzar layout
  justo después de que Svelte movió algo.
- `refreshStates` corre cada 500 ms y compara con `JSON.stringify`
  (`AgentsBoard.svelte:1391`). Barato, pero va al mismo hilo.

## 3. Lo que descarté

- **IPC por cuadro**: arrastrar o correr la vista no llama a Rust; el guardado
  va con debounce (`saveCamera`, 250 ms) y el rect se guarda al soltar.
- **`consoleResize` en cada cuadro de redimensión**: ya tiene debounce de 90 ms
  (`TerminalView.svelte`, `scheduleFit`).
- **Modo dev de Svelte**: la app que corría era la instalada (release).
- **Ventana transparente**: la de consolas es una ventana normal y opaca
  (`agents_window.rs`), a diferencia del overlay.

## 4. Cómo medirlo (antes de cambiar nada)

1. Abrir Atic en dev (`pnpm dev`): la ventana de consolas expone CDP en
   `127.0.0.1:9224` (`agents_window.rs`). En release, el truco de
   `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` sirve igual, pero exige reiniciar
   la app.
2. Abrir 4–6 consolas, al menos dos con un agente trabajando.
3. Con DevTools conectado a esa página, grabar un trace de Performance
   mientras: (a) se corre la vista 3 s, (b) se arrastra una consola por debajo
   de la lista, (c) se hace zoom con Ctrl+rueda.
4. Mirar en el trace: duración de **Paint** y **Raster** por cuadro (2.1–2.4),
   **Layout** dentro del `pointermove` (2.4), y **Scripting** de los handlers
   (2.5). Rendering → «Paint flashing» y «Layer borders» muestran qué se
   repinta y qué es capa propia.
5. Prueba rápida de la hipótesis principal: desde la consola de DevTools,
   inyectar `*{backdrop-filter:none!important}` y repetir el gesto. Si mejora
   claramente, 2.1 es la causa principal.

## 5. Arreglos posibles, de menos a más riesgo

Ninguno está aplicado. Conviene ir uno a uno y medir entre medio.

1. **Apagar el desenfoque mientras la vista se mueve o se arrastra.** Ya
   existe `viewBusy`, y `BoardCard` sabe cuándo arrastra. Con una clase en
   `.board` (`is-moving`) los paneles pasan a un fondo sólido casi opaco
   (`--rb-surface` al 95 %) y vuelven al vidrio al soltar. Bajar el radio
   (18 → 10 px) también ayuda. Cambio solo de CSS.
2. **`will-change: transform` en `.plane` solo mientras se corre la vista**
   (`panning` o `viewBusy`), no siempre: dejarlo fijo hace que el zoom se vea
   borroso hasta re-rasterizar. Así correr la vista es composición pura.
3. **Fondo de puntos en su propia capa.** Un elemento fijo detrás del plano
   con el patrón, movido con `transform` (módulo del tamaño de celda) en vez de
   `background-position`. El gradiente se rasteriza una vez.
4. **Arrastrar con `transform` y fijar al soltar.** Mientras arrastra, la
   tarjeta se corre con `translate` (y la sombra grande baja o se apaga);
   `left/top` se escribe solo al soltar. Evita layout y repintado por cuadro.
5. **Que el arrastre no invalide toda la pizarra.** Guardar el borrador fuera
   de un objeto que leen todos (un `$state` por tarjeta, o pasar el borrador
   solo a la tarjeta que se mueve) y no recalcular el minimapa e hilos en cada
   cuadro, sino al soltar o con `requestAnimationFrame`.
6. **Pausar animaciones en bucle durante el gesto** (hilos, punto de estado)
   con la misma clase `is-moving`.
7. **Tope de contextos WebGL**: renderer WebGL solo para las consolas
   visibles o las N más recientes, y avisar si un `onContextLoss` las bajó al
   DOM.

Los puntos 1–3 son CSS y se prueban con HMR sin tocar Rust. 4 y 5 tocan la
lógica de `BoardCard`/`AgentsBoard` y piden revisar los tests de
`agentBoard.test.ts`. 7 toca `TerminalView`.

## 6. Medición (2026-10-01, `pnpm dev`)

Equipo: laptop Ryzen 7 5700U con GPU integrada Vega (la misma GPU para todas
las ventanas, el overlay de la pill y el escritorio). Ventana de consolas
maximizada 1920×1009, pantalla a 75 Hz. Seis consolas: dos de Claude Code
trabajando y cuatro shells imprimiendo ~30 líneas/s cada una. Todas las
terminales con WebGL (ninguna cayó al renderer DOM).

Gestos simulados por CDP (`Input.dispatchMouseEvent`, ~2 s cada uno), con
cada variante inyectada como CSS:

| Gesto | Cuadros | GPU 3D del webview: base → sin blur → planoCapa → sin fondo → todo |
| --- | --- | --- |
| Correr la vista | 73–75 fps, p95 13.5 ms | 22 % → 13 % → 17 % → 18 % → 11 % |
| Arrastrar consola | 75 fps, p95 13.6 ms | 17 % → 13 % → 18 % → 18 % → 13 % |
| Zoom (Ctrl+rueda) | 73–75 fps, p95 13.7 ms | 19 % → 15 % → 17 % → 18 % → 12 % |

En reposo el webview usa 11–18 % del 3D (depende de cuánto imprimen las
terminales). Sin `long-animation-frame` durante los gestos: el JS de la
sección 2.5 no aparece.

Conclusiones:

- Recién abierta, la pizarra no está limitada ni por el hilo principal ni por
  la GPU.
- De lo que se probó, el `backdrop-filter` es lo que más GPU cuesta: sacarlo
  baja el paneo ~40 %. `will-change` en el plano reduce `GPUTask` en el trace
  a la mitad, pero en el contador del sistema se nota poco.
- Con una GPU integrada compartida, ese margen se puede comer desde afuera
  (video en el navegador, el overlay transparente de la pill, otra app con
  GPU).

No se midió la latencia con el mouse real: la ventana de consolas no estaba al
frente y el gesto terminó en otra app.

## 7. Si vuelve a ponerse lenta

Que se arregle al reiniciar apunta a algo que crece con las horas o con abrir
y cerrar consolas. Candidatos a revisar en ese momento:

- contextos WebGL perdidos (consolas que pasaron al renderer DOM y no
  vuelven): contar `.xterm-rows > div` por tarjeta;
- uso de GPU y memoria del proceso `msedgewebview2` de `agents-webview` en el
  Administrador de tareas, comparado con recién abierta;
- listeners o registros que no se limpian (`presenceConsole`, `activity`,
  `externalConsole` en `AgentsBoard.svelte` solo crecen).

Antes de reiniciar, medir. Con `pnpm dev` la ventana expone CDP en
`127.0.0.1:9224` y se puede repetir esta medición sobre la sesión lenta. En
la instalada no hay puerto, y reiniciarla borra la evidencia.
