<script lang="ts">
  /**
   * Dibujar encima de una captura: flechas, círculos, trazo libre, resaltador.
   *
   * La ventana la abre Rust ya con el tamaño de la imagen (`annotate.rs`), así
   * que acá no hay geometría de ventana: solo el lienzo y la barra.
   *
   * **Un solo canvas, a resolución natural.** La imagen y las formas se pintan
   * en el mismo sitio, y por eso exportar es `toDataURL` y nada más: lo que se
   * copia es, literalmente, lo que se vio. Con dos capas habría que componerlas
   * al exportar, que es el momento exacto en el que un desajuste de escala
   * aparece y ya no se puede corregir.
   *
   * Las decisiones —qué forma nace, cuándo deja de ser un temblor, cómo se
   * deshace— viven en `annotateModel.ts`, y cómo se pinta cada una en
   * `annotateDraw.ts`. Acá queda el estado, los eventos y los viajes a Rust.
   */
  import {
    Circle,
    Copy,
    Highlighter,
    MoveUpRight,
    Pencil,
    Redo2,
    Save,
    Square,
    Undo2,
    X,
  } from "$lib/icons";
  import Icon from "$ui/Icon.svelte";
  import {
    annotationImage,
    closeAnnotator,
    copyAnnotation,
    onAnnotateOpen,
    pendingAnnotation,
    saveAnnotation,
  } from "$ipc/annotate";
  import { captureSrc } from "$ipc/captures";
  import { startResizeDragging } from "$ipc/windows";
  import { drawShape, drawShapes } from "./annotateDraw";
  import {
    beginShape,
    clampToImage,
    COLORS,
    commit,
    emptyState,
    extendShape,
    redo,
    strokeWidth,
    toImagePoint,
    toolForKey,
    undo,
    WIDTH_LEVELS,
    type AnnotateState,
    type AnnotateTool,
    type Shape,
    type WidthLevel,
  } from "./annotateModel";

  const TOOLS: { id: AnnotateTool; icon: typeof Pencil; label: string; key: string }[] =
    [
      { id: "pen", icon: Pencil, label: "Lápiz", key: "1" },
      { id: "arrow", icon: MoveUpRight, label: "Flecha", key: "2" },
      { id: "ellipse", icon: Circle, label: "Círculo", key: "3" },
      { id: "rect", icon: Square, label: "Rectángulo", key: "4" },
      { id: "highlight", icon: Highlighter, label: "Resaltador", key: "5" },
    ];

  /** Cuánto queda el aviso antes de cerrar. Da tiempo a leerlo, no a esperar. */
  const NOTE_MS = 900;

  let doc = $state<AnnotateState>(emptyState());
  /** La forma que se está arrastrando ahora. Fuera de `doc` hasta soltarla. */
  let live = $state<Shape | null>(null);

  let tool = $state<AnnotateTool>("arrow");
  let color = $state<string>(COLORS[0]);
  let level = $state<WidthLevel>(2);

  let canvas = $state<HTMLCanvasElement | null>(null);
  let natural = $state({ width: 0, height: 0 });
  let ready = $state(false);
  let busy = $state(false);
  let note = $state<string | null>(null);
  let error = $state<string | null>(null);
  /**
   * Escape con trazos pide confirmación en el propio botón, como el borrado de
   * hilos: descartar un dibujo con una tecla suelta no tiene vuelta atrás.
   */
  let confirmDiscard = $state(false);

  /**
   * La captura ya decodificada.
   *
   * `$state.raw` y no `$state`: hace falta que asignarla despierte al efecto
   * que redibuja, pero no que Svelte le arme un proxy profundo a un elemento
   * del DOM.
   */
  let image = $state.raw<HTMLImageElement | null>(null);
  /** Qué captura hay cargada: evita recargar (y borrar el dibujo) al reenfocar. */
  let loadedPath: string | null = null;
  /** Sube con cada apertura: descarta la carga de la captura anterior. */
  let token = 0;
  let noteTimer: ReturnType<typeof setTimeout> | null = null;

  const width = $derived(strokeWidth(level, natural.width || 1280));
  const canUndo = $derived(doc.shapes.length > 0);
  const canRedo = $derived(doc.undone.length > 0);

  function redraw() {
    const ctx = canvas?.getContext("2d");
    if (!ctx || !image) return;
    ctx.clearRect(0, 0, natural.width, natural.height);
    ctx.drawImage(image, 0, 0, natural.width, natural.height);
    drawShapes(ctx, doc.shapes);
    if (live) drawShape(ctx, live);
  }

  $effect(() => {
    /*
     * Las dependencias se leen ANTES de llamar a `redraw`, y a propósito.
     *
     * Un `$effect` solo queda suscrito a lo que alcanzó a leer mientras corría.
     * `redraw` sale temprano si todavía no hay imagen —que es exactamente el
     * estado de la primera pasada, al montar—, así que nunca llegaba a leer los
     * trazos y se quedaba sordo a ellos para siempre: se dibujaba en el modelo
     * y el lienzo no se enteraba. El síntoma era «no puedo dibujar».
     */
    void doc.shapes;
    void live;
    void natural;
    void image;
    redraw();
  });

  /**
   * Carga la captura pendiente, si cambió.
   *
   * Se llama de tres sitios —al montar, al volverse visible la ventana y al
   * llegar el evento— y por eso compara contra lo ya cargado: sin eso, volver
   * a enfocar la ventana borraría el dibujo en curso.
   */
  async function refresh() {
    try {
      const target = await pendingAnnotation();
      if (!target || target.path === loadedPath) return;
      await load(target.path, { width: target.width, height: target.height });
    } catch (err) {
      error = String(err);
    }
  }

  async function load(path: string, size: { width: number; height: number }) {
    const mine = ++token;
    reset();
    loadedPath = path;
    natural = size;

    /*
     * Camino rápido: el PNG por el protocolo de assets, el mismo que usa el
     * estante. Responde con `Access-Control-Allow-Origin` igual al origen de la
     * ventana (`tauri/src/protocol/asset.rs`), así que con `crossOrigin` el
     * canvas NO queda contaminado y `toDataURL` sigue funcionando. El sufijo
     * evita que el webview sirva una captura anterior desde su caché.
     */
    if (await paint(mine, `${captureSrc(path)}?t=${Date.now()}`, true)) return;
    if (mine !== token) return;

    /*
     * Reserva: un data URL es del mismo origen por definición, así que carga
     * aunque el CORS no aplique. Cuesta pasar la imagen entera en base64 por el
     * IPC —notorio en una captura grande—, y por eso es la segunda opción y no
     * la primera.
     */
    try {
      const data = await annotationImage(path);
      if (mine !== token) return;
      if (!(await paint(mine, data, false))) {
        if (mine === token) error = "No se pudo abrir la captura";
      }
    } catch (err) {
      if (mine !== token) return;
      error = String(err);
    }
  }

  /** Intenta pintar `src` en el lienzo. `false` = no cargó. */
  function paint(mine: number, src: string, cors: boolean): Promise<boolean> {
    return new Promise((resolve) => {
      const img = new Image();
      if (cors) img.crossOrigin = "anonymous";
      img.onload = () => {
        if (mine !== token) return resolve(true);
        image = img;
        // Si el IHDR y la imagen no coincidieran, manda la imagen: el lienzo
        // tiene que medir lo que se va a exportar.
        natural = { width: img.naturalWidth, height: img.naturalHeight };
        ready = true;
        redraw();
        resolve(true);
      };
      img.onerror = () => resolve(false);
      img.src = src;
    });
  }

  function reset() {
    doc = emptyState();
    live = null;
    image = null;
    ready = false;
    busy = false;
    error = null;
    note = null;
    confirmDiscard = false;
    loadedPath = null;
    if (noteTimer) clearTimeout(noteTimer);
    noteTimer = null;
  }

  $effect(() => {
    const pending = onAnnotateOpen((payload) => {
      if (payload.path === loadedPath) return;
      void load(payload.path, { width: payload.width, height: payload.height });
    });

    // `visibilitychange` es del propio documento, sin IPC de por medio: es lo
    // único que no depende de que el renderer estuviera despierto cuando Rust
    // mandó el aviso. Al mostrarse la ventana, el editor va a buscar su imagen.
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    void refresh();

    return () => {
      void pending.then((off) => off());
      document.removeEventListener("visibilitychange", onVisible);
      if (noteTimer) clearTimeout(noteTimer);
    };
  });

  // --- Dibujo ---

  function pointFor(event: PointerEvent) {
    const rect = canvas?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    return clampToImage(
      toImagePoint({ x: event.clientX, y: event.clientY }, rect, natural),
      natural,
    );
  }

  function onPointerDown(event: PointerEvent) {
    if (!ready || busy || event.button !== 0) return;
    confirmDiscard = false;
    live = beginShape(tool, color, width, pointFor(event));
    // La captura del puntero —para que soltar fuera del lienzo cierre la forma
    // igual— va DESPUÉS de abrir el trazo y dentro de un `try`: si fallara,
    // antes se llevaba puesto el `beginShape` de la línea de arriba y no se
    // dibujaba nada. Sin captura solo se pierde el arrastre fuera del borde.
    try {
      canvas?.setPointerCapture(event.pointerId);
    } catch {
      // El puntero ya no está activo. No es fatal.
    }
  }

  function onPointerMove(event: PointerEvent) {
    if (!live) return;
    live = extendShape(live, pointFor(event));
  }

  function onPointerUp(event: PointerEvent) {
    if (!live) return;
    // `pointercancel` llega con la captura ya soltada por el navegador, y
    // liberarla dos veces lanza. Preguntar es más barato que un try/catch.
    if (canvas?.hasPointerCapture(event.pointerId)) {
      canvas.releasePointerCapture(event.pointerId);
    }
    doc = commit(doc, live);
    live = null;
  }

  // --- Salidas ---

  /**
   * El lienzo como PNG, o `null` si no se pudo.
   *
   * `toDataURL` lanza si el canvas quedó contaminado por una imagen de otro
   * origen. No debería pasar —el protocolo de assets manda el CORS y el
   * respaldo es un data URL—, pero de fallar en silencio, copiar y guardar no
   * harían nada sin decir por qué.
   */
  function exportPng(): string | null {
    try {
      return canvas?.toDataURL("image/png") ?? null;
    } catch {
      return null;
    }
  }

  function finish(message: string) {
    note = message;
    if (noteTimer) clearTimeout(noteTimer);
    noteTimer = setTimeout(() => {
      void closeAnnotator().catch(() => {});
      reset();
    }, NOTE_MS);
  }

  async function copy() {
    if (!ready || busy) return;
    const png = exportPng();
    if (!png) {
      error = "No se pudo leer el lienzo";
      return;
    }
    busy = true;
    try {
      await copyAnnotation(png);
      finish("Copiada al portapapeles");
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }

  async function save() {
    if (!ready || busy) return;
    const png = exportPng();
    if (!png) {
      error = "No se pudo leer el lienzo";
      return;
    }
    busy = true;
    try {
      await saveAnnotation(png);
      finish("Guardada como captura nueva");
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }

  function close() {
    void closeAnnotator().catch(() => {});
    reset();
  }

  /** Cerrar sin guardar: con trazos encima, pide confirmar una vez. */
  function requestClose() {
    if (doc.shapes.length > 0 && !confirmDiscard) {
      confirmDiscard = true;
      return;
    }
    close();
  }

  function isButton(target: EventTarget | null): boolean {
    return target instanceof HTMLElement && target.closest("button") !== null;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      requestClose();
      return;
    }
    const mod = event.ctrlKey || event.metaKey;
    if (mod && event.key.toLowerCase() === "z") {
      event.preventDefault();
      doc = event.shiftKey ? redo(doc) : undo(doc);
      return;
    }
    if (mod && event.key.toLowerCase() === "y") {
      event.preventDefault();
      doc = redo(doc);
      return;
    }
    if (mod && event.key.toLowerCase() === "c") {
      event.preventDefault();
      void copy();
      return;
    }
    /*
     * Guardar es Ctrl+Enter y no el Ctrl+S de costumbre.
     *
     * `desktopChrome.ts` se traga Ctrl+S en fase de captura sobre `window` con
     * `stopImmediatePropagation` —es el «guardar página» del navegador, que en
     * una app de escritorio no significa nada—, así que nunca llegaría acá.
     * Anunciarlo en la barra habría sido prometer una tecla muerta.
     */
    if (mod && event.key === "Enter") {
      event.preventDefault();
      void save();
      return;
    }
    // Enter sobre un botón enfocado ya es «pulsar ese botón»: robárselo para
    // copiar haría que elegir una herramienta con el teclado copiara la imagen.
    if (event.key === "Enter" && !mod && !isButton(event.target)) {
      event.preventDefault();
      void copy();
      return;
    }
    if (mod || event.altKey) return;
    const next = toolForKey(event.key);
    if (next) {
      event.preventDefault();
      tool = next;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="editor">
  <div class="bar" data-tauri-drag-region>
    <div class="group" role="radiogroup" aria-label="Herramienta">
      {#each TOOLS as item (item.id)}
        <button
          type="button"
          class="tool"
          class:is-on={tool === item.id}
          role="radio"
          aria-checked={tool === item.id}
          title="{item.label} ({item.key})"
          aria-label={item.label}
          onclick={() => (tool = item.id)}
        >
          <Icon icon={item.icon} size={15} />
        </button>
      {/each}
    </div>

    <div class="group" role="radiogroup" aria-label="Color">
      {#each COLORS as swatch (swatch)}
        <button
          type="button"
          class="swatch"
          class:is-on={color === swatch}
          style="--swatch: {swatch}"
          role="radio"
          aria-checked={color === swatch}
          aria-label="Color {swatch}"
          onclick={() => (color = swatch)}
        ></button>
      {/each}
    </div>

    <div class="group" role="radiogroup" aria-label="Grosor">
      {#each WIDTH_LEVELS as value (value)}
        <button
          type="button"
          class="width"
          class:is-on={level === value}
          role="radio"
          aria-checked={level === value}
          aria-label="Grosor {value}"
          onclick={() => (level = value)}
        >
          <span class="width-dot" style="--dot: {2 + value * 2}px"></span>
        </button>
      {/each}
    </div>

    <div class="group">
      <button
        type="button"
        class="tool"
        disabled={!canUndo}
        title="Deshacer (Ctrl+Z)"
        aria-label="Deshacer"
        onclick={() => (doc = undo(doc))}
      >
        <Icon icon={Undo2} size={15} />
      </button>
      <button
        type="button"
        class="tool"
        disabled={!canRedo}
        title="Rehacer (Ctrl+Shift+Z)"
        aria-label="Rehacer"
        onclick={() => (doc = redo(doc))}
      >
        <Icon icon={Redo2} size={15} />
      </button>
    </div>

    <div class="spacer" data-tauri-drag-region></div>

    <div class="group is-actions">
      <button
        type="button"
        class="action is-primary"
        disabled={!ready || busy}
        title="Copiar al portapapeles (Enter)"
        onclick={() => void copy()}
      >
        <Icon icon={Copy} size={14} />
        <span>Copiar</span>
      </button>
      <button
        type="button"
        class="action"
        disabled={!ready || busy}
        title="Guardar como captura nueva; aparece en el estante (Ctrl+Enter)"
        onclick={() => void save()}
      >
        <Icon icon={Save} size={14} />
        <span>Guardar</span>
      </button>
      <button
        type="button"
        class="action"
        class:is-danger={confirmDiscard}
        title="Cerrar sin guardar (Esc)"
        aria-label={confirmDiscard ? "Descartar el dibujo" : "Cerrar"}
        onclick={requestClose}
      >
        {#if confirmDiscard}
          <span>¿Descartar?</span>
        {:else}
          <Icon icon={X} size={14} />
        {/if}
      </button>
    </div>
  </div>

  <div class="stage">
    <canvas
      bind:this={canvas}
      width={natural.width || 1}
      height={natural.height || 1}
      class="canvas"
      class:is-ready={ready}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
    ></canvas>
  </div>

  <p
    class="status"
    class:is-note={Boolean(note)}
    class:is-error={Boolean(error)}
    data-tauri-drag-region
  >
    <!-- Sin lienzo listo NO se muestra la ayuda de dibujo: decir «arrastrá para
         dibujar» sobre un editor que todavía no acepta el puntero es lo que
         hizo que un fallo de carga se leyera como «no anda el dibujo». -->
    {error ??
      note ??
      (ready
        ? "Arrastrá para dibujar · Enter copia · Ctrl+Enter guarda · Esc cierra"
        : "Cargando la captura…")}
  </p>

  <!--
    Asa de redimensionado propia. La ventana no tiene decoraciones y el borde
    nativo queda debajo del webview: sin esto, agrandar el editor no es posible
    desde el contenido.
  -->
  <button
    type="button"
    class="grip"
    aria-label="Redimensionar"
    onpointerdown={(event) => {
      event.preventDefault();
      void startResizeDragging("SouthEast").catch(() => {});
    }}
  ></button>
</div>

<style>
  /* Ventana transparente y sin marco: lo único que se ve es este panel. */
  :global(html),
  :global(body) {
    overflow: hidden;
    margin: 0;
    background: transparent;
  }

  .editor {
    position: relative;
    display: flex;
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    flex-direction: column;
    padding: 10px;
    border-radius: var(--radius-md);
    background: var(--surface-2);
    box-shadow: var(--shadow-float);
    color: var(--text);
    gap: 8px;
    outline: 1px solid var(--line);
    outline-offset: -1px;
  }

  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
  }

  .group {
    display: flex;
    align-items: center;
    padding: 2px;
    border-radius: var(--radius-sm);
    background: var(--elevated);
    gap: 2px;
  }

  .group.is-actions {
    padding: 0;
    background: transparent;
    gap: 6px;
  }

  /* Hueco entre los controles y las acciones. Es la zona ancha por la que se
     agarra la ventana, que no tiene barra de título. */
  .spacer {
    flex: 1;
    align-self: stretch;
  }

  .tool,
  .width {
    display: inline-flex;
    width: 28px;
    height: 26px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    transition:
      background var(--duration-quick) var(--ease-out),
      color var(--duration-quick) var(--ease-out);
  }

  .tool:hover:not(:disabled),
  .width:hover {
    background: color-mix(in sRGB, var(--text) 10%, transparent);
    color: var(--text);
  }

  .tool.is-on,
  .width.is-on {
    background: color-mix(in sRGB, var(--accent) 22%, transparent);
    color: var(--text);
  }

  .tool:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .width-dot {
    width: var(--dot);
    height: var(--dot);
    border-radius: var(--radius-pill);
    background: currentColor;
  }

  /*
   * El color va en un pseudo y no en el fondo del botón: el anillo de
   * selección tiene que poder pintarse por fuera de la muestra, o sobre el
   * blanco desaparecería contra su propio color.
   */
  .swatch {
    position: relative;
    width: 20px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    cursor: pointer;
  }

  .swatch::after {
    position: absolute;
    inset: 4px;
    border-radius: var(--radius-pill);
    background: var(--swatch);
    box-shadow: 0 0 0 1px rgb(0 0 0 / 25%) inset;
    content: "";
    transition: inset var(--duration-quick) var(--ease-out);
  }

  .swatch.is-on::after {
    inset: 2px;
    box-shadow:
      0 0 0 1px rgb(0 0 0 / 25%) inset,
      0 0 0 2px var(--surface-2),
      0 0 0 3px var(--text);
  }

  .action {
    display: inline-flex;
    height: 26px;
    align-items: center;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-xs);
    background: var(--elevated);
    color: var(--text);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    gap: 6px;
    transition:
      background var(--duration-quick) var(--ease-out),
      color var(--duration-quick) var(--ease-out);
  }

  .action:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--text) 14%, transparent);
  }

  .action:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* Copiar es la salida por defecto: es la única que va en tinta invertida. */
  .action.is-primary {
    background: var(--accent);
    color: var(--on-accent);
  }

  .action.is-danger {
    background: var(--danger);
    color: var(--on-accent);
  }

  /*
   * Damero bajo el lienzo: dice dónde termina la captura sin necesidad de un
   * borde, que sobre una captura de fondo claro sería invisible.
   */
  .stage {
    display: flex;
    min-height: 0;
    flex: 1;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: repeating-conic-gradient(
        color-mix(in sRGB, var(--text) 4%, transparent) 0% 25%,
        transparent 0% 50%
      )
      50% / 16px 16px;
    overflow: hidden;
  }

  .canvas {
    /* `auto` y no `100%`: el canvas ya trae su relación de aspecto en los
       atributos, y forzarle un lado lo deformaría. */
    width: auto;
    max-width: 100%;
    height: auto;
    max-height: 100%;
    border-radius: var(--radius-xs);
    opacity: 0;
    cursor: crosshair;
    touch-action: none;
    transition: opacity var(--duration-quick) var(--ease-out);
  }

  .canvas.is-ready {
    opacity: 1;
  }

  .status {
    margin: 0;
    color: var(--muted);
    font-size: 11px;
    text-align: center;
  }

  /* Diagonal de dos rayas, como el asa de cualquier ventana redimensionable. */
  .grip {
    position: absolute;
    right: 2px;
    bottom: 2px;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    background:
      linear-gradient(
        135deg,
        transparent 0 45%,
        var(--muted) 45% 55%,
        transparent 55% 100%
      ),
      linear-gradient(
          135deg,
          transparent 0 45%,
          var(--muted) 45% 55%,
          transparent 55% 100%
        )
        4px 4px / 100% 100% no-repeat;
    cursor: nwse-resize;
    opacity: 0.5;
  }

  .grip:hover {
    opacity: 1;
  }

  .status.is-note {
    color: var(--text);
  }

  .status.is-error {
    color: var(--danger);
  }

  @media (prefers-reduced-motion: reduce) {
    .tool,
    .width,
    .action,
    .canvas,
    .swatch::after {
      transition: none;
    }
  }
</style>
