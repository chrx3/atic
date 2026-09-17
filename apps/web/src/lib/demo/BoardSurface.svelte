<script lang="ts">
  /**
   * La pizarra: congela la pantalla y se dibuja encima.
   *
   * Mismo motor que la app (`AnnotateSurface` + `annotateModel`): siete
   * herramientas con teclas 1-7, seis colores, tres grosores, deshacer/rehacer,
   * Enter copia, Ctrl+Enter guarda y Esc confirma si hay trazos.
   *
   * Sin imagen congela un escritorio dibujado (`captureArt.ts`); con imagen
   * congela SOBRE esa captura — es lo que abre "Dibujar" desde el shelf.
   */
  import { onMount } from "svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import {
    Circle,
    Copy,
    Crop,
    Download,
    Highlighter,
    MoveUpRight,
    Pencil,
    Redo2,
    Square,
    Type,
    Undo2,
    X,
  } from "$lib/atic/icons";
  import { canvasToBlob, drawWallpaper, drawWindow } from "./captureArt";
  import { demo } from "./state.svelte";

  let { image = null, onClose }: { image?: string | null; onClose?: () => void } = $props();

  /** Las siete reales, con sus teclas (`AnnotateSurface.svelte`). */
  const TOOLS = [
    { id: "pen", icon: Pencil, label: "Lápiz", key: "1" },
    { id: "arrow", icon: MoveUpRight, label: "Flecha", key: "2" },
    { id: "ellipse", icon: Circle, label: "Círculo", key: "3" },
    { id: "rect", icon: Square, label: "Rectángulo", key: "4" },
    { id: "highlight", icon: Highlighter, label: "Resaltador", key: "5" },
    { id: "text", icon: Type, label: "Texto", key: "6" },
    { id: "crop", icon: Crop, label: "Recortar", key: "7" },
  ] as const;

  type Tool = (typeof TOOLS)[number]["id"];

  /** Los seis de la app: el rojo primero porque es el que más se usa. */
  const COLORS = ["#ff3b30", "#ffcc00", "#34c759", "#0a84ff", "#ffffff", "#1c1c1e"];

  /** Los tres grosores (`WIDTH_LEVELS`). El valor real escala con la imagen. */
  const LEVELS = [1, 2, 3] as const;
  type Level = (typeof LEVELS)[number];
  const TEXT_SIZES: Record<Level, number> = { 1: 18, 2: 26, 3: 38 };

  type Pt = { x: number; y: number };
  type Stroke = {
    kind: Exclude<Tool, "crop">;
    color: string;
    level: Level;
    points: Pt[];
    text?: string;
  };

  let host = $state<HTMLDivElement | null>(null);
  let ink = $state<HTMLCanvasElement | null>(null);
  let tool = $state<Tool>("arrow");
  let color = $state<string>(COLORS[0]);
  let level = $state<Level>(2);

  let strokes: Stroke[] = [];
  let undone: Stroke[] = [];
  let drawing: Stroke | { kind: "crop"; color: string; level: Level; points: Pt[] } | null = null;
  /** La región elegida con Recortar, en px CSS del tablero. */
  let crop = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  /** Marquesina del recorte mientras se arrastra. */
  let marquee = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  /** Editor de texto flotante. */
  let editing = $state<{ at: Pt; value: string } | null>(null);
  let editInput = $state<HTMLInputElement | null>(null);
  /** Esc con trazos: pide confirmar una vez, como la app. */
  let confirming = $state(false);
  let raf = 0;

  /** La escena congelada, fuera de pantalla: fondo de todo. */
  let scene = typeof document !== "undefined" ? document.createElement("canvas") : null;
  let photo: HTMLImageElement | null = null;

  const dirty = $derived(strokes.length > 0 || crop !== null);

  function boardSize(): { w: number; h: number } {
    return { w: host?.clientWidth ?? 0, h: host?.clientHeight ?? 0 };
  }

  function dpr(): number {
    return Math.min(window.devicePixelRatio || 1, 2);
  }

  /** Grosor real: el nivel por dos, a la escala de la imagen. */
  function strokeWidth(lv: Level, imageWidth: number): number {
    const scale = Math.max(1, imageWidth / 1280);
    return Math.max(1, Math.round(lv * 2 * scale));
  }

  function drawScene() {
    if (!scene || !host) return;
    const { w, h } = boardSize();
    const d = dpr();
    scene.width = Math.round(w * d);
    scene.height = Math.round(h * d);
    const ctx = scene.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(d, 0, 0, d, 0, 0);
    if (photo) {
      // La captura cubre el tablero.
      const s = Math.max(w / photo.naturalWidth, h / photo.naturalHeight);
      const dw = photo.naturalWidth * s;
      const dh = photo.naturalHeight * s;
      ctx.drawImage(photo, (w - dw) / 2, (h - dh) / 2, dw, dh);
      return;
    }
    drawWallpaper(ctx, w, h);
    const w1 = drawWindow(ctx, 60, 70, 380, 260, "editor — atic");
    ctx.fillStyle = "rgba(240, 240, 234, 0.1)";
    let y = w1.y + 22;
    for (let i = 0; i < 9; i++) {
      ctx.fillRect(w1.x + 18, y, w1.w - 60 - (i % 3) * 40, 7);
      y += 15;
    }
    const w2 = drawWindow(ctx, 470, 250, 330, 210, "resumen.pdf");
    ctx.fillStyle = "rgba(240, 240, 234, 0.1)";
    y = w2.y + 24;
    for (let i = 0; i < 7; i++) {
      ctx.fillRect(w2.x + 18, y, w2.w - 44, 8);
      y += 17;
    }
  }

  function drawBase() {
    const canvas = host?.querySelector<HTMLCanvasElement>(".board-base");
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || !scene) return;
    const { w, h } = boardSize();
    const d = dpr();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (crop) {
      ctx.drawImage(
        scene,
        crop.x * d, crop.y * d, crop.w * d, crop.h * d,
        0, 0, canvas.width, canvas.height,
      );
    } else {
      ctx.drawImage(scene, 0, 0);
    }
    void w;
    void h;
  }

  function drawStroke(ctx: CanvasRenderingContext2D, stroke: Stroke, imageWidth: number) {
    const [first, ...rest] = stroke.points;
    if (!first) return;
    ctx.save();
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = stroke.color;
    ctx.fillStyle = stroke.color;

    if (stroke.kind === "highlight") {
      ctx.globalAlpha = 0.32;
      ctx.lineWidth = strokeWidth(stroke.level, imageWidth) * 5;
    } else if (stroke.kind === "text") {
      ctx.globalAlpha = 1;
      ctx.font = `500 ${TEXT_SIZES[stroke.level]}px -apple-system, system-ui, sans-serif`;
      ctx.textBaseline = "top";
      for (const [i, line] of (stroke.text ?? "").split("\n").entries()) {
        ctx.fillText(line, first.x, first.y + i * TEXT_SIZES[stroke.level] * 1.25);
      }
      ctx.restore();
      return;
    } else {
      ctx.lineWidth = strokeWidth(stroke.level, imageWidth);
    }

    const last = stroke.points[stroke.points.length - 1];
    if (stroke.kind === "arrow") {
      ctx.beginPath();
      ctx.moveTo(first.x, first.y);
      ctx.lineTo(last.x, last.y);
      ctx.stroke();
      const angle = Math.atan2(last.y - first.y, last.x - first.x);
      const head = 14 + ctx.lineWidth;
      ctx.beginPath();
      ctx.moveTo(last.x, last.y);
      ctx.lineTo(
        last.x - head * Math.cos(angle - Math.PI / 7),
        last.y - head * Math.sin(angle - Math.PI / 7),
      );
      ctx.moveTo(last.x, last.y);
      ctx.lineTo(
        last.x - head * Math.cos(angle + Math.PI / 7),
        last.y - head * Math.sin(angle + Math.PI / 7),
      );
      ctx.stroke();
    } else if (stroke.kind === "ellipse") {
      const rx = Math.abs(last.x - first.x) / 2;
      const ry = Math.abs(last.y - first.y) / 2;
      if (rx > 1 && ry > 1) {
        ctx.beginPath();
        ctx.ellipse((first.x + last.x) / 2, (first.y + last.y) / 2, rx, ry, 0, 0, Math.PI * 2);
        ctx.stroke();
      }
    } else if (stroke.kind === "rect") {
      const rw = last.x - first.x;
      const rh = last.y - first.y;
      if (Math.abs(rw) > 1 && Math.abs(rh) > 1) ctx.strokeRect(first.x, first.y, rw, rh);
    } else {
      ctx.beginPath();
      ctx.moveTo(first.x, first.y);
      for (const point of rest) ctx.lineTo(point.x, point.y);
      if (rest.length === 0) ctx.lineTo(first.x + 0.1, first.y + 0.1);
      ctx.stroke();
    }
    ctx.restore();
  }

  function redraw() {
    const ctx = ink?.getContext("2d");
    if (!ink || !ctx) return;
    const { w } = boardSize();
    ctx.setTransform(dpr(), 0, 0, dpr(), 0, 0);
    ctx.clearRect(0, 0, ink.width, ink.height);
    for (const stroke of strokes) drawStroke(ctx, stroke, w);
    if (drawing && drawing.kind !== "crop") drawStroke(ctx, drawing, w);
    if (marquee) {
      ctx.save();
      ctx.setLineDash([6, 4]);
      ctx.lineWidth = 1.5;
      ctx.strokeStyle = "#fff";
      ctx.strokeRect(marquee.x, marquee.y, marquee.w, marquee.h);
      ctx.restore();
    }
  }

  function scheduleRedraw() {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(redraw);
  }

  function loadPhoto() {
    photo = null;
    if (!image) {
      drawScene();
      drawBase();
      return;
    }
    const img = new Image();
    img.onload = () => {
      photo = img;
      drawScene();
      drawBase();
    };
    img.src = image;
  }

  function resize() {
    const el = host;
    if (!el) return;
    const d = dpr();
    const w = el.clientWidth;
    const h = el.clientHeight;
    for (const canvas of el.querySelectorAll("canvas")) {
      canvas.width = Math.round(w * d);
      canvas.height = Math.round(h * d);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
    }
    drawScene();
    drawBase();
    redraw();
  }

  onMount(() => {
    const observer = new ResizeObserver(resize);
    if (host) observer.observe(host);
    loadPhoto();
    resize();
    return () => observer.disconnect();
  });

  function point(event: PointerEvent): Pt {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: event.clientX - box.left, y: event.clientY - box.top };
  }

  function dragDist(points: Pt[]): number {
    if (points.length < 2) return 0;
    const a = points[0];
    const b = points[points.length - 1];
    return Math.hypot(b.x - a.x, b.y - a.y);
  }

  function down(event: PointerEvent) {
    if (editing) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    confirming = false;
    const at = point(event);
    if (tool === "text") {
      editing = { at, value: "" };
      queueMicrotask(() => editInput?.focus());
      return;
    }
    drawing = { kind: tool, color, level, points: [at] };
    if (tool === "crop") marquee = { x: at.x, y: at.y, w: 0, h: 0 };
    scheduleRedraw();
  }

  function move(event: PointerEvent) {
    if (!drawing) return;
    const at = point(event);
    drawing.points.push(at);
    if (tool === "crop") {
      const [first] = drawing.points;
      marquee = {
        x: Math.min(first.x, at.x),
        y: Math.min(first.y, at.y),
        w: Math.abs(at.x - first.x),
        h: Math.abs(at.y - first.y),
      };
    }
    scheduleRedraw();
  }

  function up() {
    if (!drawing) return;
    const live = drawing;
    drawing = null;
    if (live.kind === "crop") {
      if (marquee && marquee.w > 8 && marquee.h > 8) {
        crop = { ...marquee };
        strokes = [];
        undone = [];
        drawBase();
        demo.toast("Recorte aplicado", "ok");
      }
      marquee = null;
    } else if (live.kind === "pen" || live.kind === "highlight" || dragDist(live.points) >= 4) {
      // Menos de 4 px de arrastre y la forma no llegó a existir.
      strokes = [...strokes, live];
      undone = [];
    }
    scheduleRedraw();
  }

  function commitText() {
    if (!editing) return;
    const text = editing.value.trim();
    if (text) {
      strokes = [
        ...strokes,
        { kind: "text", color, level, points: [editing.at], text: editing.value },
      ];
      undone = [];
      scheduleRedraw();
    }
    editing = null;
  }

  function undo() {
    const last = strokes[strokes.length - 1];
    if (!last) return;
    strokes = strokes.slice(0, -1);
    undone = [...undone, last];
    redraw();
  }

  function redo() {
    const next = undone[undone.length - 1];
    if (!next) return;
    undone = undone.slice(0, -1);
    strokes = [...strokes, next];
    redraw();
  }

  function resetCrop() {
    crop = null;
    strokes = [];
    undone = [];
    drawBase();
    redraw();
  }

  function composite(): HTMLCanvasElement | null {
    const el = host;
    if (!el) return null;
    const canvas = document.createElement("canvas");
    canvas.width = el.clientWidth * 2;
    canvas.height = el.clientHeight * 2;
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.scale(2, 2);
    const base = el.querySelector("canvas");
    if (base) ctx.drawImage(base, 0, 0, el.clientWidth, el.clientHeight);
    if (ink) ctx.drawImage(ink, 0, 0, el.clientWidth, el.clientHeight);
    return canvas;
  }

  async function copy() {
    const canvas = composite();
    if (!canvas) return;
    try {
      const blob = await canvasToBlob(canvas);
      if (blob) {
        await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
        demo.toast("Copiada al portapapeles", "ok");
        return;
      }
    } catch {
      /* Sin permiso de imagen: cae al aviso. */
    }
    demo.toast("El navegador no dejó copiar la imagen; usa Guardar", "info");
  }

  function save() {
    const canvas = composite();
    if (!canvas) return;
    const link = document.createElement("a");
    link.href = canvas.toDataURL("image/png");
    link.download = "atic-pizarra.png";
    link.click();
    demo.toast("Pizarra guardada");
  }

  function askClose() {
    if (dirty && !confirming) {
      confirming = true;
      return;
    }
    onClose?.();
  }

  function onKey(event: KeyboardEvent) {
    if (editing) return;
    if (event.key === "Enter" && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      void copy();
      return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      save();
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "z" && !event.shiftKey) {
      event.preventDefault();
      undo();
      return;
    }
    if (
      (event.ctrlKey || event.metaKey) &&
      (event.key.toLowerCase() === "y" || (event.key.toLowerCase() === "z" && event.shiftKey))
    ) {
      event.preventDefault();
      redo();
      return;
    }
    const found = TOOLS.find((item) => item.key === event.key);
    if (found && !event.ctrlKey && !event.metaKey && !event.altKey) {
      tool = found.id;
    }
  }
