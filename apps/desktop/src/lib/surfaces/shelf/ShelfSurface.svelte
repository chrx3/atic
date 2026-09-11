<script lang="ts">
  /**
   * El estante: la tarjeta que aparece abajo tras una captura.
   *
   * Es efímera por diseño. Se va sola según `capture_shelf_timeout_seconds`
   * (20 s de fábrica; 0 la deja hasta que la cierren) porque su trabajo es
   * ofrecer lo que uno hace con una captura recién tomada —arrastrarla,
   * abrirla, sacarle el texto— y pasado ese momento estorba. Todo lo demás vive
   * en la herramienta de capturas de la ventana principal.
   *
   * La cuenta atrás se pausa si hay algo en curso o el puntero está encima:
   * la tarjeta no puede desaparecer justo cuando la mano va a agarrarla.
   *
   * Abrir/cerrar usa el mismo patrón interruptible que `.float-emerge`
   * (`alive` + `shown`): la ventana nativa solo se oculta al terminar el
   * repliegue, si no el outro nunca se ve.
   */
  import type { CaptureItem } from "$core/types";
  import { t } from "$domain/i18n.svelte";
  import { MOTION, afterTransition, ms, wait } from "$lib/motion";
  import {
    activateCapture,
    captureSrc,
    copyCaptureImage,
    ocrCaptureAndCopy,
    onScreenshotCreated,
  } from "$ipc/captures";
  import { openAnnotator } from "$ipc/annotate";
  import {
    pasteToExternalHwnd,
    startFileDrag,
    tryClipboardDropOnAgents,
  } from "$ipc/clipboard";
  import { getConfig, openDataDir } from "$ipc/config";
  import { setOverlayItemDrag, overlayCursorOverHit } from "$ipc/overlay";
  import Icon from "$ui/Icon.svelte";
  import { Folder, Pencil, ScanText, X } from "$lib/icons";
  import {
    coverShelfMonitor,
    hideWindow,
    restoreShelfBounds,
    shelfForeignHwnd,
    type PhysicalBounds,
    type ShelfCover,
  } from "$ipc/windows";
  import { tick } from "svelte";

  /** Coincide con el default de config. 0 en ajustes = no se va sola. */
  const DEFAULT_TTL_MS = 20_000;

  /** Más que esto en píxeles y el clic pasa a ser un arrastre. */
  const DRAG_THRESHOLD = 5;

  /** Recorrido hacia el borde para soltar el toast. */
  const DISCARD_PX = 56;

  /** Cerca de este margen, soltar cuenta como descarte aunque el recorrido sea corto. */
  const EDGE_ZONE = 40;

  const THUMB_W = 192;
  const THUMB_H = 120;
  const SHELF_PAD = 8;
  /** Sobre otra app, esperar esto antes de pasar a OLE (un roce no cuenta). */
  const OLE_DWELL_MS = 380;
  const FOREIGN_CHECK_MS = 80;

  type DiscardDir = "x" | "y";

  let current = $state<CaptureItem | null>(null);
  let src = $state("");
  let busy = $state(false);
  let ocrBusy = $state(false);

  /** Lo que dijo el OCR. Antes iba solo a la consola y nadie lo veía. */
  let note = $state<string | null>(null);
  /** Tono del aviso: verde solo para el éxito; el error tiene el suyo. */
  let noteTone = $state<"ok" | "error" | null>(null);

  /** Sigue montada (aunque se esté replegando). */
  let alive = $state(false);
  /** Ya emergió: dispara la transición de entrada. */
  let shown = $state(false);

  let hovering = $state(false);
  /** ms hasta ocultarse. `null` = se queda hasta que la cierren. */
  let ttlMs = $state<number | null>(DEFAULT_TTL_MS);
  let remainingMs = DEFAULT_TTL_MS;
  /** Duración visual de la barra; se alinea al remaining al (re)armar. */
  let barMs = $state(DEFAULT_TTL_MS);
  let tickStarted = 0;
  /** Reinicia la barra de cuenta atrás en cada captura nueva. */
  let ttlEpoch = $state(0);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let press: { x: number; y: number } | null = null;
  let dragging = false;
  let leftSide = false;
  let restBounds: PhysicalBounds | null = null;
  let shelfSlot = $state<{ left: number; top: number } | null>(null);
  let drag = $state<{ fling: boolean } | null>(null);
  let expanding = false;
  let fileDragStarted = false;
  let dropping = false;
  let oleSince = 0;
  let oleHwnd = 0;
  let lastForeignCheck = 0;
  let pointerId: number | null = null;
  let thumbEl: HTMLButtonElement | undefined = $state();
  let shelfEl = $state<HTMLElement | null>(null);
  let ghostEl: HTMLDivElement | undefined = $state();
  let ghostLive = $state(false);
  let ghostFling = $state(false);
  let ghostDiscarding = $state(false);
  let ghostPos = { left: 0, top: 0 };
  /** El resize del HWND dispara lostcapture / move sin botón: no es un soltar. */
  let holdGesture = false;
  let ignoreLostCapture = false;
  /** Soltaste mientras el HWND crecía: aplicar el up al terminar. */
  let pendingUp = false;
  /**
   * El HWND está creciendo a pantalla completa. La tarjeta se esconde para
   * que no se estire un frame en la esquina (0,0) del monitor.
   */
  let covering = $state(false);
  let cover: ShelfCover | null = null;
  /** Offset del clic dentro del thumb, en CSS: el ghost sigue al cursor. */
  let grab = { x: 0, y: 0 };
  let lastScreen = { x: 0, y: 0 };

  function clearTimer() {
    if (timer) clearTimeout(timer);
    timer = null;
  }

  /** Presenta la tarjeta: un frame replegada para que la transición tenga origen. */
  async function present(item: CaptureItem) {
    drag = null;
    clearGhost();
    covering = false;
    cover = null;
    pendingUp = false;
    fileDragStarted = false;
    dropping = false;
    oleSince = 0;
    oleHwnd = 0;
    holdGesture = false;
    ignoreLostCapture = false;
    endPress();
    await restoreWindow();
    current = item;
    note = null;
    noteTone = null;
    void getConfig()
      .then((cfg) => {
        leftSide = cfg.capture_shelf_side === "left";
        const secs = cfg.capture_shelf_timeout_seconds;
        ttlMs =
          Number.isFinite(secs) && secs > 0
            ? secs * 1000
            : secs === 0
              ? null
              : DEFAULT_TTL_MS;
        if (alive && shown) {
          remainingMs = ttlMs ?? 0;
          ttlEpoch += 1;
          armDismiss(true);
        }
      })
      .catch(() => {
        leftSide = false;
      });
    // El sufijo obliga a releer el archivo: dos capturas seguidas pueden
    // compartir ruta y el webview serviría la primera desde su caché.
    src = `${captureSrc(item.path)}?t=${Date.now()}`;
    alive = true;
    shown = false;
    ttlEpoch += 1;
    remainingMs = ttlMs ?? 0;
    await tick();
    requestAnimationFrame(() => {
      shown = true;
      armDismiss(true);
    });
  }

  async function restoreWindow() {
    const snap = restBounds;
    restBounds = null;
    cover = null;
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
    covering = true;
    holdGesture = true;
    ignoreLostCapture = true;
    try {
      const covered = await coverShelfMonitor();
      if (!covered) {
        covering = false;
        dragging = false;
        drag = null;
        return;
      }
      restBounds = covered.rest;
      cover = covered;
      // El ghost usa coords de pantalla: se puede pintar ya, sin esperar al
      // viewport. La tarjeta sigue oculta para no flashar en (0,0).
      showGhost(ghostFromScreen(covered));
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
      recapture();
      if (drag && !drag.fling) pinGhost(ghostFromScreen(covered));
      covering = false;
      await setOverlayItemDrag(true).catch(() => {});
    } catch {
      covering = false;
      dragging = false;
      drag = null;
    } finally {
      expanding = false;
      recapture();
      window.setTimeout(() => {
        holdGesture = false;
        ignoreLostCapture = false;
      }, 200);
      if (pendingUp) {
        pendingUp = false;
        onUp();
      }
    }
  }

  /** CSS del monitor a partir de screenX/Y. No usa clientX: tras el cover el origen cambia. */
  function ghostFromScreen(covered: ShelfCover): { left: number; top: number } {
    const dpr = window.devicePixelRatio || 1;
    return {
      left: lastScreen.x - covered.monX / dpr - grab.x,
      top: lastScreen.y - covered.monY / dpr - grab.y,
    };
  }

  function pinGhost(pos: { left: number; top: number }) {
    ghostPos = pos;
    if (ghostEl)
      ghostEl.style.transform = `translate3d(${pos.left}px, ${pos.top}px, 0)`;
  }

  function showGhost(pos: { left: number; top: number }) {
    ghostPos = pos;
    ghostFling = false;
    ghostLive = true;
    void tick().then(() => pinGhost(pos));
  }

  function clearGhost() {
    ghostLive = false;
    ghostFling = false;
    ghostDiscarding = false;
    ghostEl = undefined;
  }

  function followCursor() {
    if (!cover || ghostFling) return;
    const pos = ghostFromScreen(cover);
    pinGhost(pos);
    const discarding = discardDirOf(pos) !== null;
    if (ghostDiscarding !== discarding) ghostDiscarding = discarding;
  }

  async function hide() {
    clearTimer();
    covering = false;
    hovering = false;
    shown = false;
    const closeMs = ms(MOTION.fast);
    if (shelfEl) await afterTransition(shelfEl, "opacity", closeMs);
    else await wait(closeMs);
    if (shown) return;
    void restoreWindow().finally(() => {
      current = null;
      note = null;
      noteTone = null;
      alive = false;
      drag = null;
      clearGhost();
      covering = false;
      cover = null;
      void hideWindow();
    });
  }

  function armDismiss(reset: boolean) {
    clearTimer();
    if (ttlMs === null) return;
    if (reset) {
      if (remainingMs <= 0) remainingMs = ttlMs;
      barMs = remainingMs;
    }
    if (busy || ocrBusy || hovering || !shown || remainingMs <= 0) return;
    tickStarted = Date.now();
    timer = setTimeout(() => {
      timer = null;
      remainingMs = 0;
      hide();
    }, remainingMs);
  }

  function pauseDismiss() {
    if (!timer) return;
    remainingMs = Math.max(0, remainingMs - (Date.now() - tickStarted));
    clearTimer();
  }

  function scheduleDismiss() {
    armDismiss(false);
  }

  $effect(() => {
    const pending = onScreenshotCreated((item) => {
      void present(item);
    });
    return () => {
      void pending.then((off) => off());
      clearTimer();
      endPress();
      drag = null;
      clearGhost();
    };
  });

  function slotOrigin(): { left: number; top: number } | null {
    if (!shelfSlot) return null;
    return { left: shelfSlot.left + SHELF_PAD, top: shelfSlot.top + SHELF_PAD };
  }

  /** ¿El gesto va al borde de descarte (abajo o el lado más cercano)? */
  function discardDirOf(pos: { left: number; top: number }): DiscardDir | null {
    const origin = slotOrigin();
    if (!origin) return null;
    const down = pos.top - origin.top;
    const outward = leftSide ? origin.left - pos.left : pos.left - origin.left;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const distBottom = vh - (pos.top + THUMB_H);
    const distOuter = leftSide ? pos.left : vw - (pos.left + THUMB_W);
    const wantDown = down >= DISCARD_PX || (down > 12 && distBottom < EDGE_ZONE);
    const wantOut = outward >= DISCARD_PX || (outward > 12 && distOuter < EDGE_ZONE);
    if (wantDown && wantOut) return down >= outward ? "y" : "x";
    if (wantDown) return "y";
    if (wantOut) return "x";
    return null;
  }

  async function dropOnForeign(hwnd: number, item: CaptureItem) {
    if (dropping) return;
    dropping = true;
    clearGhost();
    drag = null;
    covering = false;
    shown = false;
    await copyCaptureImage(item.path).catch(() => {});
    await hideWindow().catch(() => {});
    await wait(60);
    busy = true;
    try {
      await pasteToExternalHwnd(hwnd);
    } finally {
      busy = false;
      await restoreWindow();
      hide();
    }
  }

  async function beginOle(item: CaptureItem) {
    if (fileDragStarted || dropping || !dragging) return;
    fileDragStarted = true;
    dropping = true;
    clearGhost();
    shown = false;
    covering = false;
    await hideWindow().catch(() => {});
    await setOverlayItemDrag(true).catch(() => {});
    busy = true;
    try {
      await startFileDrag([item.path]).catch(() => {});
      await tryClipboardDropOnAgents(`capture-${item.id}`).catch(() => false);
    } finally {
      await setOverlayItemDrag(false).catch(() => {});
      busy = false;
      await restoreWindow();
      hide();
    }
  }

  async function considerOle(item: CaptureItem) {
    if (fileDragStarted || !dragging || expanding) return;
    const hwnd = await shelfForeignHwnd().catch(() => 0);
    if (!hwnd || !dragging) {
      oleHwnd = 0;
      oleSince = 0;
      return;
    }
    if (oleHwnd !== hwnd) {
      oleHwnd = hwnd;
      oleSince = Date.now();
      return;
    }
    if (Date.now() - oleSince < OLE_DWELL_MS) return;
    void beginOle(item);
  }

  async function finishDiscard(dir: DiscardDir) {
    if (!ghostLive) {
      hide();
      return;
    }
    drag = { fling: true };
    ghostFling = true;
    const extra = 280;
    pinGhost({
      left: dir === "x" ? ghostPos.left + (leftSide ? -extra : extra) : ghostPos.left,
      top: dir === "y" ? ghostPos.top + extra : ghostPos.top,
    });
    await wait(ms(MOTION.fast));
    hide();
  }

  async function snapBack() {
    if (!ghostLive) {
      await restoreWindow();
      covering = false;
      return;
    }
    drag = { fling: true };
    ghostFling = true;
    const origin = slotOrigin();
    if (origin) pinGhost(origin);
    await wait(ms(MOTION.fast));
    drag = null;
    clearGhost();
    covering = false;
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
      const copied = text.trim().length > 0;
      note = copied ? t("page.captures.ocrCopied") : t("page.captures.ocrEmpty");
      noteTone = copied ? "ok" : null;
    } catch {
      note = t("page.captures.ocrFail");
      noteTone = "error";
    } finally {
      ocrBusy = false;
      if (ttlMs !== null) {
        // Deja leer el resultado en vez de cerrar de golpe.
        remainingMs = 2200;
        ttlEpoch += 1;
        armDismiss(true);
      }
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
      noteTone = "error";
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
    if (holdGesture || ignoreLostCapture || expanding || dragging) {
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
      if (holdGesture || expanding || ignoreLostCapture) return;
      if (press && dragging) {
        recapture();
        return;
      }
      onUp();
      return;
    }
    const rawX = event.screenX - press.x;
    const rawY = event.screenY - press.y;
    if (!dragging) {
      if (Math.hypot(rawX, rawY) <= DRAG_THRESHOLD) return;
      dragging = true;
      clearTimer();
      drag = { fling: false };
      covering = true;
      void expandForDrag();
      return;
    }
    followCursor();
    const item = current;
    if (!item || expanding || fileDragStarted) return;
    if (ghostLive && discardDirOf(ghostPos)) {
      oleHwnd = 0;
      oleSince = 0;
      return;
    }
    const now = Date.now();
    if (now - lastForeignCheck < FOREIGN_CHECK_MS) return;
    lastForeignCheck = now;
    void considerOle(item);
  }

  async function dropOnAgents(item: CaptureItem) {
    clearGhost();
    drag = null;
    covering = false;
    await restoreWindow();
    try {
      await tryClipboardDropOnAgents(`capture-${item.id}`).catch(() => false);
    } finally {
      hide();
    }
  }

  function onUp() {
    if (expanding) {
      pendingUp = true;
      return;
    }
    if (!press && !dragging) {
      endPress();
      return;
    }
    const wasClick = press !== null && !dragging;
    const wasDragging = dragging;
    const pos = ghostLive ? ghostPos : null;
    dragging = false;
    endPress();
    if (fileDragStarted) return;
    if (wasClick) {
      covering = false;
      void activate();
      return;
    }
    if (!wasDragging) return;
    if (!pos) {
      clearGhost();
      covering = false;
      void restoreWindow();
      scheduleDismiss();
      return;
    }
    const dir = discardDirOf(pos);
    if (dir) {
      void finishDiscard(dir);
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
      const hwnd = await shelfForeignHwnd().catch(() => 0);
      if (hwnd) {
        await dropOnForeign(hwnd, item);
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
    dropping = false;
    oleHwnd = 0;
    oleSince = 0;
    drag = null;
    clearGhost();
    covering = false;
    lastScreen = { x: event.screenX, y: event.screenY };
    const r = thumbEl?.getBoundingClientRect();
    grab = r ? { x: event.clientX - r.left, y: event.clientY - r.top } : { x: 0, y: 0 };
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
    if (event.key !== "Escape" || !alive) return;
    event.preventDefault();
    if (drag || fileDragStarted) {
      dragging = false;
      endPress();
      void snapBack();
      return;
    }
    hide();
  }}
/>

{#if alive && current}
  <div
    bind:this={shelfEl}
    class="shelf"
    class:is-shown={shown}
    class:is-expanded={shelfSlot !== null}
    class:is-dragging={drag !== null || covering}
    class:is-covering={covering}
    style={shelfSlot ? `left:${shelfSlot.left}px;top:${shelfSlot.top}px` : ""}
    onmouseenter={() => {
      hovering = true;
      pauseDismiss();
    }}
    onmouseleave={() => {
      hovering = false;
      armDismiss(false);
    }}
    role="group"
    aria-label={t("shelf.recent")}
  >
    <button
      type="button"
      bind:this={thumbEl}
      class="shelf-thumb"
      class:is-parked={ghostLive}
      onpointerdown={onDown}
      onlostpointercapture={onLostCapture}
      aria-label={t("shelf.open", { label: current.label || current.id })}
      aria-describedby="shelf-tip"
    >
      <img {src} alt="" draggable="false" class="shelf-thumb-img" />
    </button>

    <div class="shelf-veil">
      <button
        type="button"
        class="shelf-dot is-tl"
        onpointerdown={(e) => e.stopPropagation()}
        onclick={(e) => {
          e.stopPropagation();
          void hide();
        }}
        aria-label={t("shelf.dismiss")}
      >
        <Icon icon={X} size={14} />
      </button>
      <button
        type="button"
        class="shelf-dot is-tr"
        onpointerdown={(e) => e.stopPropagation()}
        onclick={openFolder}
        aria-label={t("shelf.folder")}
      >
        <Icon icon={Folder} size={14} />
      </button>

      <div class="shelf-center">
        <button
          type="button"
          class="shelf-sub"
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => void annotate(e)}
        >
          <Icon icon={Pencil} size={13} />
          {t("shelf.draw")}
        </button>
      </div>

      <button
        type="button"
        class="shelf-dot is-bl"
        disabled={ocrBusy}
        aria-busy={ocrBusy}
        onpointerdown={(e) => e.stopPropagation()}
        onclick={(e) => void ocr(e)}
        aria-label={t("shelf.text")}
      >
        <Icon icon={ScanText} size={14} />
      </button>
    </div>

    {#if note}
      <p
        class="shelf-note"
        class:is-ok={noteTone === "ok"}
        class:is-error={noteTone === "error"}
        role="status"
        aria-live="polite"
      >
        {note}
      </p>
    {/if}

    <span id="shelf-tip" class="shelf-tip" role="tooltip">
      {t("shelf.tip")}
    </span>

    {#if ttlMs}
      {#key ttlEpoch}
        <span
          class="shelf-ttl"
          class:is-paused={hovering || busy || ocrBusy}
          style:--shelf-ttl="{barMs}ms"
          aria-hidden="true"
        ></span>
      {/key}
    {/if}
  </div>

  {#if ghostLive}
    <div
      bind:this={ghostEl}
      class="shelf-ghost"
      class:is-flinging={ghostFling}
      class:is-discarding={ghostDiscarding && !ghostFling}
      style="transform: translate3d({ghostPos.left}px, {ghostPos.top}px, 0)"
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
   * Foto flotante: nace 150ms, se va 125ms. Sin blur (caro en PNG) y sin el
   * `--float-scale` 0.55 de la pill — acá es un recorte, no un morph.
   */
  .shelf {
    --shelf-pad: 8px;
    --shelf-open: var(--duration-medium, 150ms);
    --shelf-close: var(--duration-fast, 125ms);

    position: relative;
    box-sizing: border-box;
    display: block;
    width: 100%;
    height: 100%;
    padding: var(--shelf-pad);
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    opacity: 0;
    transform: translateY(var(--distance-base, 8px)) scale(var(--scale-large, 0.96));
    transform-origin: 100% 100%;
    pointer-events: none;
    transition:
      opacity var(--shelf-close) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--shelf-close)
        var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf.is-shown {
    opacity: 1;
    transform: none;
    pointer-events: auto;
    transition:
      opacity var(--shelf-open) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--shelf-open) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf.is-expanded {
    position: absolute;
    width: 208px;
    height: 136px;
  }

  /*
   * El HWND pasa de 208×136 a pantalla completa. Sin esto la tarjeta se estira
   * un frame en (0,0) del monitor: el preview "salta" a la esquina.
   */
  .shelf.is-covering,
  .shelf.is-shown.is-covering {
    opacity: 0 !important;
    visibility: hidden;
    pointer-events: none;
    transition: none !important;
  }

  .shelf.is-shown.is-dragging .shelf-veil,
  .shelf.is-dragging .shelf-ttl {
    opacity: 0 !important;
    pointer-events: none;
  }

  .shelf.is-dragging .shelf-tip {
    opacity: 0 !important;
    transition-delay: 0ms;
  }

  .shelf-thumb {
    position: relative;
    display: block;
    width: 192px;
    height: 120px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: grab;
    border-radius: 8px;
    touch-action: none;
    transform-origin: center center;
  }

  .shelf-thumb:active {
    cursor: grabbing;
  }

  .shelf-thumb.is-parked {
    cursor: grabbing;
  }

  .shelf.is-shown.is-dragging .shelf-thumb {
    opacity: 0.2;
  }

  .shelf-thumb.is-parked .shelf-thumb-img {
    visibility: hidden;
  }

  .shelf-thumb-img {
    display: block;
    width: 192px;
    height: 120px;
    object-fit: contain;
    border-radius: 8px;
    background: transparent;
    outline: 1px solid rgb(255 255 255 / 16%);
    outline-offset: -1px;
    box-shadow: 0 4px 8px rgb(0 0 0 / 38%);
  }

  .shelf-thumb:active .shelf-thumb-img {
    transform: scale(0.96);
  }

  .shelf.is-dragging .shelf-thumb:active .shelf-thumb-img,
  .shelf-thumb.is-parked:active .shelf-thumb-img {
    transform: none;
  }

  .shelf-ghost {
    position: fixed;
    left: 0;
    top: 0;
    z-index: 20;
    width: 192px;
    height: 120px;
    pointer-events: none;
    filter: drop-shadow(0 4px 8px rgb(0 0 0 / 40%));
    transition: none;
  }

  .shelf-ghost img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 8px;
    background: transparent;
    outline: 1px solid rgb(255 255 255 / 16%);
    outline-offset: -1px;
  }

  .shelf-ghost.is-discarding {
    opacity: 0.72;
  }

  .shelf-ghost.is-flinging {
    opacity: 0;
    transition:
      transform var(--duration-fast, 125ms)
        var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      opacity var(--duration-fast, 125ms)
        var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf-tip {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .shelf-veil {
    position: absolute;
    inset: var(--shelf-pad);
    z-index: 2;
    border-radius: 8px;
    background: transparent;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--duration-quick, 75ms) var(--ease-out, ease-out);
  }

  .shelf:hover .shelf-veil,
  .shelf:focus-within .shelf-veil {
    opacity: 1;
    pointer-events: none;
  }

  .shelf-dot,
  .shelf-sub {
    pointer-events: auto;
  }

  .shelf-dot {
    position: absolute;
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: rgb(18 18 16 / 82%);
    color: #f4f4ee;
    cursor: pointer;
    box-shadow: 0 1px 2px rgb(0 0 0 / 40%);
    outline: 1px solid rgb(255 255 255 / 16%);
    outline-offset: -1px;
    opacity: 0;
    transform: scale(0.96);
    transition:
      opacity var(--duration-quick, 75ms) var(--ease-out, ease-out),
      background-color var(--duration-quick, 75ms) var(--ease-out, ease-out),
      transform var(--duration-quick, 75ms) var(--ease-out, ease-out);
  }

  .shelf-dot.is-tl {
    top: 8px;
    left: 8px;
  }

  .shelf-dot.is-tr {
    top: 8px;
    right: 8px;
  }

  .shelf-dot.is-bl {
    bottom: 8px;
    left: 8px;
  }

  .shelf:hover .shelf-dot,
  .shelf:focus-within .shelf-dot {
    opacity: 1;
    transform: none;
  }

  .shelf-dot:hover:not(:disabled) {
    background: rgb(8 8 7 / 94%);
  }

  .shelf-dot:focus-visible {
    outline: 2px solid #f4f4ee;
    outline-offset: 1px;
  }

  .shelf-dot:active:not(:disabled) {
    transform: scale(0.96);
  }

  .shelf-dot:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .shelf-center {
    position: absolute;
    top: 50%;
    left: 50%;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    min-width: 5.5rem;
    transform: translate(-50%, -50%) translateY(4px);
    opacity: 0;
    transition:
      opacity var(--duration-quick, 75ms) var(--ease-out, ease-out),
      transform var(--duration-quick, 75ms) var(--ease-out, ease-out);
  }

  .shelf:hover .shelf-center,
  .shelf:focus-within .shelf-center {
    opacity: 1;
    transform: translate(-50%, -50%);
  }

  .shelf-sub {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    min-height: 28px;
    border: 0;
    border-radius: 8px;
    padding: 0 12px;
    background: rgb(18 18 16 / 78%);
    color: #f4f4ee;
    font: inherit;
    font-size: 12px;
    font-weight: 650;
    cursor: pointer;
    outline: 1px solid rgb(255 255 255 / 16%);
    outline-offset: -1px;
    transition:
      background-color var(--duration-quick, 150ms) var(--ease-out, ease-out),
      transform var(--duration-quick, 150ms) var(--ease-out, ease-out);
  }

  .shelf-sub:hover {
    background: rgb(8 8 7 / 90%);
  }

  .shelf-sub:focus-visible {
    outline: 2px solid #f4f4ee;
    outline-offset: 2px;
  }

  .shelf-sub:active {
    transform: scale(0.96);
  }

  .shelf-note {
    position: absolute;
    right: 10px;
    bottom: 10px;
    z-index: 3;
    max-width: calc(100% - 48px);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #f4f4ee;
    font-size: 11px;
    font-weight: 650;
    text-shadow: 0 1px 2px rgb(0 0 0 / 80%);
    pointer-events: none;
  }

  .shelf-note.is-ok {
    color: #9dffc4;
  }

  .shelf-note.is-error {
    color: #ffb4ad;
  }

  /* Anillo propio: el shelf vive sobre la captura, no bajo `.atic-root`. */
  .shelf :where(button):focus-visible {
    outline: 2px solid #f4f4ee;
    outline-offset: 2px;
  }

  .shelf-ttl {
    position: absolute;
    right: var(--shelf-pad);
    bottom: 4px;
    left: var(--shelf-pad);
    height: 2px;
    border-radius: 1px;
    background: color-mix(in srgb, var(--rb-accent, #7aa2f7) 75%, transparent);
    transform: scaleX(1);
    transform-origin: left center;
    pointer-events: none;
    animation: shelf-ttl var(--shelf-ttl, 20s) linear forwards;
  }

  .shelf-ttl.is-paused {
    animation-play-state: paused;
  }

  @keyframes shelf-ttl {
    from {
      transform: scaleX(1);
    }

    to {
      transform: scaleX(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .shelf,
    .shelf.is-shown,
    .shelf-thumb,
    .shelf-thumb-img,
    .shelf-tip,
    .shelf-veil,
    .shelf-dot,
    .shelf-center,
    .shelf-sub,
    .shelf-note,
    .shelf-ttl {
      transition: none !important;
      filter: none !important;
      transform: none !important;
      animation: none !important;
    }

    .shelf-ttl {
      opacity: 0;
    }

    .shelf {
      opacity: 0;
    }

    .shelf.is-shown {
      opacity: 1;
    }

    .shelf.is-shown .shelf-thumb {
      opacity: 1;
    }

    .shelf-ghost,
    .shelf-ghost.is-flinging,
    .shelf-ghost.is-discarding {
      opacity: 1;
      transition: none !important;
    }

    .shelf.is-covering,
    .shelf.is-shown.is-covering {
      transition: none !important;
    }

    .shelf-ghost img {
      transform: none !important;
    }
  }
</style>
