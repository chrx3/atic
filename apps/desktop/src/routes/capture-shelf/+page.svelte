<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { fly } from "svelte/transition";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import type { CaptureItem } from "$lib/types";
  import { onScreenshotCreated, activateCapture } from "$lib/api";

  const DISMISS_MS = 6000;
  const DRAG_THRESHOLD = 5;

  let current = $state<CaptureItem | null>(null);
  let src = $state("");
  let timer: ReturnType<typeof setTimeout> | null = null;
  let unlisten: UnlistenFn[] = [];

  let down: { x: number; y: number } | null = null;
  let dragged = false;
  let busy = false;

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function hide() {
    clearTimer();
    current = null;
    getCurrentWindow().hide();
  }

  function scheduleDismiss() {
    clearTimer();
    timer = setTimeout(() => {
      if (!busy) hide();
    }, DISMISS_MS);
  }

  function show(item: CaptureItem) {
    current = item;
    src = `${convertFileSrc(item.path)}?t=${Date.now()}`;
    scheduleDismiss();
  }

  async function doDrag() {
    if (!current) return;
    busy = true;
    clearTimer();
    try {
      await startDrag({ item: [current.path], icon: current.path });
    } catch (error) {
      console.error("arrastre falló", error);
    }
    busy = false;
    // Una vez arrastrada, la notificación se va.
    hide();
  }

  async function doActivate() {
    if (!current) return;
    busy = true;
    try {
      await activateCapture(current.path);
    } catch (error) {
      console.error("abrir captura falló", error);
    }
    busy = false;
    hide();
  }

  function cleanupPress() {
    down = null;
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  }

  function onMove(e: MouseEvent) {
    if (!down || dragged) return;
    if (Math.hypot(e.clientX - down.x, e.clientY - down.y) > DRAG_THRESHOLD) {
      dragged = true;
      cleanupPress();
      doDrag();
    }
  }

  function onUp() {
    const wasClick = down !== null && !dragged;
    cleanupPress();
    // Clic sin arrastre → abrir preview / ubicación (según config).
    if (wasClick) doActivate();
  }

  function onDown(e: MouseEvent) {
    if (e.button !== 0) return;
    down = { x: e.clientX, y: e.clientY };
    dragged = false;
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  onMount(async () => {
    unlisten.push(await onScreenshotCreated((item) => show(item)));
  });

  onDestroy(() => {
    for (const off of unlisten) off();
    clearTimer();
    cleanupPress();
  });
</script>

{#if current}
  {#key current.id}
    <div class="thumb" transition:fly={{ x: 40, y: 40, duration: 220 }}>
      <button class="grab" onmousedown={onDown} title="Clic: abrir · Arrastra: sacar">
        <img src={src} alt="captura" draggable="false" />
      </button>
      <div class="name">{current.id}</div>
    </div>
  {/key}
{/if}

<style>
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
  }

  .thumb {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    width: fit-content;
  }

  .grab {
    display: block;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    cursor: grab;
  }
  .grab:active {
    cursor: grabbing;
  }
  .grab img {
    display: block;
    max-width: 180px;
    max-height: 110px;
    width: auto;
    height: auto;
    border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.55);
  }

  .name {
    max-width: 180px;
    font: 12px system-ui, sans-serif;
    color: #fff;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.95);
    overflow-wrap: anywhere;
  }
</style>
