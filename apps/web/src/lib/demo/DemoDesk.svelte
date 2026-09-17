<script lang="ts">
  /**
   * El demo: un escritorio simulado con la pill abajo, la rueda que se abre
   * encima y las herramientas como floats.
   *
   * Lo único que este componente decide es la escena —fondo, capas, atajos—;
   * cada herramienta se dibuja en su panel. La pill y la rueda son la misma
   * pieza (`Pill.svelte`), como en la app.
   */
  import { onMount } from "svelte";
  import { pillLayout } from "$atic/lib/core/pillTools";
  import type { ToolId } from "$atic/lib/core/tools";
  import {
    PILL_MORE_IDS,
    PILL_MORE_NODE,
    PILL_RING_IDS,
    PILL_WINDOW_NODE,
  } from "./data";
  import Pill from "./Pill.svelte";
  import LauncherPanel from "./panels/LauncherPanel.svelte";
  import ClipboardPanel from "./panels/ClipboardPanel.svelte";
  import SnippetsPanel from "./panels/SnippetsPanel.svelte";
  import AgentsPanel from "./panels/AgentsPanel.svelte";
  import CapturesPanel from "./panels/CapturesPanel.svelte";
  import ColorPanel from "./panels/ColorPanel.svelte";
  import MeetingsPanel from "./panels/MeetingsPanel.svelte";
  import BoardSurface from "./BoardSurface.svelte";
  import { demo, navigatorLabel } from "./state.svelte";

  let host = $state<HTMLElement | null>(null);
  let hovered = $state(false);
  let handoff = 0;

  const wheelOpen = $derived(demo.phase === "wheel");
  const openTool = $derived(demo.openTool);

  /** Lo que el usuario dejó en la pill: se resuelve como en la app. */
  const layout = pillLayout(PILL_RING_IDS, PILL_MORE_IDS);
  /** Qué anillo se está mirando. `more` es el submenú detrás del gajo «Más». */
  let wheelPage = $state<"ring" | "more">("ring");
  const wheelNodes = $derived(
    wheelPage === "more"
      ? [...layout.more, PILL_WINDOW_NODE]
      : [...layout.ring, PILL_MORE_NODE],
  );

  function closeWheel() {
    demo.closeWheel();
    wheelPage = "ring";
  }

  /** El gajo elegido: la rueda se quita del medio y recién ahí nace el float.
   *  La pizarra no es un float: congela el escritorio entero. */
  function pick(id: string) {
    window.clearTimeout(handoff);
    if (id === PILL_MORE_NODE.id) {
      wheelPage = "more";
      return;
    }
    closeWheel();
    if (id === PILL_WINDOW_NODE.id) {
      demo.toast("La ventana principal es la app de escritorio");
      return;
    }
    if (id === "board") {
      handoff = window.setTimeout(() => demo.openBoard(), 80);
      return;
    }
    handoff = window.setTimeout(() => demo.openToolPanel(id as ToolId), 80);
  }

  function toggleWheel() {
    if (demo.phase === "wheel") closeWheel();
    else demo.openWheel();
  }

  /** El núcleo: en el submenú es «atrás», si no cierra la rueda. */
  function onCenter() {
    if (!wheelOpen) demo.openWheel();
    else if (wheelPage === "more") wheelPage = "ring";
    else closeWheel();
  }

  function openLauncher() {
    window.clearTimeout(handoff);
    demo.phase = "closed";
    demo.openToolPanel("launcher");
  }

  function onKey(event: KeyboardEvent) {
    const interactive = hovered || demo.busy;
    if (!interactive) return;

    if (event.key === "Escape") {
      event.preventDefault();
      demo.escape();
      return;
    }

    // El atajo real de la rueda es Ctrl+Shift+Espacio (`pill_radial_shortcut`).
    // Alt+Z solo aparece en el onboarding de la app; el README está viejo.
    if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.code === "Space") {
      event.preventDefault();
      toggleWheel();
      return;
    }

    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.code === "Space") {
      event.preventDefault();
      openLauncher();
    }
  }

  onMount(() => {
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.clearTimeout(handoff);
    };
  });
</script>

<div
  class="scene"
  role="region"
  aria-label="Demo interactivo de Atic"
  bind:this={host}
  onpointerenter={() => (hovered = true)}
  onpointerleave={() => (hovered = false)}
