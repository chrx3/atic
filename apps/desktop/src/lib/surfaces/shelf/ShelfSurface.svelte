<script lang="ts">
  /**
   * El estante: la tarjeta que aparece abajo tras una captura.
   *
   * Es efímera por diseño. Se va sola a los seis segundos porque su trabajo es
   * ofrecer lo que uno hace con una captura recién tomada —arrastrarla,
   * abrirla, sacarle el texto— y pasado ese momento estorba. Todo lo demás vive
   * en la herramienta de capturas de la ventana principal.
   *
   * La cuenta atrás se reintenta en vez de cumplirse si hay algo en curso o el
   * puntero está encima: la tarjeta no puede desaparecer justo cuando la mano
   * va a agarrarla.
   *
   * Abrir/cerrar usa el mismo patrón interruptible que `.float-emerge`
   * (`alive` + `shown`): la ventana nativa solo se oculta al terminar el
   * repliegue, si no el outro nunca se ve.
   */
  import type { CaptureItem } from "$core/types";
  import { t } from "$domain/i18n.svelte";
  import { MOTION, ms, wait } from "$lib/motion";
  import {
    activateCapture,
    captureSrc,
    ocrCaptureAndCopy,
    onScreenshotCreated,
  } from "$ipc/captures";
  import { openAnnotator } from "$ipc/annotate";
  import { startFileDrag, tryClipboardDropOnAgents } from "$ipc/clipboard";
  import { getConfig, openDataDir } from "$ipc/config";
  import { setOverlayItemDrag, overlayCursorOverHit } from "$ipc/overlay";
  import {
    coverShelfMonitor,
    hideWindow,
    restoreShelfBounds,
    type PhysicalBounds,
    type ShelfCover,
  } from "$ipc/windows";
  import { tick } from "svelte";

  const DISMISS_MS = 6000;

  /** Más que esto en píxeles y el clic pasa a ser un arrastre. */
  const DRAG_THRESHOLD = 5;

  /** Recorrido en el eje de descarte para soltar el toast. */
  const DISCARD_PX = 72;

  /** Padding de `.shelf`; el ghost se ancla al slot expandido con este inset. */
  const SHELF_PAD = 10;

  /**
   * Tirón hacia adentro o arriba: pasa al arrastre nativo (sacar a otra app).
   * Alto a propósito: el ghost tiene que poder salir del toast antes de que
   * OLE se trague el preview.
   */
  const FILE_DRAG_PX = 220;

  /** Desde acá el gesto se traba en un solo eje. */
  const LOCK_PX = 22;

  type DragLock = "none" | "x" | "y";

  let current = $state<CaptureItem | null>(null);
  let src = $state("");
  let busy = $state(false);
  let ocrBusy = $state(false);

  /** Lo que dijo el OCR. Antes iba solo a la consola y nadie lo veía. */
  let note = $state<string | null>(null);

  /** Sigue montada (aunque se esté replegando). */
  let alive = $state(false);
  /** Ya emergió: dispara la transición de entrada. */
  let shown = $state(false);

  let hovering = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  let press: { x: number; y: number } | null = null;
  let dragging = false;
  let leftSide = false;
  let restBounds: PhysicalBounds | null = null;
  let shelfSlot = $state<{ left: number; top: number } | null>(null);
  let drag = $state<{
    dx: number;
    dy: number;
    lock: DragLock;
    fling: boolean;
  } | null>(null);
  let expanding = false;
  let fileDragStarted = false;
  let pointerId: number | null = null;
  let thumbEl: HTMLButtonElement | undefined = $state();
  /** El resize del HWND dispara lostcapture / move sin botón: no es un soltar. */
  let holdGesture = false;
  let ignoreLostCapture = false;
  /** Offset del clic dentro del thumb, en CSS: el ghost sigue al cursor. */
  let grab = { x: 0, y: 0 };
  let lastClient = { x: 0, y: 0 };
  let lastScreen = { x: 0, y: 0 };
  let ghost = $state<{ left: number; top: number } | null>(null);

  function clearTimer() {
    if (timer) clearTimeout(timer);
    timer = null;
  }

  function clearHideTimer() {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = null;
  }

  /** Presenta la tarjeta: un frame replegada para que la transición tenga origen. */
  async function present(item: CaptureItem) {
    clearHideTimer();
    drag = null;
    ghost = null;
    fileDragStarted = false;
    holdGesture = false;
    ignoreLostCapture = false;
    endPress();
    await restoreWindow();
    current = item;
    note = null;
    void getConfig()
      .then((cfg) => {
        leftSide = cfg.capture_shelf_side === "left";
      })
      .catch(() => {
        leftSide = false;
      });
    // El sufijo obliga a releer el archivo: dos capturas seguidas pueden
    // compartir ruta y el webview serviría la primera desde su caché.
    src = `${captureSrc(item.path)}?t=${Date.now()}`;
    alive = true;
    shown = false;
    await tick();
    requestAnimationFrame(() => {
      shown = true;
    });
    scheduleDismiss();
  }

  async function restoreWindow() {
    const snap = restBounds;
    restBounds = null;
    shelfSlot = null;
    if (!snap) return;
    try {
      await setOverlayItemDrag(false).catch(() => {});
      await restoreShelfBounds(snap);
    } catch {
      // La ventana ya se ocultó o cambió de tamaño.
    }
  }

  async function expandForDrag() {
    if (expanding || restBounds) return;
    expanding = true;
    holdGesture = true;
    ignoreLostCapture = true;
    try {
      const covered = await coverShelfMonitor();
      if (!covered) return;
      restBounds = covered.rest;
      await tick();
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      const vw = Math.max(1, window.innerWidth);
      const vh = Math.max(1, window.innerHeight);
      const monW = Math.max(1, covered.monW);
      const monH = Math.max(1, covered.monH);
      shelfSlot = {
        left: ((covered.rest.x - covered.monX) / monW) * vw,
        top: ((covered.rest.y - covered.monY) / monH) * vh,
      };
      await tick();
      recapture();
      if (drag && !drag.fling) pinGhostToCursor(covered);
      await setOverlayItemDrag(true).catch(() => {});
    } catch {
      // Sin monitor: el ghost queda recortado al HWND del toast.
    } finally {
      expanding = false;
      recapture();
      window.setTimeout(() => {
        holdGesture = false;
        ignoreLostCapture = false;
      }, 200);
    }
  }

  /** Tras el cover, `clientX` viejo apunta al toast chico: hay que remapear. */
  function pinGhostToCursor(covered: ShelfCover) {
    const dpr = window.devicePixelRatio || 1;
    ghost = {
      left: lastScreen.x - covered.monX / dpr - grab.x,
      top: lastScreen.y - covered.monY / dpr - grab.y,
    };
  }

  function hide() {
    clearTimer();
    shown = false;
    const closeMs = ms(MOTION.floatClose);
    clearHideTimer();
    hideTimer = setTimeout(() => {
      if (shown) return;
      void restoreWindow().finally(() => {
        current = null;
        note = null;
        alive = false;
        drag = null;
        ghost = null;
        void hideWindow();
      });
    }, closeMs);
  }

  function scheduleDismiss() {
    clearTimer();
    timer = setTimeout(() => {
      if (busy || ocrBusy || hovering) {
        scheduleDismiss();
        return;
      }
      hide();
    }, DISMISS_MS);
  }

  $effect(() => {
    const pending = onScreenshotCreated((item) => {
      void present(item);
    });
    return () => {
      void pending.then((off) => off());
      clearTimer();
      clearHideTimer();
      endPress();
      drag = null;
      ghost = null;
    };
  });

  /** El ghost vive fuera de `.shelf` para no recortarse con `filter`/`transform`. */
  function placeGhost(dx: number, dy: number, event?: PointerEvent) {
    const lock = drag?.lock ?? "none";
    const followCursor = Boolean(event) && !drag?.fling && lock === "none";
    if (event) {
      lastClient = { x: event.clientX, y: event.clientY };
    }
    if (followCursor) {
      ghost = {
        left: lastClient.x - grab.x,
        top: lastClient.y - grab.y,
      };
      return;
    }
    const origin = thumbEl?.getBoundingClientRect();
    if (origin && origin.width > 0) {
      ghost = { left: origin.left + dx, top: origin.top + dy };
      return;
    }
    if (shelfSlot) {
      ghost = {
        left: shelfSlot.left + SHELF_PAD + dx,
        top: shelfSlot.top + SHELF_PAD + dy,
      };
    }
  }

  function projectDrag(dx: number, dy: number, lock: DragLock): {
    dx: number;
    dy: number;
    lock: DragLock;
  } {
    const outward = leftSide ? -dx : dx;
    if (lock === "none") {
      if (dy >= LOCK_PX && dy >= outward * 1.15) lock = "y";
      else if (outward >= LOCK_PX && outward >= dy * 1.15) lock = "x";
    }
    if (lock === "y") return { dx: 0, dy: Math.max(0, dy), lock };
    if (lock === "x") {
      return {
        dx: leftSide ? Math.min(0, dx) : Math.max(0, dx),
        dy: 0,
        lock,
      };
    }
    return { dx, dy, lock };
  }

  async function beginFileDrag(item: CaptureItem) {
    if (fileDragStarted) return;
    fileDragStarted = true;
    dragging = false;
    drag = null;
    ghost = null;
    endPress();
    await restoreWindow();
    busy = true;
    try {
      await setOverlayItemDrag(true).catch(() => {});
      await startFileDrag([item.path]).catch(() => {});
      await tryClipboardDropOnAgents(`capture-${item.id}`).catch(() => false);
    } finally {
      await setOverlayItemDrag(false).catch(() => {});
      busy = false;
      hide();
    }
  }

  async function finishDiscard(lock: DragLock) {
    if (!drag) return;
    const extra = 240;
    const dx = lock === "x" ? drag.dx + (leftSide ? -extra : extra) : 0;
    const dy = lock === "y" ? drag.dy + extra : drag.dy;
    drag = { ...drag, dx, dy, fling: true };
    placeGhost(dx, dy);
    await wait(ms(MOTION.fast));
    hide();
  }

  async function snapBack() {
    if (!drag) {
      await restoreWindow();
      return;
    }
    drag = { ...drag, dx: 0, dy: 0, fling: true };
    placeGhost(0, 0);
    await wait(ms(MOTION.fast));
    drag = null;
    ghost = null;
    await restoreWindow();
    scheduleDismiss();
  }

  async function activate() {
    if (!current) return;
    busy = true;
    try {
      await activateCapture(current.path);
    } finally {
      busy = false;
      hide();
    }
  }

  async function ocr(event: MouseEvent) {
    event.stopPropagation();
    if (!current || ocrBusy) return;
    ocrBusy = true;
    clearTimer();
    try {
      const text = await ocrCaptureAndCopy(current.path);
      note = text.trim()
        ? t("page.captures.ocrCopied")
        : t("page.captures.ocrEmpty");
    } catch {
      note = t("page.captures.ocrFail");
    } finally {
      ocrBusy = false;
      // Deja leer el resultado en vez de cerrar de golpe.
      timer = setTimeout(hide, 2200);
    }
  }

  /** Abre el editor de anotaciones sin depender de la config del clic. */
  async function annotate(event: MouseEvent) {
    event.stopPropagation();
    if (!current || busy) return;
    busy = true;
    clearTimer();
    try {
      await openAnnotator(current.path);
      hide();
    } catch {
      note = t("page.captures.editorFail");
      scheduleDismiss();
    } finally {
      busy = false;
    }
  }

  function openFolder(event: MouseEvent) {
    event.stopPropagation();
    clearTimer();
    void openDataDir("captures").catch(() => {});
  }

  function recapture() {
    if (pointerId === null || !thumbEl || !press) return;
    try {
      thumbEl.setPointerCapture(pointerId);
    } catch {
      // El resize de WebView2 a veces suelta el id; el listener de ventana sigue.
    }
  }

  function bindGesture() {
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("pointercancel", onUp, true);
    window.addEventListener("lostpointercapture", onLostCapture, true);
  }

  function unbindGesture() {
    window.removeEventListener("pointermove", onMove, true);
    window.removeEventListener("pointerup", onUp, true);
    window.removeEventListener("pointercancel", onUp, true);
    window.removeEventListener("lostpointercapture", onLostCapture, true);
  }

  function endPress() {
    unbindGesture();
    if (pointerId !== null && thumbEl) {
      try {
        thumbEl.releasePointerCapture(pointerId);
      } catch {
        // Ya no había capture.
      }
    }
    pointerId = null;
    press = null;
  }

  function onLostCapture() {
    if (holdGesture || ignoreLostCapture || expanding) {
      recapture();
      return;
    }
    if (press) onUp();
  }

  function onMove(event: PointerEvent) {
    if (pointerId !== null && event.pointerId !== pointerId) return;
    if (!press || fileDragStarted) return;
    lastScreen = { x: event.screenX, y: event.screenY };
    // Tras un resize, WebView2 inventa un move sin botón. No es un soltar.
    if (event.buttons === 0) {
      if (holdGesture || expanding) return;
      onUp();
      return;
    }
    const rawX = event.screenX - press.x;
    const rawY = event.screenY - press.y;
    if (!dragging) {
      if (Math.hypot(rawX, rawY) <= DRAG_THRESHOLD) return;
      dragging = true;
      clearTimer();
      const next = projectDrag(rawX, rawY, "none");
      drag = { ...next, fling: false };
      placeGhost(next.dx, next.dy, event);
      void expandForDrag();
      return;
    }
    const next = projectDrag(rawX, rawY, drag?.lock ?? "none");
    drag = { ...next, fling: false };
    placeGhost(next.dx, next.dy, event);

    const item = current;
    if (!item || next.lock !== "none" || holdGesture) return;
    const inward = leftSide ? rawX : -rawX;
    if (rawY < -FILE_DRAG_PX || inward > FILE_DRAG_PX) {
      void maybeStartOle(item);
    }
  }

  async function maybeStartOle(item: CaptureItem) {
    const over = await overlayCursorOverHit("agents").catch(() => false);
    if (over) return;
    void beginFileDrag(item);
  }

  async function dropOnAgents(item: CaptureItem) {
    ghost = null;
    drag = null;
    await restoreWindow();
    try {
      await tryClipboardDropOnAgents(`capture-${item.id}`).catch(() => false);
    } finally {
      hide();
    }
  }

  function onUp() {
    if (holdGesture || expanding) return;
    if (!press && !dragging) {
      endPress();
      return;
    }
    const wasClick = press !== null && !dragging;
    const wasDragging = dragging;
    const state = drag;
    dragging = false;
    endPress();
    if (fileDragStarted) return;
    if (wasClick) {
      void activate();
      return;
    }
    if (!wasDragging) return;
    if (!state) {
      ghost = null;
      void restoreWindow();
      scheduleDismiss();
      return;
    }
    const travel = state.lock === "x" ? Math.abs(state.dx) : state.lock === "y" ? state.dy : 0;
    if (state.lock !== "none" && travel >= DISCARD_PX) {
      void finishDiscard(state.lock);
      return;
    }
    void settleDrag();
  }

  async function settleDrag() {
    const item = current;
    if (item) {
      const over = await overlayCursorOverHit("agents").catch(() => false);
      if (over) {
        await dropOnAgents(item);
        return;
      }
    }
    void snapBack();
  }

  function onDown(event: PointerEvent) {
    if (event.button !== 0 || busy || ocrBusy) return;
    event.preventDefault();
    pointerId = event.pointerId;
    press = { x: event.screenX, y: event.screenY };
    dragging = false;
    fileDragStarted = false;
    drag = null;
    ghost = null;
    lastClient = { x: event.clientX, y: event.clientY };
    lastScreen = { x: event.screenX, y: event.screenY };
    const r = thumbEl?.getBoundingClientRect();
    grab = r
      ? { x: event.clientX - r.left, y: event.clientY - r.top }
      : { x: 0, y: 0 };
    bindGesture();
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      // Sin capture, los listeners de ventana cubren el gesto.
    }
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key !== "Escape" || !drag || fileDragStarted) return;
    event.preventDefault();
    dragging = false;
    endPress();
    void snapBack();
  }}
