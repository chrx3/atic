<script lang="ts">
  import { onMount } from "svelte";
  import FloatingExperience from "$lib/floating-v2/FloatingExperience.svelte";
  import { INITIAL_FLOATING_STATE, surfaceForTool, type FloatingActivity, type FloatingSurface } from "$lib/floating-v2/types";
  import { TOOLS } from "$lib/tools";

  let surface = $state<FloatingSurface>(INITIAL_FLOATING_STATE.surface);
  let activity = $state<FloatingActivity>(INITIAL_FLOATING_STATE.activity);
  let showGrid = $state(false);
  let opensUp = $state(false);
  let blend = $state(18);

  onMount(() => {
    const params = new URLSearchParams(window.location.search);
    const requestedSurface = params.get("surface");
    const requestedActivity = params.get("activity");
    opensUp = params.get("direction") === "up";
    const requestedBlend = Number(params.get("blend"));
    if (Number.isFinite(requestedBlend) && requestedBlend >= 0) blend = Math.min(64, requestedBlend);

    if (requestedSurface === "wheel") surface = { kind: "wheel" };
    else if (requestedSurface === "agents") surface = { kind: "agents" };
    else {
      const requestedTool = TOOLS.find((tool) => tool.id === requestedSurface);
      if (requestedTool) surface = surfaceForTool(requestedTool.id);
    }

    if (requestedActivity === "recording" || requestedActivity === "dictating") {
      activity = requestedActivity;
    }

    const onKeydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        surface = { kind: "pill" };
        return;
      }

      const index = Number(event.key) - 1;
      const tool = TOOLS[index];
      if (tool) surface = surfaceForTool(tool.id);
    };

    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });
</script>

<svelte:head>
  <title>Atic · Floating UI V2 Lab</title>
  <meta name="description" content="Laboratorio del renderer flotante compartido de Atic." />
</svelte:head>

<main class:grid-visible={showGrid} class="lab-shell">
  <header class="lab-header">
    <div>
      <span class="eyebrow">ATIC / INTERACTION LAB 02</span>
      <h1>Pearl Flow</h1>
      <p>Una sola superficie que cambia de intención sin dejar de sentirse física.</p>
    </div>
    <div class="lab-meta">
      <span><i class="live-dot"></i> Renderer compartido</span>
      <span>WebView2 / Svelte 5</span>
    </div>
  </header>

  <section class="stage" aria-label="Demostración interactiva de la interfaz flotante">
    <div class="stage-note top-note"><span>01</span> PILL / IDLE</div>
    <div class="stage-note bottom-note"><span>02</span> LIQUID SURFACE</div>
    <FloatingExperience
      {surface}
      {activity}
      {opensUp}
      {blend}
      onSurfaceChange={(next) => (surface = next)}
      onActivityChange={(next) => (activity = next)}
    />
  </section>

  <footer class="lab-controls">
    <div class="control-group">
      <span>ESTADO</span>
      <button class:active={surface.kind === "pill"} type="button" onclick={() => (surface = { kind: "pill" })}>Pill</button>
      <button class:active={surface.kind === "wheel"} type="button" onclick={() => (surface = { kind: "wheel" })}>Rueda</button>
      <button class:active={surface.kind === "agents"} type="button" onclick={() => (surface = { kind: "agents" })}>Agentes</button>
    </div>

    <div class="control-group">
      <span>ACTIVIDAD</span>
      <button class:active={activity === "idle"} type="button" onclick={() => (activity = "idle")}>Reposo</button>
      <button class:active={activity === "recording"} type="button" onclick={() => (activity = "recording")}>Grabando</button>
      <button class:active={activity === "dictating"} type="button" onclick={() => (activity = "dictating")}>Dictando</button>
    </div>

    <label class="blend-control">
      <span>BLEND</span>
      <input type="range" min="0" max="64" step="1" bind:value={blend} />
      <output>{blend}</output>
    </label>

    <button class:active={opensUp} type="button" aria-pressed={opensUp} onclick={() => (opensUp = !opensUp)}>
      Hacia arriba
    </button>

    <button class:active={showGrid} class="grid-toggle" type="button" aria-pressed={showGrid} onclick={() => (showGrid = !showGrid)}>
      Retícula
    </button>
  </footer>

  <p class="keyboard-help"><kbd>1–6</kbd> abre herramientas · <kbd>Esc</kbd> vuelve a la pill</p>
</main>

