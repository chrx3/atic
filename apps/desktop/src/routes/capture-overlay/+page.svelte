<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type { OverlayCandidate } from "$lib/types";
  import {
    overlayInfo,
    completeWindowCapture,
    completeRegionCapture,
    completeMonitorCapture,
    cancelCaptureSession,
    showCaptureOverlay,
  } from "$lib/api";

  const DRAG_THRESHOLD = 4;
  const FADE_MS = 120;

  let frameSrc = $state("");
  let candidates = $state<OverlayCandidate[]>([]);
  /** Evita flash negro: solo se revela cuando el frame ya cargó. */
  let revealed = $state(false);

  let hovered = $state<OverlayCandidate | null>(null);
  let region = $state<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);

  let overlayEl: HTMLDivElement | undefined = $state();
  let cursor = $state({ x: 0, y: 0 });
  let dragging = false;
  let dragStart = { x: 0, y: 0 };
  let done = false;
  let initToken = 0;
  let unlistenStart: UnlistenFn | null = null;
  let unlistenEnd: UnlistenFn | null = null;

  const selection = $derived(
    region ??
      (hovered
        ? {
            left: hovered.left,
            top: hovered.top,
            width: hovered.width,
            height: hovered.height,
          }
        : null),
  );

  function resetUi() {
    initToken += 1;
    done = true;
    revealed = false;
    region = null;
    hovered = null;
    dragging = false;
    frameSrc = "";
    candidates = [];
  }

  function sleep(ms: number) {
    return new Promise<void>((resolve) => setTimeout(resolve, ms));
  }

  async function safeClose() {
    try {
      await cancelCaptureSession();
    } catch {
      /* ignora */
    }
  }

  async function run(action: () => Promise<unknown>) {
    if (done) return;
    done = true;
    revealed = false;
    await sleep(FADE_MS);
    try {
      await action();
    } catch (error) {
      console.error("captura falló", error);
      await safeClose();
    }
  }

  function hitTest(x: number, y: number): OverlayCandidate | null {
    for (const c of candidates) {
      if (x >= c.left && x < c.left + c.width && y >= c.top && y < c.top + c.height) {
        return c;
      }
    }
    return null;
  }

  function onMouseMove(e: MouseEvent) {
    cursor = { x: e.clientX, y: e.clientY };
    if (dragging) {
      const w = Math.abs(e.clientX - dragStart.x);
      const h = Math.abs(e.clientY - dragStart.y);
      if (w > DRAG_THRESHOLD || h > DRAG_THRESHOLD) {
        region = {
          left: Math.min(dragStart.x, e.clientX),
          top: Math.min(dragStart.y, e.clientY),
          width: w,
          height: h,
        };
        hovered = null;
      }
    } else {
      hovered = hitTest(e.clientX, e.clientY);
    }
  }

  function onMouseDown(e: MouseEvent) {
    if (!revealed || e.button !== 0) return;
    dragging = true;
    dragStart = { x: e.clientX, y: e.clientY };
    region = null;
  }

  function onMouseUp(e: MouseEvent) {
    if (done || !revealed) return;
    const wasDragging = dragging;
    dragging = false;
    const currentRegion = region;
    if (
      wasDragging &&
      currentRegion &&
      (currentRegion.width > DRAG_THRESHOLD || currentRegion.height > DRAG_THRESHOLD)
    ) {
      run(() =>
        completeRegionCapture(
          currentRegion.left,
          currentRegion.top,
          currentRegion.width,
          currentRegion.height,
        ),
      );
      return;
    }
    region = null;
    const target = hitTest(e.clientX, e.clientY);
    if (target) {
      run(() => completeWindowCapture(target.hwnd));
    } else {
      run(() => safeClose());
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (done) return;
    // Escape SIEMPRE cierra, aunque el frame no haya cargado: si no, un
    // fallo de carga deja la pantalla tapada por el overlay opaco sin salida.
    if (e.key === "Escape") {
      run(() => safeClose());
      return;
    }
    if (!revealed) return;
    if (e.key === " ") {
      e.preventDefault();
      run(() => completeMonitorCapture(cursor.x, cursor.y));
    } else if (e.key === "Enter") {
      if (region) {
        run(() =>
          completeRegionCapture(region!.left, region!.top, region!.width, region!.height),
        );
      } else if (hovered) {
        run(() => completeWindowCapture(hovered!.hwnd));
      }
    }
  }

  function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    if (!revealed) return;
    run(() => safeClose());
  }

  function onFrameLoad() {
    // Pintar el frame mientras la ventana sigue oculta; recién después show.
    // Si show primero, el usuario ve el body #111 (telón gris) hasta el fade.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (done) return;
        revealed = true;
        void showCaptureOverlay().catch((error) => {
          console.error("show_capture_overlay falló", error);
          if (!done) {
            done = true;
            void safeClose();
          }
        });
      });
    });
  }

  /** El PNG congelado no cargó: cerrar en vez de dejar la pantalla tapada. */
  function onFrameError() {
    console.error("frame congelado no cargó");
    if (!done) {
      done = true;
      void safeClose();
    }
  }

  /// Solo corre cuando hay sesión (evento). No llamar al montar: la ventana
  /// existe oculta desde el arranque y aún no hay frame congelado.
  async function init() {
    const token = ++initToken;
    done = false;
    revealed = false;
    region = null;
    hovered = null;
    dragging = false;
    frameSrc = "";
    try {
      const info = await overlayInfo();
      if (token !== initToken) return;
      candidates = info.candidates;
      frameSrc = `${convertFileSrc(info.framePath)}?t=${Date.now()}`;
      // Watchdog: si en 5 s el frame no se reveló (carga colgada), cierra la
      // sesión en vez de dejar un telón opaco sobre el escritorio.
      setTimeout(() => {
        if (token === initToken && !revealed && !done) {
          console.error("overlay sin revelar tras 5s; cerrando sesión");
          done = true;
          void safeClose();
        }
      }, 5000);
    } catch (error) {
      console.error("overlay_info falló", error);
      revealed = false;
      await safeClose();
    }
  }

  onMount(() => {
    const el = overlayEl;
    if (el) {
      el.addEventListener("mousemove", onMouseMove);
      el.addEventListener("mousedown", onMouseDown);
      el.addEventListener("mouseup", onMouseUp);
      el.addEventListener("contextmenu", onContextMenu);
    }
    window.addEventListener("keydown", onKeyDown);

    let cancelled = false;
    void (async () => {
      try {
        unlistenStart = await listen("overlay-session-started", () => {
          void init();
        });
        unlistenEnd = await listen("overlay-session-ended", () => {
          // Si ya hay una sesión nueva (ended viejo llegó tarde), no tumbar el init.
          void overlayInfo()
            .then(() => {
              /* sesión vigente: ignorar */
            })
            .catch(() => {
              resetUi();
            });
        });
      } catch (error) {
        console.error("listen overlay-session falló", error);
      }
      // Si ya hay sesión (p. ej. atajo antes de que el webview escuchara),
      // intenta cargar; si no hay, el error se ignora sin tumbar la página.
      if (!cancelled) {
        try {
          await overlayInfo();
          if (!cancelled) await init();
        } catch {
          /* sin sesión aún: normal al arrancar */
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  });

  onDestroy(() => {
    overlayEl?.removeEventListener("mousemove", onMouseMove);
    overlayEl?.removeEventListener("mousedown", onMouseDown);
    overlayEl?.removeEventListener("mouseup", onMouseUp);
    overlayEl?.removeEventListener("contextmenu", onContextMenu);
    window.removeEventListener("keydown", onKeyDown);
    unlistenStart?.();
    unlistenEnd?.();
  });
</script>

<div class="overlay" class:is-revealed={revealed} bind:this={overlayEl}>
  {#if frameSrc}
    <img
      class="frame"
      src={frameSrc}
      alt=""
      draggable="false"
      onload={onFrameLoad}
      onerror={onFrameError}
    />
  {/if}

  {#if selection}
    <div
      class="spotlight"
      style="left:{selection.left}px; top:{selection.top}px; width:{selection.width}px; height:{selection.height}px;"
    ></div>
    <div
      class="dims"
      style="left:{selection.left}px; top:{Math.max(2, selection.top - 26)}px;"
    >
      {Math.round(selection.width)} × {Math.round(selection.height)}
    </div>
  {:else if revealed}
    <div class="dim-full"></div>
  {/if}

  {#if revealed}
    <div class="hint" style="left:{cursor.x}px;">
      Clic: ventana · Arrastra: región · Espacio: pantalla · Esc cancela
    </div>
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    background: #111;
    cursor: crosshair;
  }

  .overlay {
    position: fixed;
    inset: 0;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    user-select: none;
    opacity: 0;
    transition: opacity var(--duration-quick) var(--ease-smooth-out);
  }
  .overlay.is-revealed {
    opacity: 1;
  }

  .frame {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }

  .dim-full {
    position: absolute;
    inset: 0;
    background: var(--rb-overlay-scrim);
    pointer-events: none;
  }

  .spotlight {
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--rb-overlay-select);
    box-shadow: 0 0 0 100000px var(--rb-overlay-scrim);
    pointer-events: none;
  }

  .dims,
  .hint {
    position: absolute;
    border-radius: var(--rb-radius-xs);
    background: var(--rb-overlay-chip);
    color: var(--rb-overlay-ink);
    font-family: var(--rb-font);
    pointer-events: none;
    white-space: nowrap;
  }

  .dims {
    padding: 2px 6px;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }

  .hint {
    position: fixed;
    bottom: 32px;
    padding: 6px 14px;
    border-radius: var(--rb-radius-sm);
    font-size: 0.8125rem;
    transform: translateX(-50%);
  }
</style>