/>

{#if alive && current}
  <div
    class="shelf"
    class:is-shown={shown}
    class:is-expanded={shelfSlot !== null}
    class:is-dragging={drag !== null}
    style={shelfSlot ? `left:${shelfSlot.left}px;top:${shelfSlot.top}px` : ""}
    onmouseenter={() => (hovering = true)}
    onmouseleave={() => (hovering = false)}
    role="group"
    aria-label={t("shelf.recent")}
  >
    <button
      type="button"
      bind:this={thumbEl}
      class="shelf-thumb"
      class:is-parked={ghost !== null}
      onpointerdown={onDown}
      onlostpointercapture={onLostCapture}
      aria-label={t("shelf.open", { label: current.label || current.id })}
      aria-describedby="shelf-tip"
    >
      <img {src} alt="" draggable="false" class="shelf-thumb-img" />
    </button>

    <div class="shelf-side">
      <div class="shelf-actions">
        <button
          type="button"
          class="shelf-action"
          disabled={ocrBusy}
          aria-busy={ocrBusy}
          onclick={(e) => void ocr(e)}
        >
          {ocrBusy ? "…" : t("shelf.text")}
        </button>
        <button type="button" class="shelf-action" onclick={(e) => void annotate(e)}>
          {t("shelf.draw")}
        </button>
        <button type="button" class="shelf-action" onclick={openFolder}>{t("shelf.folder")}</button>
      </div>

      <p
        class="shelf-note"
        class:is-ok={Boolean(note)}
        role="status"
        aria-live="polite"
      >
        {note ?? current.label ?? current.id}
      </p>
    </div>

    <span id="shelf-tip" class="shelf-tip" role="tooltip">
      {t("shelf.tip")}
    </span>
  </div>

  {#if ghost}
    <div
      class="shelf-ghost"
      class:is-flinging={Boolean(drag?.fling)}
      style="left:{ghost.left}px;top:{ghost.top}px"
    >
      <img {src} alt="" draggable="false" />
    </div>
  {/if}
{/if}

<style>
  /* La ventana es transparente y sin marco: lo único que se ve es la tarjeta.
     Va en `:global` porque `html` y `body` no son de este componente. */
  :global(html),
  :global(body) {
    overflow: hidden;
    margin: 0;
    background: transparent;
  }

  /*
   * Toast/panel: open 400ms + scale/blur/distance; close 350ms y más quieto.
   * CSS transition (no keyframes) para poder interrumpir al reabrir.
   */
  .shelf {
    --shelf-pad: 10px;
    position: relative;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 100%;
    padding: var(--shelf-pad);
    opacity: 0;
    transform: translateY(var(--distance-base, 8px)) scale(var(--float-scale, 0.96));
    filter: blur(var(--float-blur, 2px));
    transform-origin: 100% 100%;
    pointer-events: none;
    transition:
      opacity var(--float-close-dur, 350ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--float-close-dur, 350ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1)),
      filter var(--float-close-dur, 350ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf.is-shown {
    opacity: 1;
    transform: none;
    filter: blur(0);
    pointer-events: auto;
    transition:
      opacity var(--float-open-dur, 400ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--float-open-dur, 400ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1)),
      filter var(--float-open-dur, 400ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf.is-expanded {
    position: absolute;
    width: 256px;
    height: 104px;
  }

  .shelf.is-dragging .shelf-side {
    opacity: 0.4;
    pointer-events: none;
    transition: opacity var(--duration-fast, 125ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf.is-dragging .shelf-tip {
    opacity: 0 !important;
    transition-delay: 0ms;
  }

  .shelf-thumb {
    position: relative;
    display: block;
    flex-shrink: 0;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: grab;
    border-radius: calc(var(--rb-radius-xs, 5px) + 2px);
    touch-action: none;
  }

  .shelf-thumb:active {
    cursor: grabbing;
  }

  .shelf-thumb.is-parked {
    cursor: grabbing;
  }

  .shelf-thumb.is-parked .shelf-thumb-img {
    visibility: hidden;
  }

  .shelf-thumb-img {
    display: block;
    width: 96px;
    height: 64px;
    object-fit: cover;
    border-radius: var(--rb-radius-xs, 5px);
    background: var(--rb-surface, #1e1e1b);
    box-shadow: var(--shadow-pop, 0 8px 24px rgb(0 0 0 / 32%));
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
    transition: transform var(--duration-quick, 75ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf-thumb:hover .shelf-thumb-img {
    transform: scale(1.02);
  }

  .shelf-thumb:active .shelf-thumb-img {
    transform: scale(0.96);
  }

  .shelf-ghost {
    position: fixed;
    z-index: 20;
    width: 96px;
    height: 64px;
    pointer-events: none;
    filter: drop-shadow(0 24px 70px rgb(0 0 0 / 45%));
  }

  .shelf-ghost img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: var(--rb-radius-xs, 5px);
    background: var(--rb-surface, #1e1e1b);
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
    transform: scale(1.04);
  }

  .shelf-ghost.is-flinging {
    transition:
      left var(--duration-fast, 125ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      top var(--duration-fast, 125ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  /*
   * Tip propio (no el `title` nativo del SO): redondeado, soft, delay de
   * intención al aparecer y sin delay al salir. Va al fondo del estante para
   * caber en la ventana transparente.
   */
  .shelf-tip {
    position: absolute;
    left: 50%;
    bottom: 6px;
    z-index: 2;
    box-sizing: border-box;
    padding: 4px 8px;
    border-radius: var(--rb-radius-xs, 5px);
    background: color-mix(in srgb, var(--rb-surface-elevated, #2a2a26) 94%, transparent);
    color: var(--rb-text, #f2f2ee);
    font-size: 11px;
    font-weight: 500;
    line-height: 1.25;
    letter-spacing: 0.01em;
    white-space: nowrap;
    box-shadow:
      0 1px 0 rgb(255 255 255 / 6%) inset,
      0 8px 20px rgb(0 0 0 / 35%);
    outline: 1px solid var(--rb-hairline, rgb(255 255 255 / 12%));
    outline-offset: -1px;
    opacity: 0;
    transform: translateX(-50%) translateY(4px) scale(var(--scale-small, 0.98));
    pointer-events: none;
    transition:
      opacity var(--duration-quick, 150ms) var(--ease-out, ease-out),
      transform var(--duration-quick, 150ms) var(--ease-out, ease-out);
    transition-delay: 0ms;
  }

  .shelf:has(.shelf-thumb:hover) .shelf-tip,
  .shelf:has(.shelf-thumb:focus-visible) .shelf-tip {
    opacity: 1;
    transform: translateX(-50%) translateY(0) scale(1);
    transition-delay: var(--duration-micro, 80ms);
  }

  .shelf:has(.shelf-thumb:hover) .shelf-note,
  .shelf:has(.shelf-thumb:focus-visible) .shelf-note {
    opacity: 0;
    transition-delay: 0ms;
  }

  .shelf-side {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 6px;
  }

  .shelf-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  /* Acciones del estante: chip suave, no el Button soft genérico. */
  .shelf-action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 24px;
    padding: 0 9px;
    border: 0;
    border-radius: var(--rb-radius-xs, 5px);
    background: color-mix(in srgb, var(--rb-surface-elevated, #2a2a26) 88%, transparent);
    color: var(--rb-text, #f2f2ee);
    font: inherit;
    font-size: 11px;
    font-weight: 550;
    letter-spacing: 0.01em;
    outline: 1px solid var(--rb-hairline, rgb(255 255 255 / 12%));
    outline-offset: -1px;
    cursor: pointer;
    transition:
      color var(--duration-quick, 150ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      background-color var(--duration-quick, 150ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--duration-quick, 150ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      outline-color var(--duration-quick, 150ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf-action:hover:not(:disabled) {
    background: color-mix(in srgb, var(--rb-surface-elevated, #2a2a26) 100%, transparent);
    outline-color: var(--rb-hairline-strong, rgb(255 255 255 / 24%));
  }

  .shelf-action:active:not(:disabled) {
    transform: scale(0.96);
  }

  .shelf-action:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .shelf-note {
    max-width: 100%;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: color-mix(in srgb, var(--rb-text, #f2f2ee) 72%, transparent);
    font-size: 11px;
    font-weight: 500;
    line-height: 1.2;
    transition: color var(--duration-fast, 125ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf-note.is-ok {
    color: var(--rb-ok, #3dd68c);
  }

  /* Entrada escalonada: miniatura → acciones → nota (sin blur anidado). */
  .shelf-thumb,
  .shelf-actions,
  .shelf-note {
    opacity: 0;
    transform: translateY(4px);
    transition:
      opacity var(--duration-fast, 250ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--duration-fast, 250ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf.is-shown .shelf-thumb {
    opacity: 1;
    transform: none;
    transition-delay: 0ms;
  }

  .shelf.is-shown .shelf-actions {
    opacity: 1;
    transform: none;
    transition-delay: var(--duration-stagger, 40ms);
  }

  .shelf.is-shown .shelf-note {
    opacity: 1;
    transform: none;
    transition-delay: calc(var(--duration-stagger, 40ms) * 2);
  }

  @media (prefers-reduced-motion: reduce) {
    .shelf,
    .shelf.is-shown,
    .shelf-thumb,
    .shelf-thumb-img,
    .shelf-tip,
    .shelf-action,
    .shelf-actions,
    .shelf-note {
      transition: none !important;
      filter: none !important;
      transform: none !important;
    }

    .shelf {
      opacity: 0;
    }

    .shelf.is-shown {
      opacity: 1;
    }

    .shelf.is-shown .shelf-thumb,
    .shelf.is-shown .shelf-actions,
    .shelf.is-shown .shelf-note {
      opacity: 1;
    }

    .shelf-ghost,
    .shelf-ghost.is-flinging {
      transition: none !important;
    }

    .shelf-ghost img {
      transform: none !important;
    }
  }
</style>