>
  <div class="desk" class:is-busy={demo.busy}>
    <div class="desk-bg" aria-hidden="true"></div>
    <div class="desk-grid" aria-hidden="true"></div>

    {#if demo.busy}
      <button
        class="catch"
        type="button"
        aria-label="Cerrar"
        onclick={() => demo.escape()}
      ></button>
    {/if}

    <!-- Las herramientas: un float apoyado sobre la pill. -->
    {#if openTool}
      <div class="float-slot" class:is-board={demo.boardOpen}>
        {#if openTool === "launcher"}
          <LauncherPanel onClose={() => demo.closeTool()} />
        {:else if openTool === "clipboard"}
          <ClipboardPanel onClose={() => demo.closeTool()} />
        {:else if openTool === "snippets"}
          <SnippetsPanel onClose={() => demo.closeTool()} />
        {:else if openTool === "agents"}
          <AgentsPanel onClose={() => demo.closeTool()} />
        {:else if openTool === "captures"}
          <CapturesPanel onClose={() => demo.closeTool()} />
        {:else if openTool === "color"}
          <ColorPanel onClose={() => demo.closeTool()} />
        {:else if openTool === "meetings"}
          <MeetingsPanel onClose={() => demo.closeTool()} />
        {/if}
      </div>
    {/if}

    <!-- La pill, que es también el núcleo de la rueda. -->
    <div class="dock" class:is-up={wheelOpen}>
      <Pill tools={wheelNodes} open={wheelOpen} onSelect={pick} onToggle={onCenter} />
    </div>

    {#if demo.boardOpen}
      <BoardSurface image={demo.boardImage} onClose={() => demo.closeBoard()} />
    {/if}

    <div class="toasts" aria-live="polite">
      {#each demo.toasts as toast (toast.id)}
        <div class="toast" class:is-ok={toast.kind === "ok"}>{toast.text}</div>
      {/each}
    </div>
  </div>

  <div class="hints">
    <button type="button" class="hint" onclick={toggleWheel}>
      <kbd>{navigatorLabel()}</kbd><kbd>Shift</kbd><kbd>Espacio</kbd> Herramientas
    </button>
    <button type="button" class="hint" onclick={openLauncher}>
      <kbd>{navigatorLabel()}</kbd><kbd>Espacio</kbd> Apps
    </button>
    <span class="hint is-note">Clic en la pill o en un atajo. Todo muestra datos de ejemplo.</span>
  </div>
</div>

<style>
  .scene {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .desk {
    position: relative;
    height: clamp(560px, 76vh, 800px);
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    /* La forma del escritorio es del demo, no propuesta de la app: acá la
       ventana del sistema es este recuadro. */
    isolation: isolate;
  }

  /* ─── Escritorio ────────────────────────────────────────────────────── */
  .desk-bg {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(900px 620px at 18% 12%, #2f3a63 0%, transparent 62%),
      radial-gradient(820px 700px at 84% 78%, #123f46 0%, transparent 62%),
      linear-gradient(160deg, #14161f 0%, #0d1016 60%, #090b10 100%);
  }

  /* En tema oscuro la piel es oscura y el escritorio se invierte, como en las
     demos de la app: si no, la silueta no se lee. */
  :global([data-theme="dark"]) .desk-bg,
  :global([data-theme="graphite"]) .desk-bg,
  :global([data-theme="midnight"]) .desk-bg,
  :global([data-theme="claude-dark"]) .desk-bg {
    background:
      radial-gradient(900px 620px at 18% 12%, #dfe6f2 0%, transparent 62%),
      radial-gradient(820px 700px at 84% 78%, #cfe4e2 0%, transparent 62%),
      linear-gradient(160deg, #b9c2d1 0%, #aab4c4 60%, #97a2b4 100%);
  }

  .desk-grid {
    position: absolute;
    inset: 0;
    background-image:
      linear-gradient(rgb(255 255 255 / 3.2%) 1px, transparent 1px),
      linear-gradient(90deg, rgb(255 255 255 / 3.2%) 1px, transparent 1px);
    background-size: 48px 48px;
  }

  :global([data-theme="dark"]) .desk-grid,
  :global([data-theme="graphite"]) .desk-grid,
  :global([data-theme="midnight"]) .desk-grid,
  :global([data-theme="claude-dark"]) .desk-grid {
    background-image:
      linear-gradient(rgb(0 0 0 / 4%) 1px, transparent 1px),
      linear-gradient(90deg, rgb(0 0 0 / 4%) 1px, transparent 1px);
  }

  .catch {
    position: absolute;
    inset: 0;
    z-index: 1;
    margin: 0;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: default;
  }

  /* ─── Dock: la pill ─────────────────────────────────────────────────── */
  .dock {
    position: absolute;
    bottom: 28px;
    left: 50%;
    z-index: 3;
    width: 0;
    height: 0;
    transform: translateY(0);
    transition: transform var(--morph-open-dur) var(--morph-ease);
  }

  .dock.is-up {
    /* La rueda mide 252: el disco sube 100 px para dejar lugar a las gotas. */
    transform: translateY(-100px);
  }

  /* ─── Floats ────────────────────────────────────────────────────────── */
  .float-slot {
    position: absolute;
    inset: 16px 12px 96px;
    z-index: 2;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    pointer-events: none;
  }

  .float-slot :global(.float) {
    pointer-events: auto;
  }

  .float-slot.is-board {
    display: none;
  }

  /* ─── Avisos ────────────────────────────────────────────────────────── */
  .toasts {
    position: absolute;
    bottom: 16px;
    left: 16px;
    z-index: 4;
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
    pointer-events: none;
  }

  .toast {
    padding: 7px 11px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--surface) 94%, transparent);
    box-shadow: var(--shadow-pop);
    color: var(--text);
    font-size: var(--text-xs);
    animation: toast-in var(--duration-fast) var(--ease-smooth-out);
  }

  .toast.is-ok {
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
    color: var(--ok);
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.97);
    }
  }

  /* ─── Atajos del demo ───────────────────────────────────────────────── */
  .hints {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--line);
    border-radius: var(--radius-pill);
    padding: 6px 12px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out);
  }

  .hint:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .hint.is-note {
    border-color: transparent;
    color: var(--faint);
    cursor: default;
  }

  kbd {
    padding: 2px 6px;
    border: 1px solid var(--line);
    border-radius: var(--radius-xs);
    background: var(--surface);
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 0.625rem;
  }

  @media (prefers-reduced-motion: reduce) {
    .dock,
    .toast {
      transition: none;
      animation-duration: 1ms;
    }
  }
</style>
