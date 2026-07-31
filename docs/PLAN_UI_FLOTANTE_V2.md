# Plan de implementación — UI flotante V2

## Objetivo

Reemplazar la experiencia flotante por un compositor único, pulido y verificable
que cubra pill, rueda, reuniones, dictado, clipboard, textos, capturas y agentes.
El backend Rust, almacenamiento y comandos existentes se conservan.

## Decisión arquitectónica

```text
Tauri commands ─┐
Fake lab data ──┴─> adapter -> controller -> shared liquid renderer -> host
```

El renderer no conoce Tauri. Producción y lab cambian únicamente el adapter.
Toda superficie que se fusione vive en el mismo documento.

## Fases

### 1. Renderer y lab compartidos

- Tokens V2 y piel SVG única generada con SDF, smooth-union y marching squares.
- Control `Blend` en el lab para recorrer juntas geométricas y fusión orgánica.
- Estado explícito: `idle`, `wheel`, `tool`, `agents`.
- Pill, seis herramientas y panel representativo con datos falsos.
- Navegación por mouse y teclado; reduced motion.

### 2. Host de escritorio

- La rama `codex/floating-ui-v2` apunta la ventana `pill` a `/pill-v2`; la ruta
  anterior se conserva para comparación y rollback hasta completar QA visual.
- Una escena por monitor activo, empezando por un monitor.
- Hit-testing transparente fuera de superficies vivas.
- Foco activable solo cuando hay campos de texto.
- Medición, recorte y rendimiento validados dentro de WebView2.

### 3. Conexión funcional

- Reuniones y dictado.
- Clipboard y textos.
- Capturas.
- Agentes y permisos.

### 4. Robustez

- DPI mixto y cambio de monitor.
- Estados vacíos, error, carga y tareas largas.
- Tema oscuro y contraste sobre fondos variables.
- Trazas de geometría y medición de frames largos.

### 5. Variante futura de borde

`PlacementAdapter` permitirá fijar la misma experiencia a un borde de pantalla y
hacerla nacer de él, al estilo de una Dynamic Island. No se duplica el renderer.

## Criterios de aceptación

- Abrir/cerrar rueda diez veces no produce recorte ni salto de posición.
- Cada herramienta se alcanza con teclado y mouse.
- La primera pulsación llega al control correcto.
- Escribir en agentes no roba ni devuelve foco de forma inesperada.
- Reduced motion conserva todos los estados sin desplazamientos.
- `pnpm check`, autofixer Svelte, Rust fmt/clippy/tests y build pasan.
- Validación visual final realizada dentro de WebView2, no solo en navegador.

## Estado verificado · 2026-07-31

- `pnpm check`: 0 errores y 0 warnings.
- `pnpm build`: build estático correcto; `/pill-v2` y `/lab/floating-v2`
  responden HTTP 200 desde preview.
- Test geométrico: contornos cerrados, puente pill-panel, apertura hacia arriba,
  caché y variación de `Blend` comprobados.
- Perfil CPU orientativo: 60 contornos de la escena de agentes en 208.2 ms
  (3.47 ms por cálculo en esta máquina).
- `cargo fmt --all --check` y `cargo check --workspace`: correctos.
- `cargo test --workspace`: la compilación no terminó dentro del límite de cinco
  minutos; no se considera aprobado ni fallido.
- QA visual WebView2: pendiente. Las capacidades de navegador y control de
  Windows no estuvieron disponibles en esta sesión; no se sustituye por una
  afirmación visual sin evidencia.

## Goal Pearl Flow persistente · brief confirmado

La segunda etapa cambia el modelo de “una pill que se agranda y reemplaza su
contenido” por un mundo de objetos persistentes:

- la pill reposa como círculo y solo se alarga para actividad, progreso o
  confirmaciones; la forma de reposo será configurable;
- cada herramienta nace desde la pill, queda abierta como objeto arrastrable y
  no obliga a cerrar las demás;
- una sola instancia por herramienta; Agentes contiene varias conversaciones;
- la cercanía crea unión líquida y atracción; al soltar, los objetos forman un
  grupo que se mueve como conjunto;