</script>

<div
  class="board"
  bind:this={host}
  role="dialog"
  aria-label="Pizarra"
  tabindex="-1"
  onkeydown={onKey}
>
  <canvas class="board-base" aria-hidden="true"></canvas>
  <canvas
    class="board-ink"
    bind:this={ink}
    aria-label="Área de dibujo"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
  ></canvas>

  {#if editing}
    <input
      class="text-edit"
      bind:this={editInput}
      bind:value={editing.value}
      style="left: {editing.at.x}px; top: {editing.at.y}px; color: {color}; font-size: {TEXT_SIZES[level]}px"
      aria-label="Texto de la anotación"
      onkeydown={(event) => {
        if (event.key === "Enter") commitText();
        else if (event.key === "Escape") editing = null;
        event.stopPropagation();
      }}
      onblur={commitText}
    />
  {/if}

  <span class="freeze-tag">Pantalla congelada — dibuja encima</span>

  {#if crop}
    <button type="button" class="crop-reset" onclick={resetCrop} title="Volver a la imagen entera">
      Quitar el recorte
    </button>
  {/if}

  {#if confirming}
    <div class="confirm" role="alertdialog" aria-label="Descartar el dibujo">
      <span>¿Descartar?</span>
      <button type="button" class="confirm-yes" onclick={() => onClose?.()}>Descartar</button>
      <button type="button" class="confirm-no" onclick={() => (confirming = false)}>Seguir</button>
    </div>
  {/if}

  <button type="button" class="close" aria-label="Cerrar la pizarra" title="Cerrar sin guardar (Esc)" onclick={askClose}>
    <Icon icon={X} size={16} />
  </button>

  <div class="bar" role="toolbar" aria-label="Herramienta">
    <div class="tools">
      {#each TOOLS as item (item.id)}
        <button
          type="button"
          class="tool"
          class:is-on={tool === item.id}
          aria-label="{item.label} ({item.key})"
          title="{item.label} ({item.key})"
          onclick={() => (tool = item.id)}
        >
          <Icon icon={item.icon} size={16} />
        </button>
      {/each}
    </div>

    <span class="bar-sep"></span>

    <div class="colors" role="group" aria-label="Color">
      {#each COLORS as value (value)}
        <button
          type="button"
          class="color"
          class:is-on={color === value}
          style="--c: {value}"
          aria-label="Color {value}"
          title={value}
          onclick={() => (color = value)}
        ></button>
      {/each}
    </div>

    <span class="bar-sep"></span>

    <div class="widths" role="group" aria-label="Grosor">
      {#each LEVELS as lv (lv)}
        <button
          type="button"
          class="width"
          class:is-on={level === lv}
          aria-label="Grosor {lv}"
          title="Grosor {lv}"
          onclick={() => (level = lv)}
        >
          <i style="height: {lv * 2}px"></i>
        </button>
      {/each}
    </div>

    <span class="bar-sep"></span>

    <button type="button" class="tool" aria-label="Deshacer (Ctrl+Z)" title="Deshacer (Ctrl+Z)" onclick={undo}>
      <Icon icon={Undo2} size={16} />
    </button>
    <button type="button" class="tool" aria-label="Rehacer (Ctrl+Shift+Z)" title="Rehacer (Ctrl+Shift+Z)" onclick={redo}>
      <Icon icon={Redo2} size={16} />
    </button>
    <button type="button" class="tool" aria-label="Copiar al portapapeles (Enter)" title="Copiar al portapapeles (Enter)" onclick={copy}>
      <Icon icon={Copy} size={16} />
    </button>
    <button type="button" class="tool" aria-label="Guardar como captura nueva (Ctrl+Enter)" title="Guardar como captura nueva (Ctrl+Enter)" onclick={save}>
      <Icon icon={Download} size={16} />
    </button>
  </div>

  <p class="freeze-hint">Arrastra para dibujar · Enter copia · Ctrl+Enter guarda · Esc cierra</p>
</div>

<style>
  .board {
    position: absolute;
    inset: 0;
    z-index: 10;
    cursor: crosshair;
    animation: board-in var(--duration-medium) var(--ease-smooth-out);
    outline: none;
  }

  @keyframes board-in {
    from {
      opacity: 0;
      filter: blur(6px);
    }
  }

  canvas {
    position: absolute;
    inset: 0;
    display: block;
  }

  .board-ink {
    touch-action: none;
  }

  .text-edit {
    position: absolute;
    z-index: 2;
    min-width: 120px;
    border: 1px dashed rgb(255 255 255 / 60%);
    border-radius: 4px;
    padding: 2px 6px;
    background: rgb(10 10 14 / 55%);
    font-family: inherit;
    font-weight: 500;
    outline: none;
    transform: translateY(-100%);
  }

  .freeze-tag {
    position: absolute;
    top: 14px;
    left: 14px;
    border-radius: var(--radius-pill);
    padding: 5px 11px;
    background: rgb(10 10 14 / 82%);
    border: 1px solid rgb(255 255 255 / 16%);
    color: #fff;
    font-size: var(--text-micro);
    font-weight: var(--font-weight-semibold);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  .crop-reset {
    position: absolute;
    top: 14px;
    left: 50%;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: var(--radius-pill);
    padding: 5px 11px;
    background: rgb(18 18 22 / 88%);
    color: #e6e6e0;
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
    transform: translateX(-50%);
  }

  .crop-reset:hover {
    background: rgb(18 18 22 / 100%);
  }

  .confirm {
    position: absolute;
    top: 14px;
    right: 54px;
    display: flex;
    gap: 6px;
    align-items: center;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    background: rgb(18 18 22 / 92%);
    color: #fff;
    font-size: var(--text-xs);
  }

  .confirm-yes,
  .confirm-no {
    border: 1px solid rgb(255 255 255 / 22%);
    border-radius: var(--radius-xs);
    padding: 3px 8px;
    background: transparent;
    color: #fff;
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
  }

  .confirm-yes {
    border-color: var(--danger);
    color: #ff9a8a;
  }

  .close {
    position: absolute;
    top: 12px;
    right: 12px;
    display: grid;
    width: 30px;
    height: 30px;
    place-items: center;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: var(--radius-sm);
    background: rgb(18 18 22 / 86%);
    color: #e6e6e0;
    cursor: pointer;
  }

  .close:hover {
    background: rgb(18 18 22 / 100%);
  }

  .bar {
    position: absolute;
    bottom: 14px;
    left: 50%;
    display: flex;
    max-width: calc(100% - 24px);
    flex-wrap: wrap;
    justify-content: center;
    gap: 4px;
    align-items: center;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: var(--radius-md);
    padding: 6px;
    background: rgb(18 18 22 / 88%);
    box-shadow: 0 14px 40px rgb(0 0 0 / 45%);
    transform: translateX(-50%);
  }

  .tools,
  .colors,
  .widths {
    display: flex;
    gap: 2px;
    align-items: center;
  }

  .tool {
    display: grid;
    width: 32px;
    height: 32px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: #b9b9b2;
    cursor: pointer;
  }

  .tool:hover {
    background: rgb(255 255 255 / 10%);
    color: #fff;
  }

  .tool.is-on {
    background: rgb(255 255 255 / 18%);
    color: #fff;
  }

  .bar-sep {
    width: 1px;
    height: 22px;
    margin: 0 3px;
    background: rgb(255 255 255 / 18%);
  }

  .color {
    width: 22px;
    height: 22px;
    margin: 5px 2px;
    border: 2px solid transparent;
    border-radius: 50%;
    background: var(--c);
    cursor: pointer;
  }

  .color.is-on {
    border-color: rgb(255 255 255 / 85%);
  }

  .width {
    display: grid;
    width: 28px;
    height: 32px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    cursor: pointer;
  }

  .width i {
    display: block;
    width: 16px;
    border-radius: 999px;
    background: #b9b9b2;
  }

  .width.is-on {
    background: rgb(255 255 255 / 18%);
  }

  .width.is-on i {
    background: #fff;
  }

  .freeze-hint {
    position: absolute;
    top: 18px;
    left: 50%;
    margin: 0;
    color: rgb(255 255 255 / 66%);
    font-size: var(--text-xs);
    text-shadow: 0 1px 6px rgb(0 0 0 / 70%);
    transform: translateX(-50%);
    white-space: nowrap;
    pointer-events: none;
  }

  @media (max-width: 640px) {
    .freeze-hint {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .board {
      animation: none;
    }
  }
</style>
