<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import LiquidSkin from "$lib/floating-v2/liquid/LiquidSkin.svelte";
  import type { LiquidBox } from "$lib/floating-v2/liquid/engine";

  type SnapshotObject = {
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
    radius: number;
  };

  type Snapshot = {
    width: number;
    height: number;
    scale: number;
    objects: SnapshotObject[];
  };

  let label = $state("");
  let focusInput: HTMLInputElement | undefined = $state();
  let snapshot = $state<Snapshot>({ width: 1, height: 1, scale: 1, objects: [] });
  let renderSamples: number[] = [];
  const isWorld = $derived(label === "liquid-world");
  const objectId = $derived(label.replace("liquid-spike-", ""));
  const boxes = $derived<LiquidBox[]>(
    snapshot.objects.map((object) => ({
      id: object.id,
      cx: object.x + object.width / 2,
      cy: object.y + object.height / 2,
      hw: object.width / 2,
      hh: object.height / 2,
      r: object.radius,
    })),
  );

  function recordRender(milliseconds: number) {
    renderSamples.push(milliseconds);
    if (renderSamples.length < 60) return;
    const samples = renderSamples;
    renderSamples = [];
    void invoke("report_liquid_world_metrics", { samples });
  }

  function reportFocus() {
    const active = document.activeElement;
    void invoke("report_liquid_world_focus", {
      label,
      focused: document.hasFocus(),
      active: active instanceof HTMLElement ? `${active.tagName.toLowerCase()}#${active.id || "-"}` : "none",
    });
  }

  onMount(() => {
    label = getCurrentWindow().label;
    if (label !== "liquid-world") {
      if (label === "liquid-spike-agents") {
        let unlistenFocus: UnlistenFn | undefined;
        window.setTimeout(() => {
          focusInput?.focus();
          reportFocus();
        }, 180);
        void listen("liquid-world-updated", () => reportFocus()).then((off) => (unlistenFocus = off));
        return () => unlistenFocus?.();
      }
      return;
    }
    let unlisten: UnlistenFn | undefined;
    void invoke<Snapshot>("liquid_world_snapshot").then((value) => (snapshot = value));
    void listen<Snapshot>("liquid-world-updated", (event) => (snapshot = event.payload)).then(
      (off) => (unlisten = off),
    );
    return () => unlisten?.();
  });
</script>

{#if isWorld}
  <main class="world" aria-hidden="true">
    <LiquidSkin
      {boxes}
      parameters={{ k: 34, cell: 5, smooth: 2 }}
      width={snapshot.width}
      height={snapshot.height}
      duration={90}
      onRender={recordRender}
    />
  </main>
{:else if objectId === "pill"}
  <button class="pill-ink" type="button" data-tauri-drag-region aria-label="Mover núcleo de Atic">
    <span></span>
  </button>
{:else}
  <main class:agents={objectId === "agents"} class="surface-ink">
    <header data-tauri-drag-region>
      <span class="object-mark"></span>
      <div>
        <strong>{objectId === "agents" ? "Agentes" : "Reuniones"}</strong>
        <small>{objectId === "agents" ? "Superficie persistente" : "Objeto líquido"}</small>
      </div>
      <i aria-hidden="true"></i>
    </header>
    <section>
      {#if objectId === "agents"}
        <p>El campo conserva foco mientras la piel vive en otra ventana.</p>
        <input bind:this={focusInput} aria-label="Mensaje de prueba" placeholder="Escribe para probar el foco…" />
      {:else}
        <button type="button"><span></span>Iniciar reunión</button>
        <p>Arrastra el encabezado para acercar este objeto a la pill o a Agentes.</p>
      {/if}
    </section>
  </main>
{/if}

<style>
  :global(html), :global(body) {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  :global(body) { margin: 0; }
  .world { position: relative; width: 100%; height: 100%; pointer-events: none; }

  .pill-ink {
    display: grid;
    width: 100%;
    height: 100%;
    padding: 0;
    place-items: center;
    border: 0;
    color: oklch(0.22 0.012 255);
    background: transparent;
    cursor: grab;
  }

  .pill-ink:active { cursor: grabbing; }
  .pill-ink span, .object-mark {
    width: 14px;
    height: 14px;
    border: 4px solid currentColor;
    border-radius: 50%;
    box-sizing: border-box;
  }

  .surface-ink {
    display: grid;
    width: 100%;
    height: 100%;
    grid-template-rows: 62px 1fr;
    color: oklch(0.22 0.012 255);
    font-family: "Segoe UI Variable", "Aptos", system-ui, sans-serif;
  }

  header {
    display: flex;
    gap: 10px;
    padding: 14px 18px 10px;
    align-items: center;
    cursor: grab;
    user-select: none;
  }

  header:active { cursor: grabbing; }
  header div { display: flex; min-width: 0; flex: 1; flex-direction: column; }
  header strong { font-size: 13px; font-weight: 680; }
  header small { margin-top: 3px; color: oklch(0.48 0.014 255); font-size: 10px; }
  header i { width: 8px; height: 8px; border-radius: 50%; background: oklch(0.62 0.18 252); }

  section { display: grid; gap: 12px; padding: 18px; align-content: center; }
  section p { max-width: 42ch; margin: 0; color: oklch(0.42 0.014 255); font-size: 11px; line-height: 1.45; }
  section button, section input {
    min-height: 38px;
    border: 0;
    border-radius: 10px;
    font: 12px inherit;
  }
  section button { color: white; background: oklch(0.22 0.012 255); cursor: pointer; }
  section input { padding: 0 12px; color: inherit; background: oklch(1 0 0 / 0.48); outline: 1px solid oklch(0.22 0.012 255 / 0.12); }
  section input:focus { outline: 2px solid oklch(0.62 0.18 252 / 0.62); }

  @media (prefers-color-scheme: dark) {
    .pill-ink, .surface-ink { color: oklch(0.9 0.008 255); }
    section p, header small { color: oklch(0.7 0.012 255); }
  }
</style>