- `Separar` y `Alt+arrastrar` desprenden un miembro de forma explícita;
- la nueva superficie de Agentes debe crear y operar sesiones completas sin
  depender de `/agents`;
- la ventana principal pasa a ser biblioteca y centro de control, con
  navegación líquida lateral o inferior según el ancho.

El modo anclado al borde tipo Dynamic Island sigue siendo posterior. Debe usar
el mismo renderer y el mismo registro de objetos, no una implementación paralela.

## ADR: compositor híbrido por monitor

### Decisión inicial

El spike parte con una capa visual global **siempre click-through** y ventanas
interactivas pequeñas para la pill y el contenido de cada herramienta.

```text
Rust FloatingWorld (geometría, grupos, monitores)
             │
             ├──> LiquidWorld por monitor (SDF, click-through, sin foco)
             └──> ventanas de contenido (DOM accesible, foco e interacción)
```

Una WebView interactiva del tamaño del monitor queda descartada como diseño de
producción: WebView2 recibe los clics también en sus píxeles transparentes, y
alternar `ignore_cursor_events` afecta a la ventana completa. El enfoque híbrido
conserva el escritorio utilizable sin exigir hit-testing por píxel dentro del
WebView.

La rama `ui-rewrite` ya midió el SDF dentro del overlay WebView2 real: 2.3 ms de
cálculo por cuadro, 88 fps y p95 de 16.9 ms en su peor caso animado. Nuestra V2
también usa SDF y ya comprobó contornos, puentes y caché. El riesgo principal ya
no es el contorno: es sincronizar piel, contenido y z-order entre ventanas.

### Spike que debe cerrar la decisión

Antes de migrar herramientas, un prototipo con pill + dos objetos debe probar:

1. El escritorio recibe clics fuera de las ventanas de contenido.
2. Un campo de texto conserva foco mientras se mueve otra superficie.
3. La piel sigue a las ventanas sin tearing visible ni saltos de DPI.
4. Acercar objetos crea cuello; soltarlos agrupa; arrastrar uno mueve el grupo.
5. `Alt+arrastrar` y la acción `Separar` desprenden solo el objeto elegido.
6. El p95 permanece bajo 20 ms durante arrastre en WebView2.
7. Al cruzar de monitor, el objeto cambia de compositor sin duplicarse ni
   bloquear clics.

Si el z-order de una capa global no puede mantenerse detrás de todas las
ventanas de contenido sin parpadeo, el fallback es una ventana compuesta por
cluster: cada grupo líquido vive en una ventana ajustada a su bounding box; los
objetos libres conservan ventanas propias. Ese fallback se valida antes de
construir la UI final.

### Evidencia inicial del spike · 2026-07-31

El binario estático debug se ejecutó con `ATIC_LIQUID_WORLD_SPIKE=1` dentro de
WebView2 y creó cuatro ventanas reales:

- `Pearl Flow world`: 1536×816 en coordenadas lógicas del monitor, capa SDF;
- `Pearl Flow pill`: 48×48 físicos;
- `Pearl Flow meetings`: 280×180 físicos;
- `Pearl Flow agents`: 360×260 físicos, con campo de texto real.

Comprobaciones cerradas:

- proceso vivo y responsive, sin salida en `stderr`;
- la capa global tiene `WS_EX_TRANSPARENT` y `WS_EX_NOACTIVATE`;
- las tres ventanas de contenido no tienen esas banderas y siguen siendo
  regiones interactivas independientes;
- a 125% de escala, piel y tinta quedaron alineadas en la captura compuesta;
- mover Reuniones mediante `SetWindowPos` disparó `WindowEvent::Moved`, actualizó
  el snapshot y produjo tres contornos separados; restaurarlo vuelve a permitir
  la fusión por proximidad;
- la ruta y las ventanas son opt-in: no se crean en un arranque normal.

Pendientes del comportamiento de objetos, ya fuera del riesgo del compositor:

