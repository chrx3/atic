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
  } from "$lib/api";

  const DRAG_THRESHOLD = 4;
  const FADE_MS = 120;

  let frameSrc = $state("");
  let candidates: OverlayCandidate[] = [];
  /** Evita flash negro: solo se revela cuando el frame ya cargó. */
  let revealed = $state(false);

  let hovered = $state<OverlayCandidate | null>(null);
  let region = $state<{
    left: number;
    top: number;
    width: number;
    height: number;
  } | null>(null);

  let overlayEl: HTMLDivElement;
  let cursor = $state({ x: 0, y: 0 });
  let dragging = false;
  let dragStart = { x: 0, y: 0 };
  let done = false; // evita capturar dos veces
  let initToken = 0;

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

  function sleep(ms: number) {
    return new Promise<void>((resolve) => setTimeout(resolve, ms));
  }

  /// Oculta el overlay (cancela la sesión en el backend). NO cierra la ventana:
  /// destruirla provoca un crash de wry en WM_SETFOCUS. Se reutiliza después.
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
    // `candidates` viene topmost-first: el primero que contiene el punto gana.
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
      // Clic en vacío (sin ventana): cancelar, igual que Esc.
      run(() => safeClose());
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (done || !revealed) return;
    if (e.key === "Escape") {
      run(() => safeClose());
    } else if (e.key === " ") {
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
    // Clic derecho = cancelar (salida de respaldo).
    e.preventDefault();
    if (!revealed) return;
    run(() => safeClose());
  }

  function onFrameLoad() {
    // Un frame para que el paint del <img> ocurra antes del fade-in.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (!done) revealed = true;
      });
    });
  }

  let unlistenStart: UnlistenFn | null = null;

  /// (Re)inicializa la sesión: el webview se reutiliza entre capturas, así que
  /// esto corre en cada `overlay-session-started`, no solo al montar.
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
      // Cache-bust: el archivo se reescribe con el mismo nombre cada sesión.
      frameSrc = `${convertFileSrc(info.framePath)}?t=${Date.now()}`;
    } catch (error) {
      console.error("overlay_info falló", error);
      await safeClose();
    }
  }

  onMount(async () => {
    overlayEl.addEventListener("mousemove", onMouseMove);
    overlayEl.addEventListener("mousedown", onMouseDown);
    overlayEl.addEventListener("mouseup", onMouseUp);
    overlayEl.addEventListener("contextmenu", onContextMenu);
    window.addEventListener("keydown", onKeyDown);
    unlistenStart = await listen("overlay-session-started", () => init());
    await init();
  });

  onDestroy(() => {
    overlayEl?.removeEventListener("mousemove", onMouseMove);
    overlayEl?.removeEventListener("mousedown", onMouseDown);
    overlayEl?.removeEventListener("mouseup", onMouseUp);
    overlayEl?.removeEventListener("contextmenu", onContextMenu);
    window.removeEventListener("keydown", onKeyDown);
    unlistenStart?.();
  });
</script>

<div class="overlay" class:is-revealed={revealed} bind:this={overlayEl}>
  {#if frameSrc}
    <img class="frame" src={frameSrc} alt="" draggable="false" onload={onFrameLoad} />
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
  {:else}
    <div class="dim-full"></div>
  {/if}

  <div class="hint" style="left:{cursor.x}px;">
    Clic: ventana · Arrastra: región · Espacio: pantalla · Esc cancela
  </div>
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
    transition: opacity 140ms ease-out;
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
    background: rgba(0, 0, 0, 0.28);
    pointer-events: none;
  }

  .spotlight {
    position: absolute;
    border: 2px solid #2f9e44;
    box-shadow: 0 0 0 100000px rgba(0, 0, 0, 0.28);
    box-sizing: border-box;
    pointer-events: none;
  }

  .dims {
    position: absolute;
    background: rgba(0, 0, 0, 0.75);
    color: #fff;
    font: 12px system-ui, sans-serif;
    padding: 2px 6px;
    border-radius: 4px;
    pointer-events: none;
    white-space: nowrap;
  }

  .hint {
    position: fixed;
    bottom: 32px;
    transform: translateX(-50%);
    background: rgba(0, 0, 0, 0.75);
    color: #fff;
    font: 13px system-ui, sans-serif;
    padding: 6px 14px;
    border-radius: 8px;
    pointer-events: none;
    white-space: nowrap;
  }
</style>
