# Prototipo de la pill en GPUI

Experimento para evaluar portar la pill de Atic de Svelte/WebView2 a
[GPUI](https://crates.io/crates/gpui). Por ahora solo funciona en Windows.

Incluye:

- El tab acoplado (124 × 40) a cualquier borde sin barra de tareas, con la
  marca de Atic: sus ojos siguen al cursor y parpadea. En los bordes laterales
  va de pie y la tira de herramientas se abre en columna.
- Arrastrar la pill (`src/geometry.rs`, reglas de `edgeDock.ts`):
  - Se arrastra desde cualquier parte del tab o de la gota pasados 4 px; un
    movimiento menor cuenta como clic.
  - Alejarla más de 64 px del borde la convierte en una gota flotante de 52 px.
    Cerca de un borde, un cuello líquido la une a él.
  - Al soltarla a 28 px o menos de un borde se acopla con un aplastón de
    125 ms; arriba siempre al centro.
  - Pasar el cursor 180 ms sobre la gota abre la rueda, metida dentro de la
    pantalla.
  - La posición se guarda en `%LOCALAPPDATA%tic-gpui\home.txt`.
- La tira de herramientas al pasar el cursor: 240 ms con `--ease-island` y
  150 ms de gracia al salir.
- La rueda de 9 herramientas. Desde el tab se abre con clic en la marca o con
  Alt+Z; desde la gota, al pasar el cursor. Se cierra con clic al centro, al
  elegir una herramienta o al alejarse.
- La animación líquida: la gota baja desde el tab y las herramientas brotan
  con cuellos tipo metaball. Es solo animación; no hay deformación interactiva.
- Una ventana transparente, siempre encima, que no roba el foco y deja pasar
  los clics fuera de la pill.
- El panel del Clipboard (312 × 372). Se abre con Clipboard en la tira o en la
  rueda: una semilla que gotea de la pill, crece y se separa. Acoplada, se abre
  hacia el centro de la pantalla; flotante, abajo, arriba, a la derecha o a la
  izquierda, donde quepa.
  - Tiene buscador con IME y portapapeles, filtros por tipo y de favoritos, y
    una lista virtualizada de 100 entradas de prueba (texto, colores e
    imágenes).
  - Pega como Atic: devuelve el foco a la ventana anterior, escribe el
    portapapeles y manda Ctrl+V.
  - Teclas: ↑/↓, Enter pega y Esc cierra. Un clic fuera lo cierra, salvo que
    esté fijado.
  - Arrastre a otras apps (OLE, `src/drag.rs`, copiado de Atic): el texto va
    como Unicode, ANSI, OEM y Locale; las imágenes, como archivo PNG
    (`CF_HDROP`) escrito en `%TEMP%tic-gpui-drag`. Empieza al mover el
    cursor 6 px con el botón apretado.
  - Si sueltas texto sobre algo que no lo acepta (una consola) o sobre una
    terminal web, pega como respaldo con Ctrl+V o Ctrl+Shift+V según la app
    (`src/paste.rs`, de `clipboard_history.rs`).

Fuera del Clipboard no hay lógica real: elegir otra herramienta solo imprime
su nombre.

## Limitaciones conocidas

- GPUI copia imágenes al portapapeles solo en formato PNG, sin `CF_DIB`. Las
  apps que esperan un bitmap (Paint, por ejemplo) no las pegan.
- No hay fuente Aptos en esta máquina; se usa Segoe UI.
- Falta la vista previa grande al pasar el cursor.
- Solo el monitor principal; Atic acopla en los bordes exteriores de cualquier
  monitor.
- El panel no "vuela de vuelta" la pill a su lugar al abrir una herramienta.

## Ejecutar

Está fuera del workspace de Atic y usa su propio `target/` para no chocar con
`tauri dev`:

```bash
CARGO_TARGET_DIR="$LOCALAPPDATA/atic-gpui" cargo run
```

Con `PILL_FPS=1` imprime los cuadros por segundo en stderr y con
`PILL_DEBUG=1`, los clics, arrastres y acoplados.

```bash
CARGO_TARGET_DIR="$LOCALAPPDATA/atic-gpui" cargo test
```
