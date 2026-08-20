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
  import { MOTION, ms } from "$lib/motion";
  import {
    activateCapture,
    captureSrc,
    ocrCaptureAndCopy,
    onScreenshotCreated,
  } from "$ipc/captures";
  import { openAnnotator } from "$ipc/annotate";
  import { openDataDir } from "$ipc/config";
  import { dragOut, hideWindow } from "$ipc/windows";
  import { tick } from "svelte";

  const DISMISS_MS = 6000;

  /** Más que esto en píxeles y el clic pasa a ser un arrastre. */
  const DRAG_THRESHOLD = 5;

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
    current = item;
    note = null;
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

  function hide() {
    clearTimer();
    shown = false;
    const closeMs = ms(MOTION.floatClose);
    clearHideTimer();
    hideTimer = setTimeout(() => {
      if (shown) return;
      current = null;
      note = null;
      alive = false;
      void hideWindow();
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
      releasePress();
    };
  });

  async function drag() {
    if (!current) return;
    busy = true;
    clearTimer();
    try {
      await dragOut(current.path);
    } finally {
      busy = false;
      hide();
    }
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

  // --- Clic contra arrastre ---
  //
  // El mismo botón hace las dos cosas, así que la decisión se toma por
  // distancia: si el puntero se movió, es un arrastre; si se soltó donde
  // empezó, es un clic. Sin el umbral, cualquier temblor de mano se convertía
  // en un arrastre fallido.

  function releasePress() {
    press = null;
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  }

  function onMove(event: MouseEvent) {
    if (!press || dragging) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) > DRAG_THRESHOLD) {
      dragging = true;
      releasePress();
      void drag();
    }
  }

  function onUp() {
    const wasClick = press !== null && !dragging;
    releasePress();
    if (wasClick) void activate();
  }

  function onDown(event: MouseEvent) {
    if (event.button !== 0) return;
    press = { x: event.clientX, y: event.clientY };
    dragging = false;
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
</script>

{#if alive && current}
  <div
    class="shelf"
    class:is-shown={shown}
    onmouseenter={() => (hovering = true)}
    onmouseleave={() => (hovering = false)}
    role="group"
    aria-label={t("shelf.recent")}
  >
    <button
      type="button"
      class="shelf-thumb"
      onmousedown={onDown}
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

  .shelf-thumb {
    position: relative;
    display: block;
    flex-shrink: 0;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: grab;
    border-radius: calc(var(--rb-radius-xs, 5px) + 2px);
  }

  .shelf-thumb:active {
    cursor: grabbing;
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
    transition: transform var(--duration-quick, 150ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .shelf-thumb:active .shelf-thumb-img {
    transform: scale(0.96);
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
  }
</style>
