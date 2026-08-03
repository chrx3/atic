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
   */
  import type { CaptureItem } from "$core/types";
  import { MOTION, ms } from "$lib/motion";
  import { activateCapture, captureSrc, ocrCaptureAndCopy } from "$ipc/captures";
  import { openDataDir } from "$ipc/config";
  import { onScreenshotCreated } from "$ipc/captures";
  import { dragOut, hideWindow } from "$ipc/windows";
  import Button from "$ui/Button.svelte";
  import { fly } from "svelte/transition";

  const DISMISS_MS = 6000;

  /** Más que esto en píxeles y el clic pasa a ser un arrastre. */
  const DRAG_THRESHOLD = 5;

  let current = $state<CaptureItem | null>(null);
  let src = $state("");
  let busy = $state(false);
  let ocrBusy = $state(false);

  /** Lo que dijo el OCR. Antes iba solo a la consola y nadie lo veía. */
  let note = $state<string | null>(null);

  let hovering = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let press: { x: number; y: number } | null = null;
  let dragging = false;

  function clearTimer() {
    if (timer) clearTimeout(timer);
    timer = null;
  }

  function hide() {
    clearTimer();
    current = null;
    note = null;
    void hideWindow();
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
      current = item;
      note = null;
      // El sufijo obliga a releer el archivo: dos capturas seguidas pueden
      // compartir ruta y el webview serviría la primera desde su caché.
      src = `${captureSrc(item.path)}?t=${Date.now()}`;
      scheduleDismiss();
    });
    return () => {
      void pending.then((off) => off());
      clearTimer();
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
        ? "Texto copiado al portapapeles"
        : "No se encontró texto en la captura";
    } catch {
      note = "No se pudo extraer el texto";
    } finally {
      ocrBusy = false;
      // Deja leer el resultado en vez de cerrar de golpe.
      timer = setTimeout(hide, 2200);
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

{#if current}
  {#key current.id}
    <div
      class="flex h-full w-full items-center gap-2.5 px-2.5 py-2"
      onmouseenter={() => (hovering = true)}
      onmouseleave={() => (hovering = false)}
      transition:fly={{ x: 40, y: 40, duration: ms(MOTION.fast) }}
      role="group"
      aria-label="Captura reciente"
    >
      <button
        type="button"
        class="block shrink-0 cursor-grab rounded-sm active:cursor-grabbing"
        onmousedown={onDown}
        aria-label="Abrir la captura {current.label || current.id}"
        title="Clic: abrir · Arrastrar: sacar"
      >
        <img
          {src}
          alt=""
          draggable="false"
          class="block h-16 w-24 rounded-sm bg-surface object-cover shadow-pop"
        />
      </button>

      <div class="flex min-w-0 flex-1 flex-col gap-1.5">
        <div class="flex flex-wrap gap-1">
          <Button
            variant="soft"
            size="sm"
            loading={ocrBusy}
            onclick={(e) => void ocr(e)}
          >
            Texto
          </Button>
          <Button variant="soft" size="sm" onclick={openFolder}>Carpeta</Button>
        </div>

        <p
          class="max-w-full truncate rounded-xs bg-surface px-1.5 py-0.5 text-xs
                 font-medium {note ? 'text-ok' : 'text-text'}"
          role="status"
          aria-live="polite"
        >
          {note ?? current.label ?? current.id}
        </p>
      </div>
    </div>
  {/key}
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
</style>