- arrastre humano sostenido y foco del campo durante interacción cruzada;
- medición p95 del cálculo y de la entrega de snapshots durante arrastre;
- z-order después de muchos movimientos y cambios de foco;
- transferencia a un monitor con otro DPI.

### DecisiÃ³n cerrada del compositor Â· 2026-07-31

El spike se extendiÃ³ a una `LiquidWorld` click-through por monitor y se ejecutÃ³
desde un binario Tauri estÃ¡tico (`tauri build --debug --no-bundle`), sin servidor
de desarrollo ni HMR. El equipo de prueba expuso dos monitores de 1536Ã—864
(Ã¡rea Ãºtil 1536Ã—816), ambos a 120 DPI / 125 %.

Evidencia reproducible obtenida sobre las ventanas WebView2 reales:

- 38 movimientos nativos de Reuniones, con 120 ms entre posiciones, mantuvieron
  `Pearl Flow agents` como foreground y el DOM reportÃ³
  `focused=true active=input#-` durante la secuencia;
- el SDF del compositor primario registrÃ³ 60 muestras con media entre 1,28 y
  1,44 ms y p95 mÃ¡ximo de 2,70 ms, muy por debajo del lÃ­mite de 20 ms;
- despuÃ©s de 12 ciclos alternando foco entre Reuniones y Agentes, las ventanas
  interactivas siguieron delante de pill y ambas delante de la capa global;
- la telemetrÃ­a por monitor midiÃ³, tras el cruce, p95 SDF de 1,10 ms en el
  destino y 3,40 ms en el origen; la entrega de snapshots fue 6 ms p95 en el
  destino;
- Reuniones cruzÃ³ de `x=528` a `x=-800` sin activar su ventana: apareciÃ³ una
  sola vez en `monitor-0`, desapareciÃ³ del contorno de `monitor-1` y Agentes
  conservÃ³ foco en el monitor primario;
- las dos capas globales conservaron `WS_EX_TRANSPARENT` y
  `WS_EX_NOACTIVATE`; las superficies de contenido conservaron input nativo.

**DecisiÃ³n:** se acepta el compositor hÃ­brido por monitor como base de
producciÃ³n. El fallback por cluster no es necesario con la evidencia actual.
El caso de monitores con DPI diferentes queda como prueba de robustez porque el
hardware disponible usa la misma escala; la conversiÃ³n ya se hace contra el
`scale_factor` de cada `LiquidWorld`.

La siguiente fase promueve este contrato a `FloatingWorld`: registro persistente
de superficies, pill circular, agrupaciÃ³n y separaciÃ³n. El spike sigue opt-in y
la UI anterior permanece disponible para rollback hasta completar QA.

### Registro persistente y superficies reales - 2026-07-31

La arquitectura aceptada ya opera en el arranque normal, no solo en el spike:

- se crea una `LiquidWorld` click-through por monitor y la pill conserva su
  ventana interactiva de 64x64 px fisicos a 125 %;
- `FloatingWorldState` mantiene una instancia por herramienta, posicion,
  tamano y visibilidad en `floating-world.json`;
- las herramientas nuevas se crean desde un comando asincrono para evitar el
  deadlock de WebView2 al construir un WebView desde un comando Tauri sincrono;
- Reuniones y Clipboard se abrieron desde la rueda mediante sus nombres UIA,
  coexistieron a ambos lados de la pill y conservaron DOM interactivo;
- cerrar Clipboard actualizo `visible=false` sin destruir Reuniones; al
  reiniciar el binario solo Reuniones fue restaurada;
- el primer objeto se coloca a la derecha, el segundo a la izquierda y los
  siguientes abajo/arriba, evitando que las superficies nazcan superpuestas;
- la piel global conoce la pill circular, la forma compacta, los seis nodos de
  la rueda y todas las ventanas `pearl-tool-*`; la tinta no se duplica dentro
  del SVG;
- la piel y la tinta usan variantes claras y oscuras coherentes; la telemetria
  del spike queda oculta en produccion.

La UI anterior (`/pill` y `/agents`) sigue disponible como rollback. Agrupacion,
separacion y la sustitucion funcional completa de Agentes continuan pendientes.
