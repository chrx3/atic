# Prototipo de la pill en GPUI

Experimento para evaluar portar la pill de Atic de Svelte/WebView2 a
[GPUI](https://crates.io/crates/gpui). Por ahora solo funciona en Windows.

## El modelo: un solo notch

La pill es un solo objeto que cambia de forma. Las herramientas con panel
(Clipboard, Textos, Apps, Agentes, Sistema y Ahora suena) **no abren tarjetas
aparte**: el notch se estira hasta el tamaño de la herramienta, y la franja de
40 px de arriba del panel queda para la marca y el buscador o el título.
Cambiar de herramienta con el notch abierto solo cambia el contenido.

**El notch se abre en el borde donde está la pill.** Arriba, al centro, como
siempre. En un costado el tab se estira ahí mismo, con la misma curva y el
mismo tiempo (`ease_island`, 300 ms), el mismo vidrio y el mismo tinte, y al
cerrarse vuelve a ser el tab en su lugar. Ahí todo se ordena en vertical:

- **Todo va de pie.** Los paneles y los vistazos no se giran con el borde: el
  texto va siempre derecho. `src/geometry.rs` mide todo como tab (largo a lo
  largo del borde, grosor hacia adentro), así que en un costado el ancho de un
  panel es grosor y su alto es largo (`Edge::extent`, `notch_rect`).
- **Paneles altos y angostos.** En un costado el panel mide `SIDE_PANEL_W`
  (380) de ancho y crece hasta casi todo el alto del área de trabajo
  (`SIDE_PANEL_MARGIN` arriba y abajo): las listas (Clipboard, Textos, Apps,
  Agentes) muestran todo lo que quepa (`max_height` de cada panel) y los que
  tienen poco (Sistema, Ahora suena) miden lo suyo, sin espacio muerto. Va
  pegado al borde, centrado en la altura del tab y corrido para no salirse.
- **La marca** se corre a la izquierda de la franja del panel en cualquier
  borde (el hueco de los buscadores). Las esquinas redondas son las del lado
  que mira al escritorio, como en el tab.
- **Lo que cuelga del tab** (los vistazos, el aviso de la bandeja, el uso, la
  letra y el dictado) va bajo la franja arriba. En un costado sale en un
  **bloque angosto al lado de la herramienta bajo el cursor, a su altura**
  (`geometry::side_block`, `SIDE_W` = 300), y mide solo lo que ocupa: lo que
  arriba va en fila ahí va en columna (los colores, las capturas y las páginas
  de Flip, una por fila con su miniatura; los anillos del uso, de a tres; las
  filas de la bandeja, con los botones bajo el texto). En reposo (la letra, el
  aviso de la bandeja, el dictado) sale junto a la marca. Al pasar de una
  herramienta a otra el bloque se desliza a la nueva altura (240 ms,
  `ease_island`, como el alto de los vistazos).
- **El cuerpo es una L.** La columna de la tira y el bloque son una sola piel
  (`geometry::side_outline`): esquinas redondas hacia el escritorio y una
  esquina cóncava donde el bloque se une a la columna, que se achica sola
  cuando los bordes de los dos casi coinciden (no salta al deslizarse). El
  vidrio recorta las dos piezas (`glass::Shape::Pair`: dos visuales con el
  mismo fondo en la misma ventana); el rincón cóncavo queda sin vidrio, bajo
  el tinte casi opaco no se nota.
- Los contadores de la bandeja van antes y después de la marca a lo largo del
  tab: en un costado, arriba y abajo de ella.
- **Solo vuela si donde está no hay notch**: flotando, o en un costado más
  bajo que `NOTCH_MIN_SIDE_H` (un monitor muy bajo), la gota
  **vuela hasta el notch de arriba** (340 ms, en arco), se estira ahí, y al
  cerrarse vuelve a su lugar (300 ms) con el aplastón de acoplado. La posición
  guardada no cambia.
- Abajo usa lo mismo (el panel crece hacia arriba, con su franja arriba), pero
  no se ha visto: en esta máquina la barra de tareas ocupa ese borde.

Las herramientas de pantalla completa (Capturas, Pizarra, Color y Flip) usan
la misma ventana del overlay, encima de todo.

## Vidrio (`src/glass.rs`)

La pill (tab, notch o gota) es de vidrio esmerilado: una ventana nativa sin
contenido justo debajo del overlay, movida en cada cuadro, con un visual de
Windows.UI.Composition pintado con el escritorio desenfocado que da DWM
(`HostBackdropBrush`) y un recorte geométrico con antialias, como el acrylic
de WinUI. Encima, la pill pinta un tinte del 60 % y un filo de luz de 1 px.
La rueda y la burbuja de vista previa siguen opacas: detrás no tienen blur.

- El acrylic de ventana (`SetWindowCompositionAttribute`) no sirve: ignora la
  región de la ventana y deja un cuadrado detrás de la gota.
- Requiere los «Efectos de transparencia» de Windows; sin ellos no hay fondo
  desenfocado y queda el tinte.
- `PILL_GLASS=off` lo apaga.
- Falta el grano del acrylic (una textura de ruido encima).

## La pill

- El tab acoplado (124 × 40) a cualquier borde sin barra de tareas, con la
  marca de Atic: sus ojos siguen al cursor y parpadea. En los bordes laterales
  va de pie y la tira de herramientas se abre en columna; los vistazos salen
  al lado de la columna (ver «El modelo: un solo notch»).
- Arrastrar la pill (`src/geometry.rs`, reglas de `edgeDock.ts`):
  - Se arrastra desde cualquier parte del tab o de la gota pasados 4 px; un
    movimiento menor cuenta como clic.
  - Alejarla más de 64 px del borde la convierte en una gota flotante de 52 px.
    Cerca de un borde, un cuello líquido la une a él.
  - Al soltarla a 28 px o menos de un borde se acopla con un aplastón de
    125 ms; arriba siempre al centro.
  - La posición se guarda en `%LOCALAPPDATA%\atic-gpui\home.txt`.
- La tira de herramientas al pasar el cursor: 240 ms con `--ease-island` y
  150 ms de gracia al salir.
- La rueda de 10 herramientas. Desde el tab se abre con clic en la marca o con
  Alt+Z; desde la gota, al pasar el cursor 180 ms. Se cierra con clic al
  centro, al elegir una herramienta o al alejarse. Es solo animación (la gota
  baja y las herramientas brotan con cuellos tipo metaball); no hay
  deformación interactiva.
- Una ventana transparente, siempre encima, que no roba el foco y deja pasar
  los clics fuera de la pill.

## Herramientas en el notch

### Clipboard (`src/clipboard.rs`)

- La franja es el buscador (con IME y portapapeles). Debajo, una tira con las
  imágenes y colores y después los textos en una línea, agrupados por día. El
  alto se ajusta a lo que hay.
- Filtros por tipo y favoritos; lista virtualizada. Lo que parece una clave se
  muestra incompleto (`sk-ab••••xyz`, `src/secrets.rs`) y se pega entero.
- **Vistazo**: el cursor sobre Clipboard en la tira estira el notch con las
  últimas 3 copiadas. Clic pega en la app de atrás y arrastrar las lleva a otra.
- **Vista previa**: una burbuja que se desprende del costado del notch para
  los textos largos y las imágenes.
- Lee el historial real de Atic, `%APPDATA%\ciat\atic\data\clipboard\history.json`
  (u otra carpeta con `ATIC_CLIPBOARD_DIR`), y lo recarga cuando Atic lo
  reescribe (`src/history.rs`). No lo escribe: favoritos y borrados van a
  `local.json`. Sin ese archivo usa 100 entradas de prueba.
- Pega como Atic: devuelve el foco a la ventana anterior, escribe el
  portapapeles y manda Ctrl+V (o Ctrl+Shift+V en terminales, `src/paste.rs`).
  Las imágenes van como `CF_DIB` y PNG, para Paint y Word (`src/clip_image.rs`).
- Arrastre a otras apps (OLE, `src/drag.rs`, copiado de Atic): el texto va
  como Unicode, ANSI, OEM y Locale; las imágenes, como archivo PNG
  (`CF_HDROP`) en `%TEMP%\atic-gpui-drag`. Si el destino no acepta el texto,
  pega como respaldo.
- Teclas: ↑/↓, Enter pega y Esc cierra. Un clic fuera lo cierra, salvo que
  esté fijado.

### Textos (`src/snippets.rs`)

Tres pestañas en la franja:

- **Textos**: busca por alias, nombre y cuerpo (también con letras salteadas)
  y pega expandiendo `{fecha}`, `{hora}`, `{fecha_hora}` y `{portapapeles}`.
  Ctrl+N guarda lo copiado. Lee `snippets.json` de Atic (`ATIC_SNIPPETS_DIR`)
  sin escribirlo; lo que creas o borras va a `snippets-local.json`.
- **Bloc**: editor multilínea (`src/text_area.rs`) que guarda solo, 600 ms
  después de la última tecla, en el `scratchpad.json` de Atic.
- **Tablero**: las notas de texto y listas del tablero del flip, para pegarlas
  (solo lectura).

### Apps (`src/launcher.rs`)

El lanzador, con Ctrl+Shift+Espacio (`PILL_LAUNCHER_KEY=ctrl-space` usa el
Ctrl+Espacio de Atic). La franja es el buscador. Sin texto muestra favoritos y
recientes; con texto, los resultados:

- Accesos del menú Inicio, apps de Store por AUMID y acciones del sistema, con
  el mismo puntaje de Atic.
- La calculadora: es el `calc.rs` de Atic, incluido con `#[path]`.
- «:» entra al modo emoji.
- Las apps abiertas se marcan «En uso» (`src/running.rs`) y Ctrl+Enter las
  cierra con `WM_CLOSE`.
- Ctrl+1–9 abre directo. Ctrl+M lo pasa al centro de la pantalla, tipo
  Spotlight: es el único panel que no vive en el notch.

### Agentes (`src/agents.rs`)

- **En curso**: las sesiones de Claude Code y Codex, leídas de los mismos
  archivos que vigila Atic. Para Claude Code es `~/.claude/projects/*/*.jsonl`
  (o `CLAUDE_CONFIG_DIR`); para Codex, los rollouts de la TUI en
  `~/.codex/sessions`.
  - Cada fila muestra el proyecto y el estado, que puede ser trabajando (con
    lo que hace: «Editando main.rs», «Ejecutando cargo test») o listo (con la
    última línea de la respuesta).
  - Solo aparecen los archivos tocados en los últimos 15 min. Lo listo se va a
    los 30 min.
  - Se relee cada segundo y solo se leen las líneas nuevas.
- **Clic en una fila** trae al frente la terminal del agente. La busca
  subiendo por el árbol de procesos desde `claude.exe` o `codex.exe`, sin
  contar los que cuelgan de Atic. Como el JSONL no trae pid, solo enfoca si
  hay una ventana posible; si hay varias, lo dice en el pie y no adivina.
- **↗ en una fila** la reanuda en el espacio: `claude --resume <id>` o
  `codex resume <id>`, en su carpeta.
- **Nuevo**: el catálogo de `agentCatalog.ts` (Claude Code, OpenCode, Codex,
  Cursor, Antigravity y Grok) y una carpeta.
  - Las carpetas son las recientes de tus sesiones, o «Otra…» con el selector
    de Windows.
  - «Abrir» lo lanza como consola en el espacio, en `cmd /k`.
  - Un agente que no está en el PATH aparece atenuado y el botón lo instala
    con el comando de Atic.
  - Lo elegido se guarda en `%LOCALAPPDATA%\atic-gpui\agents.json`.
  - Teclas: ←/→ cambian de agente, Enter abre y Esc cierra.
- Falta respecto de Atic:
  - El estado «esperando permiso», que en Atic llega solo por los hooks.
  - OpenCode y Cursor en curso.
  - El chat estructurado de Agentes.

### Bandeja (`src/tray.rs`)

Lo que los agentes dejan para ti, dentro del mismo notch y sin ventanas aparte.

- **Contadores en el tab.** A la izquierda de la marca, un punto ámbar con
  cuántos agentes van; a la derecha, una verificación verde (por revisar) o un
  rombo azul (decisión) con cuántos esperan: la forma dice lo más urgente y el
  número cuenta todo. Se pegan a la marca para dejar los bordes a la música
  (carátula y onda). Un clic en un contador abre Agentes; ni ellos ni las filas
  del vistazo abren la tira (la tira sale de la marca).
- **Vistazo.** Cuando llega algo nuevo y el notch está quieto, el tab se
  ensancha a 440 y baja hasta 3 filas, la decisión primero, con sus botones:
  «Ver» y «Aceptar» en un turno terminado, «Negar» y «Permitir» en un permiso.
  La franja no cambia, así que la música sigue a la vista. Un turno se recoge a
  los 5 s y una decisión a los 20 s; con el cursor encima no se van. Entre un
  vistazo y el siguiente pasan 6 s, salvo una decisión, y en una llamada (el
  punto de privacidad) un turno no sale solo. También sale al pasar el cursor
  por Agentes en la tira, donde la insignia del ícono lleva la cuenta. No toma
  el foco: sus clics se resuelven a mano (`layout` da las mismas medidas al
  dibujo y al clic), como los del vistazo de Clipboard.
- **De dónde sale.** De las sesiones que ya lee Agentes: cuando una pasa de
  trabajando a lista deja una fila «terminó un turno» con la última línea de su
  respuesta. Si vuelves a escribirle, la fila se va sola; si la terminal del
  agente es la ventana que estás mirando, no deja fila. La primera lectura no
  cuenta: lo que ya estaba listo al abrir el notch no es noticia. Las filas
  caducan a los 30 min (las decisiones no).
- **Panel de Agentes.** «Por revisar» va sobre «En curso», que no repite lo que
  ya está arriba; «Nuevo» se pliega a una fila cuando hay pendientes. Al pie, el
  **mensaje rápido** (Enter lo escribe, con su Enter, en cada agente vivo del
  espacio; sin agentes no se muestra) y un **mini reproductor** con lo que suena
  (anterior, pausa, siguiente): al abrir un panel el tab deja de mostrar la
  carátula, y la música no debería quedar fuera de alcance.
- Falta: los permisos necesitan los hooks de Atic (hoy solo existen con
  `PILL_TRAY_DEMO=1`); las consolas del espacio que no son Claude Code o Codex
  (OpenCode, Cursor…) no dejan filas, porque no tienen JSONL y habría que leer su
  estado de la salida del PTY, como el Mando; el reproductor no abre «Ahora
  suena»; los contadores no se ven en la gota; y el
  mensaje rápido con la ventana del espacio cerrada escribe en las consolas
  guardadas, pero esa vía no se ha probado.

## Espacio (`src/space/`)

La ventana de consolas: un plano infinito como la pizarra de agentes de
Atic, pero con terminales nativas. Se abre con «Espacio» en el notch de
Agentes, y ahí van «Abrir» y «↗» (reanudar).

- Las terminales son `alacritty_terminal` (parser VT, grilla, scrollback y
  ConPTY) dibujado por GPUI: un solo renderer para todas, sin un contexto
  WebGL por consola. Cada renglón se compone una vez y GPUI lo reusa
  mientras no cambie.
- El zoom no escala un bitmap: el texto se compone al tamaño real. Mientras
  la cámara se mueve, la letra va en escalones del 6 % para reusar tamaños ya
  rasterizados.
- Zoom semántico: desde 65 % texto; entre 30 % y 65 %, la silueta de la
  salida en vivo (barras del color de cada tramo); por debajo, una tarjeta
  con el agente y sus últimas líneas.
- Lo que queda fuera de la vista no se dibuja.
- Estado por consola: trabajando si escribió algo en los últimos 1,5 s.
- Arrastrar el fondo mueve, Ctrl+rueda hace zoom, la rueda sobre una consola
  recorre su historial. Doble clic en el fondo ajusta todo y en una tarjeta
  lejana la acerca. Se mueve por el encabezado y cambia de tamaño por la
  esquina (el PTY se ajusta al soltar).
- Atajos: Ctrl+V pega, Ctrl+Tab cambia de consola, Ctrl+Shift+T abre una
  PowerShell, Ctrl+Shift+W cierra, Ctrl+0 ajusta, Ctrl+± zoom.
- Cerrar la ventana no termina a los agentes (ver «Agentes que sobreviven» más
  abajo).
- Falta: selección y copiar con el mouse, dibujar el texto de un IME en
  composición, guardar el espacio entre ejecuciones y las islas por proyecto.

### Vista Mando (`src/space/mando.rs`)

El espacio tiene dos vistas, con pestañas arriba: **Mando** (la de siempre al
abrir) y **Pizarra** (el plano de arriba). `SPACE_VIEW=pizarra` abre la otra.

- **La ventana es la barra de arriba** (`src/space/chrome.rs`): no tiene la barra
  de Windows. La barra lleva la marca de Atic, las pestañas, los botones de
  ventana de Windows 11 y una zona libre que arrastra la ventana (doble clic la
  maximiza, y el menú de ajustar de Windows 11 sale al pasar por maximizar). El
  ícono de la barra de tareas y de Alt+Tab es el de Atic (`assets/atic.png`).
- **Estilo plano, sin bordes.** Entre una capa y la siguiente solo cambia el
  color: ventana, terminal, panel y carta enfocada (más clara). GPUI no recorta
  el contenido a las esquinas redondeadas, solo a un rectángulo; por eso nada se
  pinta contra la esquina de su contenedor: todo va metido hacia adentro y con
  su propio radio. La zona de arrastre y los botones de ventana llevan `occlude()`: sin
  eso la raíz, que toma el foco al hacer clic, hace creer a GPUI que el clic ya
  fue atendido y Windows no mueve la ventana ni pulsa el botón. La Pizarra usa
  el mismo estilo: sin borde ni anillo de foco, la tarjeta enfocada lleva el
  encabezado más claro.
- Una **carta por agente** con su logo, su estado y, bajo el nombre, lo que el
  agente dice de sí (su título), si no la carpeta donde trabaja y si no la
  última línea de su salida.
- La **consola enfocada**, grande, es la misma terminal de la pizarra. Su PTY
  toma el tamaño del panel; al volver a la pizarra recupera el de su tarjeta.
- La **bandeja de revisión** junta lo que nadie ha mirado: un turno de 3 s o
  más que se calló, o un proceso que terminó. Mirar la consola la saca de ahí,
  y «Aceptar» la descarta sin abrirla. «En curso» lista lo que trabaja.
- El estado sale solo de la salida del PTY (trabajando si escribió hace menos
  de 1,5 s), como `consoleStatus.ts` de Atic. Cambiar el tamaño de una TUI la
  hace repintar entera: 1,5 s después no cuenta como trabajo.
- Atajos: `Alt+J` salta a lo que más espera (`Ctrl+J` es el salto de línea de
  Claude Code), `Ctrl+1…9` enfoca la consola N y `Ctrl+Shift+M` cambia de vista.
- Teclado de las terminales: las teclas especiales y los atajos (Enter, flechas,
  Ctrl+letra, Alt+letra…) los atiende `console::key_bytes`. El **texto** (letras,
  símbolos, el espacio, AltGr y las tildes) lo entrega el sistema por
  `src/space/input.rs`, como a un campo de texto: así Windows resuelve lo que
  depende de la distribución. GPUI solo reconoce AltGr en las de su lista (España
  sí; la latinoamericana, `080A`, no) y en las otras lo informa como Ctrl+Alt, y
  con una tecla muerta (`´` + `a`) sale `´a` si se escribe desde `key_down`;
  por el manejador de texto sale `á`. Ese manejador sostiene la vista de forma
  débil a propósito (ver abajo).
- **«Cerrar» pregunta.** En el encabezado de la consola enfocada, «Cerrar» ofrece
  «En segundo plano» (el agente sigue trabajando, sin carta; vuelve con «Abrir»
  desde «En segundo plano» de la bandeja, y si termina o se detiene avisa en la
  bandeja de revisión como cualquier otro), «Terminar» (detiene el proceso y
  quita la consola) y «Cancelar» (también Esc).
- Falta: «bloqueado por permiso» (en Atic llega por los hooks), la pestaña Árbol
  (necesita el hub del MCP), hablar a todos y más cartas que ancho.

### Agentes que sobreviven (`src/space/persist.rs`)

Las consolas son de la aplicación, no de la ventana. Al cerrar la del espacio
(la X de la barra), la vista se suelta y `on_release` pasa sus consolas a un
global de GPUI: los PTY siguen corriendo con su historial y la ventana que se
abra después las recoge con todo (cuál tenía el foco, la cámara, qué quedó por
revisar). El notch (Agentes → Espacio) abre esa ventana nueva: `open_space` ve
que su ventana anterior ya no existe y llama a `open_window`. Si la aplicación
se cierra, las consolas se van con ella: un ConPTY no sobrevive a su proceso
padre. Para sobrevivir a la aplicación haría falta un proceso aparte que las
sostenga.

- Para que la vista llegue a soltarse, nada que viva dentro de la ventana nativa
  puede sostenerla con una referencia fuerte: `ElementInputHandler` guarda la
  vista así y Windows libera esa ventana con retraso (o no la libera), por eso
  `input::Receiver` usa `WeakEntity`. Con la referencia fuerte `stash` nunca
  corría y los agentes de la ventana cerrada seguían vivos pero huérfanos.
- Si dos ventanas se cerraran sin abrirse otra entre medio, las tandas se
  encolan; guardar una encima de la otra mataría a esos agentes.
- Prueba: `SPACE_RECYCLE=1` cierra la ventana a los 10 s y abre otra a los 2 s;
  `SPACE_RECYCLE=x` espera a que la cierres tú. Una ventana oculta de 1 px hace
  de notch (mantiene viva la aplicación).

Medición (`SPACE_DEMO=1 SPACE_BENCH=1`, build de desarrollo): 6 consolas,
4 escribiendo 30 líneas/s, como la prueba de `RENDIMIENTO_PIZARRA_AGENTES.md`.

| Fase | Cuadros/s | Intervalos > 12,5 ms | p95 | Pintar p95 | Armar p95 |
| --- | --- | --- | --- | --- | --- |
| Quieto | 164 | 2 % | 8,7 ms | 0,67 ms | 1,15 ms |
| Pan | 180 | 0 % | 7,2 ms | 0,71 ms | 1,07 ms |
| Zoom | 149 | 2 % | 10,2 ms | 7,59 ms | 0,93 ms |
| Zoom semántico | 149 | 1 % | 9,8 ms | 7,16 ms | 1,00 ms |

GPU 3D del proceso: 10–15 %. El costo del zoom son los cuadros en que entra
un tamaño de letra nuevo y hay que rasterizarlo.

## Herramientas de pantalla completa

- **Capturas** (`src/capture.rs`): la mira sobre la pantalla congelada con
  `atic-capture`. El frame va directo a una textura, sin JPEG ni WebView.
  - Después de capturar, el **estante** (`src/shelf.rs`) aparece abajo a la
    derecha con descartar, carpeta, Copiar, Dibujar y Texto (OCR de Windows,
    `src/ocr.rs`).
  - Se va solo a los 20 s, salvo con el cursor encima.
- **Pizarra** (`src/board.rs`): dibujar sobre la pantalla congelada.
  - Trazos suavizados con puntas redondas y borrador por trazo.
  - Shift hace líneas a 45°, cuadrados y círculos.
  - Exporta con `tiny-skia` a resolución física.
- **Color** (`src/color.rs`): el cuentagotas con lupa de 13×13 que lee bajo
  ella misma.
  - Las flechas mueven 1 px (10 px con Shift) y Tab cambia el formato.
  - R abre la rosa: muestra grande, saturación/brillo, matiz, HEX y recientes.
  - El color elegido cuelga del notch un momento.
- **Flip** (`src/flip.rs`): congela el monitor, toma la ventana que tenía el
  foco (su marco visible de DWM; ignora el escritorio, la barra de tareas y
  las ventanas de Atic), la esconde y la da vuelta 620 ms con perspectiva.
  - GPUI no tiene 3D: la tarjeta son 120 tiras verticales de la captura, cada
    una con su escala y sombra según la profundidad.
  - Esc o clic fuera la vuelve a poner en su sitio.
  - El reverso es el tablero de Atic (`src/flip_board.rs`): hojas de
    1200×900 que leen y escriben el mismo `notes/atic-tablero/note.json`
    (`ATIC_NOTES_DIR`). Tiene cajas de texto, listas, imágenes, tinta,
    deshacer/rehacer, páginas y autoguardado.
  - **Cajón**: historial, textos y capturas recientes para insertar.
  - **Exportar**: PNG, JPEG, PDF, Word y PowerPoint a
    `Documentos\Tableros de Atic\` (`src/flip_export.rs`, copiado de Atic).

Reuniones, Sistema y Más todavía solo imprimen su nombre.

## Limitaciones conocidas

- Solo el monitor principal; Atic acopla en los bordes exteriores de cualquier
  monitor.
- No hay fuente Aptos en esta máquina; se usa Segoe UI.
- En el vuelo al notch la gota viaja como círculo y al llegar pasa a tab de
  golpe (lo suaviza el aplastón). No hay morph continuo entre las formas.

## Ejecutar

Está fuera del workspace de Atic y usa su propio `target/` para no chocar con
`tauri dev`:

```bash
CARGO_TARGET_DIR="$LOCALAPPDATA/atic-gpui" cargo run
```

```bash
CARGO_TARGET_DIR="$LOCALAPPDATA/atic-gpui" cargo test
```

Variables para probar:

| Variable | Efecto |
| --- | --- |
| `PILL_OPEN=clipboard\|textos\|agentes` | Abre esa herramienta en el notch al arrancar |
| `PILL_OPEN=peek` | Deja abierto el vistazo 12 s |
| `PILL_OPEN_SEQ=clipboard,textos,cerrar` | Abre esas herramientas una tras otra con el notch abierto, cada `PILL_OPEN_STEP_MS` (1500 por omisión); `cerrar` lo cierra |
| `PILL_OPEN=capture\|board\|flip\|shelf` | Abre esa herramienta de pantalla completa |
| `PILL_OPEN=space` | Abre el espacio al arrancar |
| `SPACE_DEMO=1` | Abre 6 consolas de prueba (4 escribiendo 30 líneas/s) |
| `SPACE_BENCH=1` | Mueve la cámara sola por fases y mide, en stderr |
| `SPACE_SHELL=1` | Abre una PowerShell en el espacio |
| `SPACE_ALONE=1` | Solo el espacio, sin la pill |
| `SPACE_VIEW=pizarra` | Abre el espacio en la pizarra en vez del Mando |
| `SPACE_MANDO_DEMO=1` | Abre 4 consolas que fingen ser agentes (trabajando, turno que termina, proceso que termina y quieta) |
| `SPACE_RECYCLE=1\|x` | Prueba que los agentes sobreviven a cerrar la ventana del espacio: `1` la cierra y reabre sola, `x` espera a que la cierres tú |
| `PILL_TRAY_DEMO=1` | Llena la bandeja con una decisión y dos turnos terminados, y cuenta dos agentes trabajando de mentira (y muestra el mensaje rápido como si hubiera cuatro) |
| `PILL_TRAY_SEND=texto` | A los 5 s manda `texto` como mensaje rápido a los agentes del espacio |
| `PILL_MEDIA_DEMO=1` | Una pista inventada sonando, sin tocar tu reproductor |
| `PILL_FPS=1` | Cuadros por segundo y el peor cuadro, en stderr |
| `PILL_DEBUG=1` | Clics, arrastres y acoplados |
| `PILL_FLIP_HWND=<hex>` | La ventana a voltear |
| `PILL_FLIP_ANGLE=0..1` | Congela el giro |
| `PILL_FLIP_NOHIDE=1` | No esconde la ventana volteada |
| `PILL_PAPER_DEMO=1` | Llena la hoja con ejemplos sin guardarlos |
| `PILL_PAPER_DRAWER=clip\|texts\|caps` | Abre el cajón |
| `PILL_LAUNCHER_KEY=ctrl-space` | Usa el atajo de Atic para el lanzador |
