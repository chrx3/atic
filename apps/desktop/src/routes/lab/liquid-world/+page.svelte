<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import LiquidSkin from "$lib/floating-v2/liquid/LiquidSkin.svelte";
  import { connectorBridges, type ConnectorGroup } from "$lib/floating-v2/liquid/connectors";
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
    worldLabel: string;
    width: number;
    height: number;
    scale: number;
    revision: number;
    emittedAtMs: number;
    debug: boolean;
    dragging: boolean;
    groups: ConnectorGroup[];
    objects: SnapshotObject[];
  };

  let label = $state("");
  let focusInput: HTMLInputElement | undefined = $state();
  let snapshot = $state<Snapshot>({
    worldLabel: "",
    width: 1,
    height: 1,
    scale: 1,
    revision: 0,
    emittedAtMs: 0,
    debug: false,
    dragging: false,
    groups: [],
    objects: [],
  });
  let renderSamples = $state<number[]>([]);
  let renderCount = 0;
  let deliverySamples = $state<number[]>([]);
  let focused = $state(false);
  let activeElement = $state("none");
  const isWorld = $derived(label.startsWith("liquid-world-"));
  const objectId = $derived(label.replace("liquid-spike-", ""));
  const renderP95 = $derived(percentile(renderSamples, 0.95));
  const renderAverage = $derived(average(renderSamples));
  const deliveryP95 = $derived(percentile(deliverySamples, 0.95));
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
  const bridges = $derived(
    snapshot.debug ? [] : connectorBridges(snapshot.objects, snapshot.groups),
  );

  function recordRender(milliseconds: number) {
    renderCount += 1;
    renderSamples = [...renderSamples.slice(-239), milliseconds];
    if (renderCount % 60 === 0) {
      void invoke("report_liquid_world_metrics", { label, samples: renderSamples.slice(-60) });
    }
  }

  function percentile(samples: number[], quantile: number) {
    if (samples.length === 0) return 0;
    const ordered = [...samples].sort((a, b) => a - b);
    return ordered[Math.round((ordered.length - 1) * quantile)] ?? 0;
  }

  function average(samples: number[]) {
    return samples.length === 0 ? 0 : samples.reduce((total, value) => total + value, 0) / samples.length;
  }

  function acceptSnapshot(value: Snapshot) {
    if (value.worldLabel !== label) return;
    snapshot = value;
    if (value.emittedAtMs > 0) {
      deliverySamples = [...deliverySamples.slice(-239), Math.max(0, Date.now() - value.emittedAtMs)];
    }
  }

  function reportFocus() {
    const active = document.activeElement;
    focused = document.hasFocus();
    activeElement = active instanceof HTMLElement ? `${active.tagName.toLowerCase()}#${active.id || "-"}` : "none";
    void invoke("report_liquid_world_focus", {
      label,
      focused,
      active: activeElement,
    });
  }

  onMount(() => {
    label = getCurrentWindow().label;
    if (!label.startsWith("liquid-world-")) {
      if (label === "liquid-spike-agents") {
        let unlistenFocus: UnlistenFn | undefined;
        const refreshFocus = () => window.setTimeout(reportFocus, 0);
        window.addEventListener("focus", refreshFocus);
        window.addEventListener("blur", refreshFocus);
        document.addEventListener("focusin", refreshFocus);
        document.addEventListener("focusout", refreshFocus);
        window.setTimeout(() => {
          focusInput?.focus();
          reportFocus();
        }, 180);
        void listen("liquid-world-updated", () => reportFocus()).then((off) => (unlistenFocus = off));
        return () => {
          unlistenFocus?.();
          window.removeEventListener("focus", refreshFocus);
          window.removeEventListener("blur", refreshFocus);
          document.removeEventListener("focusin", refreshFocus);
          document.removeEventListener("focusout", refreshFocus);
        };
      }
      return;
    }
    let unlisten: UnlistenFn | undefined;
    void invoke<Snapshot>("liquid_world_snapshot", { label }).then(acceptSnapshot);
    void listen<Snapshot>("liquid-world-updated", (event) => acceptSnapshot(event.payload)).then(
      (off) => (unlisten = off),
    );
    return () => unlisten?.();
  });
</script>

{#if isWorld}
  <main class:dragging={snapshot.dragging && !snapshot.debug} class="world" aria-hidden="true">
    <LiquidSkin
      {boxes}
      {bridges}
      parameters={snapshot.debug ? { k: 42, cell: 5, smooth: 2 } : { k: 34, cell: 4, smooth: 2 }}
      width={snapshot.width}
      height={snapshot.height}
      duration={snapshot.debug ? 90 : 0}
      onRender={recordRender}
    />
    {#if snapshot.debug}
      <output class="telemetry" aria-label="Telemetria del compositor">
        <span>SDF p95 <strong>{renderP95.toFixed(2)} ms</strong></span>
        <span>media {renderAverage.toFixed(2)} ms</span>
        <span>evento p95 {deliveryP95.toFixed(1)} ms</span>
        <span>{renderSamples.length} frames / rev {snapshot.revision} / {snapshot.scale.toFixed(2)}x</span>
      </output>
    {/if}
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
        <output class:verified={focused && activeElement.startsWith("input#")} class="focus-state">
          {focused && activeElement.startsWith("input#") ? "Foco conservado" : "Sin foco de escritura"}
        </output>
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
  .world {
    position: relative;
    width: 100%;
    height: 100%;
    opacity: 1;
    pointer-events: none;
    transition: opacity 100ms ease-out;
  }

  /* Durante el drag el cuerpo local permanece pegado al contenido. La piel
     completa vuelve a aparecer solo cuando ya recibió la geometría final. */
  .world.dragging {
    opacity: 0;
    transition-duration: 35ms;
  }
  .telemetry {
    position: absolute;
    top: 12px;
    right: 12px;
    display: flex;
    gap: 6px 12px;
    flex-wrap: wrap;
    max-width: 520px;
    padding: 7px 10px;
    color: oklch(0.94 0.006 255);
    border-radius: 8px;
    background: oklch(0.18 0.012 255 / 0.88);
    font: 11px/1.35 "Cascadia Mono", monospace;
  }

  .telemetry strong { color: white; font-weight: 650; }

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
  .focus-state {
    justify-self: start;
    padding: 4px 8px;
    color: oklch(0.42 0.014 255);
    border-radius: 999px;
    background: oklch(0.22 0.012 255 / 0.07);
    font-size: 10px;
  }
  .focus-state.verified {
    color: oklch(0.42 0.13 154);
    background: oklch(0.78 0.13 154 / 0.18);
  }

  :global(:root[data-theme="dark"]) .pill-ink,
  :global(:root[data-theme="dark"]) .surface-ink { color: oklch(0.9 0.008 255); }
  :global(:root[data-theme="dark"]) section p,
  :global(:root[data-theme="dark"]) header small { color: oklch(0.7 0.012 255); }
</style>