<style>
  :global(html), :global(body) { min-height: 100%; background: #e9e9e5; }
  :global(body) { overflow: auto; }

  .lab-shell {
    --paper: #e9e9e5;
    --ink: #1c1d1f;
    --muted: #74767a;
    min-height: 100vh;
    padding: 34px 44px 24px;
    color: var(--ink);
    background:
      radial-gradient(circle at 50% 38%, rgba(255, 255, 255, 0.8), transparent 40%),
      var(--paper);
    font-family: "Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif;
  }

  .grid-visible {
    background-image:
      linear-gradient(rgba(28, 29, 31, 0.045) 1px, transparent 1px),
      linear-gradient(90deg, rgba(28, 29, 31, 0.045) 1px, transparent 1px),
      radial-gradient(circle at 50% 38%, rgba(255, 255, 255, 0.8), transparent 40%);
    background-size: 24px 24px, 24px 24px, auto;
  }

  .lab-header {
    display: flex;
    max-width: 1180px;
    margin: 0 auto;
    align-items: flex-end;
    justify-content: space-between;
  }

  .eyebrow, .control-group > span {
    color: var(--muted);
    font-size: 9px;
    font-weight: 720;
    letter-spacing: 0.16em;
  }

  h1 {
    margin: 8px 0 0;
    font-family: "Avenir Next", "Aptos Display", sans-serif;
    font-size: clamp(28px, 4vw, 46px);
    font-weight: 540;
    letter-spacing: -0.045em;
    line-height: 1;
  }

  .lab-header p {
    margin: 10px 0 0;
    color: var(--muted);
    font-size: 12px;
  }

  .lab-meta {
    display: flex;
    gap: 8px;
    flex-direction: column;
    align-items: flex-end;
    color: var(--muted);
    font: 9px var(--rb-mono, "Cascadia Mono", monospace);
  }

  .live-dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-right: 5px;
    border-radius: 50%;
    background: #49a86d;
  }

  .stage {
    position: relative;
    display: grid;
    max-width: 1180px;
    min-height: 650px;
    margin: 18px auto 8px;
    place-items: start center;
    border-top: 1px solid rgba(28, 29, 31, 0.11);
    border-bottom: 1px solid rgba(28, 29, 31, 0.11);
  }

  .stage-note {
    position: absolute;
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--muted);
    font: 8px var(--rb-mono, "Cascadia Mono", monospace);
    letter-spacing: 0.08em;
  }

  .stage-note span { color: var(--ink); }
  .top-note { top: 18px; left: 0; }
  .bottom-note { right: 0; bottom: 18px; }

  .lab-controls {
    display: flex;
    max-width: 1180px;
    margin: 0 auto;
    gap: 28px;
    align-items: center;
  }

  .control-group {
    display: flex;
    gap: 5px;
    align-items: center;
  }

  .control-group > span { margin-right: 7px; }

  .blend-control {
    display: grid;
    min-width: 180px;
    gap: 8px;
    grid-template-columns: auto minmax(80px, 1fr) 22px;
    align-items: center;
    color: var(--muted);
    font: 9px var(--rb-mono, "Cascadia Mono", monospace);
  }

  .blend-control > span {
    font-weight: 720;
    letter-spacing: 0.16em;
  }

  .blend-control input {
    width: 100%;
    accent-color: var(--ink);
  }

  .blend-control output { text-align: right; }

  .lab-controls button {
    min-height: 30px;
    padding: 0 10px;
    border: 1px solid rgba(28, 29, 31, 0.13);
    border-radius: 8px;
    color: var(--muted);
    background: rgba(255, 255, 255, 0.35);
    font: 9px inherit;
    cursor: pointer;
  }

  .lab-controls button:hover { color: var(--ink); background: rgba(255, 255, 255, 0.6); }
  .lab-controls button.active { border-color: var(--ink); color: white; background: var(--ink); }
  .grid-toggle { margin-left: auto; }

  .keyboard-help {
    max-width: 1180px;
    margin: 14px auto 0;
    color: var(--muted);
    font-size: 9px;
    text-align: right;
  }

  kbd {
    padding: 2px 5px;
    border: 1px solid rgba(28, 29, 31, 0.15);
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.4);
    font: inherit;
  }

  @media (max-width: 760px) {
    .lab-shell { padding: 24px 18px; }
    .lab-meta, .stage-note { display: none; }
    .stage { min-height: 580px; overflow: hidden; }
    .lab-controls { flex-wrap: wrap; gap: 10px; }
    .control-group { flex-wrap: wrap; }
    .grid-toggle { margin-left: 0; }
  }
</style>
