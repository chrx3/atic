<script lang="ts">
  /**
   * Dibujar encima de una captura: flechas, círculos, trazo libre, resaltador,
   * texto y recorte.
   *
   * La ventana la abre Rust ya con el tamaño de la imagen (`annotate.rs`), así
   * que acá no hay geometría de ventana: solo el lienzo y la barra.
   *
   * Las dos últimas no son formas como las otras. El texto se escribe en un
   * cuadro que flota sobre el lienzo y recién al confirmar deja una forma. El
   * recorte no deja ninguna: mueve el encuadre por el que se mira la imagen
   * —ver `crop`—, y por eso se puede quitar sin haber perdido nada.
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
    Crop,
    Highlighter,
    MoveUpRight,
    Pencil,
    Redo2,
    Save,
    Square,
    Type,
    Undo2,
    X,
  } from "$lib/icons";
  import Icon from "$ui/Icon.svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    annotationImage,
    closeAnnotator,
    copyAnnotation,
    onAnnotateOpen,
    pendingAnnotation,
    saveAnnotation,
  } from "$ipc/annotate";
  import type { AnnotateMode, FocusRect } from "$core/types";
  import { captureSrc } from "$ipc/captures";
  import { startResizeDragging } from "$ipc/windows";
  import { drawCropMask, drawShape, drawShapes, fontFor, haloFor } from "./annotateDraw";
  import {
    beginShape,
    COLORS,
    commit,
    cropRect,
    emptyState,
    extendShape,
    isDragTool,
    LINE_HEIGHT,
    redo,
    strokeWidth,
    textSize,
    toImagePoint,
    toolForKey,
    undo,
    WIDTH_LEVELS,
    type AnnotateState,
    type AnnotateTool,
    type Point,
    type Rect,
    type Shape,
    type WidthLevel,
  } from "./annotateModel";

  const TOOLS = $derived(
    [
      { id: "pen" as const, icon: Pencil, label: t("page.annotate.pen"), key: "1" },
      { id: "arrow" as const, icon: MoveUpRight, label: t("page.annotate.arrow"), key: "2" },
      { id: "ellipse" as const, icon: Circle, label: t("page.annotate.ellipse"), key: "3" },
      { id: "rect" as const, icon: Square, label: t("page.annotate.rect"), key: "4" },
      { id: "highlight" as const, icon: Highlighter, label: t("page.annotate.highlight"), key: "5" },
      { id: "text" as const, icon: Type, label: t("page.annotate.text"), key: "6" },
      { id: "crop" as const, icon: Crop, label: t("page.annotate.crop"), key: "7" },
    ],
  );

  /** Cuánto queda el aviso antes de cerrar. Da tiempo a leerlo, no a esperar. */
  const NOTE_MS = 900;

  /** Más que esto y mover la barra fue a propósito, no un temblor al hacer clic. */
  const BAR_DRAG_THRESHOLD = 4;

  let doc = $state<AnnotateState>(emptyState());
  /** La forma que se está arrastrando ahora. Fuera de `doc` hasta soltarla. */
  let live = $state<Shape | null>(null);

  let tool = $state<AnnotateTool>("arrow");
  let color = $state<string>(COLORS[0]);
  let level = $state<WidthLevel>(2);

  /**
   * Panel o pizarra.
   *
   * Es lo unico que cambia entre las dos: la pizarra cubre el escritorio sobre
   * la pantalla congelada, el panel es una ventana del tamano de la captura. El
   * motor de dibujo no distingue, y ese era el punto de la fase 1.
   */
  let mode = $state<AnnotateMode>("panel");
  /** Monitor donde van los controles, en pixeles de la imagen. */
  let focus = $state<FocusRect | null>(null);
  /** Cuanto movio el usuario la barra desde su sitio, en pixeles CSS. */
  let barShift = $state({ x: 0, y: 0 });

  /**
   * El recorte, como encuadre y no como tijera.
   *
   * Recortar no toca ni la imagen ni los trazos: mueve la ventana por la que se
   * los mira. El lienzo pasa a medir el recorte y todo se dibuja corrido, así
   * que lo exportado ya sale recortado —es el canvas de siempre— y quitar el
   * recorte devuelve lo que había, incluido lo dibujado afuera.
   */
  let crop = $state<Rect | null>(null);
  /** El recuadro mientras se arrastra. Se ve como velo, todavía no recorta. */
  let pendingCrop = $state<Rect | null>(null);
  /** Esquina donde arrancó el arrastre del recorte. No reactivo: no se pinta. */
  let cropFrom: Point | null = null;

  /** El cuadro de texto abierto, si hay uno. `at` en píxeles de la imagen. */
  let editing = $state<{ at: Point; text: string } | null>(null);
  let textEl = $state<HTMLElement | null>(null);

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

  /*
   * Donde van los controles en la pizarra, en porcentaje de la imagen.
   *
   * En porcentaje y no en pixeles medidos: el lienzo cubre la ventana entera,
   * asi que un % de la imagen es el mismo % de la pantalla, sin importar el DPI
   * ni tener que esperar a que el DOM mida nada.
   */
  const anchorX = $derived(
    focus && natural.width ? ((focus.x + focus.width / 2) / natural.width) * 100 : 50,
  );
  const anchorTop = $derived(
    focus && natural.height ? (focus.y / natural.height) * 100 : 0,
  );
  const anchorBottom = $derived(
    focus && natural.height ? ((focus.y + focus.height) / natural.height) * 100 : 100,
  );
  const boardBarStyle = $derived(
    `left: ${anchorX}%; top: ${anchorTop}%; --shift-x: ${barShift.x}px; --shift-y: ${barShift.y}px;`,
  );
  const boardStatusStyle = $derived(`left: ${anchorX}%; top: ${anchorBottom}%;`);

  /**
   * Arrastrar la barra la mueve A ELLA, no a la ventana.
   *
   * En la pizarra la ventana ES la pantalla congelada: dejarle el
   * `data-tauri-drag-region` hacia que arrastrar la barra corriera el pantallazo
   * entero. Acá se mueve solo la barra, y el congelado queda donde estaba.
   */
  function onBarDown(event: PointerEvent) {
    if (mode !== "board") return;
    // Se agarra desde cualquier parte, botones incluidos: el umbral de abajo es
    // el que separa un clic de un arrastre, así que no hace falta reservar
    // zonas muertas en una barra que ya es angosta.
    const el = event.currentTarget as HTMLElement;
    const start = { x: event.clientX, y: event.clientY };
    const from = { ...barShift };
    /*
     * La captura del puntero se pide recién cuando el puntero SE MOVIÓ.
     *
     * Capturarla en el `pointerdown` se comía el clic: con la captura puesta,
     * el `click` se dispara contra quien capturó —la barra— y nunca contra el
     * botón, así que elegir herramienta no hacía nada. Es la misma regla que
     * usa el estante para distinguir clic de arrastre: si se soltó donde
     * empezó, fue un clic.
     */
    let dragging = false;
    const move = (e: PointerEvent) => {
      const dx = e.clientX - start.x;
      const dy = e.clientY - start.y;
      if (!dragging) {
        if (Math.hypot(dx, dy) < BAR_DRAG_THRESHOLD) return;
        dragging = true;
        try {
          el.setPointerCapture(e.pointerId);
        } catch {
          // Sin captura se sigue: el arrastre se corta al salir de la barra.
        }
      }
      barShift = { x: from.x + dx, y: from.y + dy };
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    // En `window` y no en la barra: hasta que se pide la captura, un movimiento
    // rápido que ya salió de la barra no le llegaría, y el arrastre no
    // arrancaría nunca.
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  const width = $derived(strokeWidth(level, natural.width || 1280));
  /** El cuerpo del texto, en píxeles de la imagen. */
  const fontSize = $derived(textSize(level, natural.width || 1280));
  const canUndo = $derived(doc.shapes.length > 0);
  const canRedo = $derived(doc.undone.length > 0);

  /** Lo que se ve: el recorte, o la imagen entera. En píxeles de la imagen. */
  const view = $derived<Rect>(
    crop ?? { x: 0, y: 0, w: natural.width, h: natural.height },
  );

  function redraw() {
    const ctx = canvas?.getContext("2d");
    if (!ctx || !image) return;
    // El recorte se aplica corriendo el origen y no recortando cada forma: así
    // el resto del dibujo no sabe que existe, y lo que quedó afuera sigue ahí
    // para cuando se lo quite.
    ctx.setTransform(1, 0, 0, 1, -view.x, -view.y);
    ctx.clearRect(view.x, view.y, view.w, view.h);
    ctx.drawImage(image, 0, 0, natural.width, natural.height);
    drawShapes(ctx, doc.shapes);
    if (live) drawShape(ctx, live);
    if (pendingCrop) drawCropMask(ctx, pendingCrop, view);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
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
    void view;
    void pendingCrop;
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
      await load(target.path, target);
    } catch (err) {
      error = String(err);
    }
  }

  async function load(
    path: string,
    open: {
      width: number;
      height: number;
      mode: AnnotateMode;
      focus: FocusRect | null;
    },
  ) {
    const mine = ++token;
    reset();
    loadedPath = path;
    natural = { width: open.width, height: open.height };
    mode = open.mode;
    focus = open.focus;

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
        if (mine === token) error = t("page.annotate.openFail");
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
    crop = null;
    pendingCrop = null;
    cropFrom = null;
    editing = null;
    image = null;
    ready = false;
    busy = false;
    error = null;
    note = null;
    confirmDiscard = false;
    loadedPath = null;
    mode = "panel";
    focus = null;
    barShift = { x: 0, y: 0 };
    if (noteTimer) clearTimeout(noteTimer);
    noteTimer = null;
  }

  $effect(() => {
    const pending = onAnnotateOpen((payload) => {
      if (payload.path === loadedPath) return;
      void load(payload.path, payload);
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

  /**
   * Punto del ratón → píxel de la imagen, pasando por el encuadre.
   *
   * El lienzo mide el recorte, así que `toImagePoint` devuelve coordenadas
   * DENTRO del recorte: sumarle su origen es lo que mantiene a las formas en
   * píxeles de la imagen, que es donde viven aunque el encuadre cambie.
   */
  function pointFor(event: PointerEvent): Point {
    const rect = canvas?.getBoundingClientRect();
    if (!rect) return { x: 0, y: 0 };
    const local = toImagePoint({ x: event.clientX, y: event.clientY }, rect, {
      width: view.w,
      height: view.h,
    });
    return {
      x: Math.min(Math.max(local.x + view.x, view.x), view.x + view.w),
      y: Math.min(Math.max(local.y + view.y, view.y), view.y + view.h),
    };
  }

  function capturePointer(event: PointerEvent) {
    // La captura del puntero —para que soltar fuera del lienzo cierre el gesto
    // igual— va DESPUÉS de abrirlo y dentro de un `try`: si fallara, antes se
    // llevaba puesto el `beginShape` y no se dibujaba nada. Sin captura solo se
    // pierde el arrastre fuera del borde.
    try {
      canvas?.setPointerCapture(event.pointerId);
    } catch {
      // El puntero ya no está activo. No es fatal.
    }
  }

  function onPointerDown(event: PointerEvent) {
    if (!ready || busy || event.button !== 0) return;
    confirmDiscard = false;
    const at = pointFor(event);

    // Con un cuadro abierto, el primer clic afuera lo cierra y no abre otro:
    // es lo que hace cualquier editor, y sin eso escribir dos textos seguidos
    // dejaba el primero a medias.
    if (editing) {
      commitText();
      if (tool === "text") return;
    }

    if (tool === "text") {
      startText(at);
      return;
    }

    if (tool === "crop") {
      cropFrom = at;
      pendingCrop = null;
      capturePointer(event);
      return;
    }

    if (!isDragTool(tool)) return;
    live = beginShape(tool, color, width, at);
    capturePointer(event);
  }

  function onPointerMove(event: PointerEvent) {
    if (cropFrom) {
      pendingCrop = cropRect(cropFrom, pointFor(event), view);
      return;
    }
    if (!live) return;
    live = extendShape(live, pointFor(event));
  }

  function onPointerUp(event: PointerEvent) {
    if (!live && !cropFrom) return;
    // `pointercancel` llega con la captura ya soltada por el navegador, y
    // liberarla dos veces lanza. Preguntar es más barato que un try/catch.
    if (canvas?.hasPointerCapture(event.pointerId)) {
      canvas.releasePointerCapture(event.pointerId);
    }
    if (cropFrom) {
      const next = cropRect(cropFrom, pointFor(event), view);
      cropFrom = null;
      pendingCrop = null;
      if (next) crop = next;
      return;
    }
    if (!live) return;
    doc = commit(doc, live);
    live = null;
  }

  // --- Texto ---

  function startText(at: Point) {
    editing = { at, text: "" };
    // El cuadro todavía no existe: enfocarlo en el mismo tick no haría nada.
    requestAnimationFrame(() => textEl?.focus());
  }

  function commitText() {
    const draft = editing;
    editing = null;
    if (!draft) return;
    // `commit` descarta lo vacío, así que colocar y arrepentirse no deja nada.
    doc = commit(doc, {
      kind: "text",
      color,
      width,
      at: draft.at,
      text: draft.text,
      size: fontSize,
    });
  }

  function onTextInput(event: Event) {
    if (!editing) return;
    const el = event.currentTarget as HTMLElement;
    // `innerText` y no `textContent`: el primero devuelve los saltos de línea
    // como `\n`, que es lo que después dibuja el lienzo.
    editing = { ...editing, text: el.innerText };
  }

  function onTextKey(event: KeyboardEvent) {
    // El cuadro se queda con sus teclas: si subieran a la ventana, escribir
    // «1» cambiaría de herramienta y Escape cerraría el editor entero.
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      editing = null;
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      commitText();
    }
  }

  /**
   * Dónde y con qué letra se ve el cuadro de edición.
   *
   * Se calcula contra el rect del lienzo —igual que `pointFor`, y por el mismo
   * motivo— para que lo que se escribe caiga exactamente donde va a quedar
   * dibujado: mismo cuerpo, misma familia, mismo color.
   */
  const textStyle = $derived.by(() => {
    if (!editing || !canvas) return "";
    const rect = canvas.getBoundingClientRect();
    const scale = view.w > 0 ? rect.width / view.w : 1;
    const left = rect.left + (editing.at.x - view.x) * scale;
    const top = rect.top + (editing.at.y - view.y) * scale;
    return [
      `left: ${left}px`,
      `top: ${top}px`,
      `font: ${fontFor(Math.max(8, fontSize * scale))}`,
      `line-height: ${LINE_HEIGHT}`,
      `color: ${color}`,
      `--halo: ${haloFor(color)}`,
    ].join("; ");
  });

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
      error = t("page.annotate.canvasFail");
      return;
    }
    busy = true;
    try {
      await copyAnnotation(png);
      finish(t("page.annotate.copiedClip"));
    } catch (err) {
      error = String(err);
      busy = false;
    }
  }

  async function save() {
    if (!ready || busy) return;
    const png = exportPng();
    if (!png) {
      error = t("page.annotate.canvasFail");
      return;
    }
    busy = true;
    try {
      await saveAnnotation(png);
      finish(t("page.annotate.savedNew"));
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

  /**
   * ¿El evento salió de un botón?
   *
   * `Element` y no `HTMLElement`: el ícono de cada herramienta es un `<svg>`, y
   * un SVGElement **no** es un HTMLElement. Con el chequeo estrecho, hacer clic
   * justo sobre el dibujo del ícono —y no sobre el aire del botón— decía «esto
   * no es un botón», la barra se quedaba con el puntero y el clic no llegaba
   * nunca a cambiar de herramienta. De ahí que funcionara «a veces»: dependía
   * de un par de píxeles.
   */
  function isButton(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest("button") !== null;
  }

  function onKeydown(event: KeyboardEvent) {
    // Escribiendo, el teclado es del cuadro de texto. Sus propias teclas las
    // ataja `onTextKey`, que corta la propagación antes de llegar acá.
    if (editing) return;
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

<div class="editor" class:is-board={mode === "board"}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="bar"
    data-tauri-drag-region={mode === "panel" ? "" : undefined}
    style={mode === "board" ? boardBarStyle : undefined}
    onpointerdown={onBarDown}
  >
    <div class="group" role="radiogroup" aria-label={t("page.annotate.tools")}>
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

    <div class="group" role="radiogroup" aria-label={t("page.annotate.color")}>
      {#each COLORS as swatch (swatch)}
        <button
          type="button"
          class="swatch"
          class:is-on={color === swatch}
          style="--swatch: {swatch}"
          role="radio"
          aria-checked={color === swatch}
          aria-label={t("page.annotate.colorSwatch", { swatch })}
          onclick={() => (color = swatch)}
        ></button>
      {/each}
    </div>

    <div class="group" role="radiogroup" aria-label={t("page.annotate.width")}>
      {#each WIDTH_LEVELS as value (value)}
        <button
          type="button"
          class="width"
          class:is-on={level === value}
          role="radio"
          aria-checked={level === value}
          aria-label={t("page.annotate.widthValue", { value })}
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
        title={t("page.annotate.undoTitle")}
        aria-label={t("page.annotate.undo")}
        onclick={() => (doc = undo(doc))}
      >
        <Icon icon={Undo2} size={15} />
      </button>
      <button
        type="button"
        class="tool"
        disabled={!canRedo}
        title={t("page.annotate.redoTitle")}
        aria-label={t("page.annotate.redo")}
        onclick={() => (doc = redo(doc))}
      >
        <Icon icon={Redo2} size={15} />
      </button>
      <!-- Solo con un recorte puesto: es la única forma de volver a la imagen
           entera, y un botón muerto al lado de deshacer confundiría. -->
      {#if crop}
        <button
          type="button"
          class="tool is-reset"
          title={t("page.annotate.cropResetTitle")}
          aria-label={t("page.annotate.cropReset")}
          onclick={() => (crop = null)}
        >
          <Icon icon={Crop} size={15} />
        </button>
      {/if}
    </div>

    <div class="spacer" data-tauri-drag-region></div>

    <div class="group is-actions">
      <button
        type="button"
        class="action is-primary"
        disabled={!ready || busy}
        title={t("page.annotate.copyTitle")}
        onclick={() => void copy()}
      >
        <Icon icon={Copy} size={14} />
        <span>{t("page.annotate.copy")}</span>
      </button>
      <button
        type="button"
        class="action"
        disabled={!ready || busy}
        title={t("page.annotate.saveTitle")}
        onclick={() => void save()}
      >
        <Icon icon={Save} size={14} />
        <span>{t("page.annotate.save")}</span>
      </button>
      <button
        type="button"
        class="action"
        class:is-danger={confirmDiscard}
        title={t("page.annotate.closeTitle")}
        aria-label={confirmDiscard ? t("page.annotate.discard") : t("page.common.close")}
        onclick={requestClose}
      >
        {#if confirmDiscard}
          <span>{t("page.annotate.discardQ")}</span>
        {:else}
          <Icon icon={X} size={14} />
        {/if}
      </button>
    </div>
  </div>

  <div class="stage">
    <canvas
      bind:this={canvas}
      width={view.w || 1}
      height={view.h || 1}
      class="canvas"
      class:is-ready={ready}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
    ></canvas>
  </div>

  <!--
    El cuadro de texto vive sobre el lienzo, no dentro de él.
    `contenteditable` y no un `<textarea>`: crece solo a lo ancho y a lo alto
    con lo que se escribe, que es lo que hace que el cuadro y lo que se va a
    dibujar ocupen el mismo sitio. `plaintext-only` deja pegar sin formato.
  -->
  {#if editing}
    <div
      bind:this={textEl}
      class="text-input"
      style={textStyle}
      contenteditable="plaintext-only"
      role="textbox"
      tabindex="0"
      aria-label={t("page.annotate.text")}
      oninput={onTextInput}
      onkeydown={onTextKey}
      onblur={commitText}
    ></div>
  {/if}

  <p
    class="status"
    class:is-note={Boolean(note)}
    class:is-error={Boolean(error)}
    style={mode === "board" ? boardStatusStyle : undefined}
    data-tauri-drag-region={mode === "panel" ? "" : undefined}
  >
    <!-- Sin lienzo listo NO se muestra la ayuda de dibujo: decir «arrastrá para
         dibujar» sobre un editor que todavía no acepta el puntero es lo que
         hizo que un fallo de carga se leyera como «no anda el dibujo». -->
    {error ??
      note ??
      (ready
        ? t("page.annotate.help")
        : t("page.annotate.loading"))}
  </p>

  <!--
    Asa de redimensionado propia. La ventana no tiene decoraciones y el borde
    nativo queda debajo del webview: sin esto, agrandar el editor no es posible
    desde el contenido.
  -->
  {#if mode === "panel"}
    <button
      type="button"
      class="grip"
      aria-label={t("page.annotate.resize")}
      onpointerdown={(event) => {
        event.preventDefault();
        void startResizeDragging("SouthEast").catch(() => {});
      }}
    ></button>
  {/if}
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
    isolation: isolate;
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

  /*
   * El cuadro de texto, encima de todo y anclado al viewport.
   *
   * `fixed` porque su posición sale del rect del lienzo, que ya está en
   * coordenadas del viewport: cualquier otro contenedor obligaría a restarle su
   * propio origen. `pre` para que los saltos de línea del cuadro sean los
   * mismos que dibuja el lienzo, y el halo del `text-shadow` es el gemelo del
   * contorno que `annotateDraw` le pinta al confirmar.
   */
  .text-input {
    position: fixed;
    z-index: 3;
    min-width: 1ch;
    padding: 0;
    border: 0;
    margin: 0;
    background: transparent;
    caret-color: currentColor;
    outline: none;
    text-shadow:
      0 0 2px var(--halo),
      0 0 2px var(--halo);
    white-space: pre;
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

  /*
   * Pizarra: la ventana ES la pantalla.
   *
   * Se va todo lo que hace de «panel» —relleno, esquinas, sombra, borde— para
   * que el congelado quede pegado píxel a píxel con lo que había debajo. Si el
   * lienzo se corriera aunque sea un poco, la marca dejaría de señalar lo que
   * señala, que es lo único que la pizarra tiene que hacer bien.
   */
  .editor.is-board {
    padding: 0;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    gap: 0;
    outline: none;
  }

  .editor.is-board .stage {
    border-radius: 0;
    background: transparent;
  }

  .editor.is-board .canvas {
    width: 100%;
    height: 100%;
    border-radius: 0;
  }

  /*
   * La barra flota sobre la pantalla, anclada al monitor donde está el cursor.
   *
   * `left` / `top` los escribe el componente en % de la imagen —el centro del
   * escritorio virtual cae en la costura entre dos pantallas— y el `transform`
   * suma el desplazamiento del arrastre.
   */
  .editor.is-board .bar {
    position: absolute;
    z-index: 2;
    padding: 8px 10px;
    border-radius: var(--radius-md);
    background: var(--surface-2);
    box-shadow: var(--shadow-float);
    cursor: grab;
    transform: translate(
      calc(-50% + var(--shift-x, 0px)),
      calc(14px + var(--shift-y, 0px))
    );
    outline: 1px solid var(--line);
    outline-offset: -1px;
  }

  .editor.is-board .bar:active {
    cursor: grabbing;
  }

  .editor.is-board .status {
    position: absolute;
    z-index: 2;
    padding: 5px 10px;
    border-radius: var(--radius-pill);
    background: var(--surface-2);
    transform: translate(-50%, calc(-100% - 14px));
    outline: 1px solid var(--line);
    outline-offset: -1px;
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
