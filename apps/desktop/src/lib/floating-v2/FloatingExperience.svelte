<script lang="ts">
  import type { Snippet } from "svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { TOOLS, toolById, type ToolId } from "$lib/tools";
  import LiquidSkin from "./liquid/LiquidSkin.svelte";
  import { sceneFor, WHEEL_POSITIONS } from "./liquid/scene";
  import {
    surfaceForTool,
    type FloatingActivity,
    type FloatingPlacement,
    type FloatingSurface,
    type PillRestShape,
  } from "./types";
  import { pillPresentationFor } from "./geometry";

  let {
    surface,
    activity = "idle",
    placement = "free",
    layout = "stage",
    opensUp = false,
    pillRestShape = "circle",
    blend,
    interactive = true,
    queueCount = 0,
    agentCount = 0,
    agentWorking = false,
    selectedTool = null,
    content,
    onSurfaceChange = () => {},
    onActivityChange = () => {},
    onQueueOpen = () => {},
    onAgentOpen = () => onSurfaceChange({ kind: "agents" }),
    onToolSelect = (tool: ToolId) => onSurfaceChange(surfaceForTool(tool)),
    onToolHighlight = () => {},
  }: {
    surface: FloatingSurface;
    activity?: FloatingActivity;
    placement?: FloatingPlacement;
    layout?: "stage" | "window";
    opensUp?: boolean;
    pillRestShape?: PillRestShape;
    blend?: number;
    interactive?: boolean;
    queueCount?: number;
    agentCount?: number;
    agentWorking?: boolean;
    selectedTool?: ToolId | null;
    content?: Snippet<[FloatingSurface]>;
    onSurfaceChange?: (surface: FloatingSurface) => void;
    onActivityChange?: (activity: FloatingActivity) => void;
    onQueueOpen?: () => void;
    onAgentOpen?: () => void;
    onToolSelect?: (tool: ToolId) => void;
    onToolHighlight?: (tool: ToolId | null) => void;
  } = $props();

  let sceneWidth = $state(760);
  let sceneHeight = $state(620);
  const wheelOpen = $derived(surface.kind === "wheel");
  const panelOpen = $derived(surface.kind === "tool" || surface.kind === "agents" || surface.kind === "queue");
  const agentOpen = $derived(surface.kind === "agents");
  const activeTool = $derived(surface.kind === "tool" ? toolById(surface.tool) : null);
  const hasAttention = $derived(queueCount > 0 || agentCount > 0 || agentWorking);
  const pillShape = $derived(pillPresentationFor(surface, activity, pillRestShape, hasAttention));
  const liquidScene = $derived(
    sceneFor(surface, Math.max(1, sceneWidth), Math.max(1, sceneHeight), opensUp, pillShape),
  );
  const liquidParameters = $derived({
    ...liquidScene.parameters,
    k: blend ?? liquidScene.parameters.k,
  });
  const stateLabel = $derived(
    activity === "recording" ? "Grabando reunión" : activity === "dictating" ? "Escuchando dictado" : "Atic listo",
  );
  const pillTitle = $derived(
    activity === "recording"
      ? "Grabando"
      : activity === "dictating"
        ? "Dictando"
        : queueCount > 0
          ? `${queueCount} ${queueCount === 1 ? "pendiente" : "pendientes"}`
          : agentCount > 0
            ? `${agentCount} ${agentCount === 1 ? "aviso" : "avisos"}`
          : "Atic",
  );
  const pillSubtitle = $derived(
    activity !== "idle"
      ? "Toca para abrir"
      : queueCount > 0
        ? "Listo para pegar"
        : agentCount > 0
          ? "Agente listo"
          : agentWorking
            ? "Agente trabajando"
            : "Herramientas",
  );

  function toggleWheel() {
    if (!interactive) return;
    onSurfaceChange(wheelOpen ? { kind: "pill" } : { kind: "wheel" });
  }

  function activatePill() {
    if (!interactive) return;
    if (activity === "idle" && queueCount > 0) onQueueOpen();
    else if (activity === "idle" && agentCount > 0) onAgentOpen();
    else toggleWheel();
  }

  function openTool(tool: ToolId) {
    if (!interactive) return;
    onToolSelect(tool);
  }

  function closePanel() {
    if (!interactive) return;
    onSurfaceChange({ kind: "pill" });
  }

  function toggleActivity(next: Exclude<FloatingActivity, "idle">) {
    if (!interactive) return;
    onActivityChange(activity === next ? "idle" : next);
  }
</script>

<div
  class:wheel-open={wheelOpen}
  class:panel-open={panelOpen}
  class:agent-open={agentOpen}
  class:edge-placement={placement === "edge"}
  class:window-layout={layout === "window"}
  class:opens-up={opensUp && panelOpen}
  class:active={activity !== "idle"}
  class:agent-working={activity === "idle" && agentWorking}
  class:circle-pill={pillShape === "circle"}
  class="floating-experience"
  data-activity={activity}
  bind:clientWidth={sceneWidth}
  bind:clientHeight={sceneHeight}
>
  <LiquidSkin
    boxes={liquidScene.boxes}
    bridges={liquidScene.bridges}
    parameters={liquidParameters}
    width={sceneWidth}
    height={sceneHeight}
  />

  <div class="floating-ink">
    <div class="pill-control">
      <button
        class="pill-main"
        type="button"
        aria-label={queueCount > 0 && activity === "idle" ? "Abrir cola de pegado" : agentCount > 0 && activity === "idle" ? "Abrir respuestas de agentes" : wheelOpen ? "Cerrar herramientas" : "Abrir herramientas"}
        onclick={activatePill}
      >
        <span class:has-count={(queueCount > 0 || agentCount > 0) && activity === "idle"} class="status-orb" aria-hidden="true">
          {#if (queueCount > 0 || agentCount > 0) && activity === "idle"}
            {@const count = queueCount > 0 ? queueCount : agentCount}
            <b>{Math.min(count, 9)}{count > 9 ? "+" : ""}</b>
          {/if}
          {#if activity !== "idle" || agentWorking}<span class="status-pulse"></span>{/if}
        </span>
        <span class="pill-copy">
          <strong>{pillTitle}</strong>
          <small>{pillSubtitle}</small>
        </span>
      </button>
      <button
        class="pill-glyph"
        type="button"
        aria-label={wheelOpen ? "Cerrar herramientas" : "Abrir herramientas"}
        aria-expanded={wheelOpen}
        onclick={toggleWheel}
      ><i></i><i></i></button>
    </div>

    <div class="wheel-controls" aria-hidden={!wheelOpen}>
      {#each TOOLS as tool, index (tool.id)}
        <button
          class:selected={selectedTool === tool.id}
          class="wheel-control"
          style:--node-x={`${WHEEL_POSITIONS[index].x}px`}
          style:--node-y={`${WHEEL_POSITIONS[index].y}px`}
          type="button"
          tabindex={wheelOpen ? 0 : -1}
          aria-label={`${tool.label}: ${tool.short}`}
          onfocus={() => onToolHighlight(tool.id)}
          onmouseenter={() => onToolHighlight(tool.id)}
          onmouseleave={() => onToolHighlight(null)}
          onclick={() => openTool(tool.id)}
        >
          <ToolIcon id={tool.id} size={21} />
          <span>{tool.label}</span>
        </button>
      {/each}
    </div>

    <section class="tool-surface" inert={!panelOpen} aria-hidden={!panelOpen} aria-label={agentOpen ? "Agentes" : surface.kind === "queue" ? "Cola de pegado" : activeTool?.label}>
      <header class="surface-header">
        <div class="surface-identity">
          {#if surface.kind === "tool"}
            <ToolIcon id={surface.tool} size={19} />
          {:else if surface.kind === "queue"}
            <ToolIcon id="clipboard" size={19} />
          {:else}
            <ToolIcon id="agents" size={19} />
          {/if}
          <div>
            <strong>{agentOpen ? "Agentes" : surface.kind === "queue" ? "Pegado pendiente" : activeTool?.label}</strong>
            <span>{agentOpen ? "Espacio de trabajo" : surface.kind === "queue" ? "Cola local" : activeTool?.short}</span>
          </div>
        </div>
        <button class="close-control" type="button" tabindex={panelOpen ? 0 : -1} aria-label="Cerrar panel" onclick={closePanel}>
          <span aria-hidden="true"></span>
        </button>
      </header>

      <div class="surface-body">
        {#if content}
          {@render content(surface)}
        {:else if surface.kind === "tool" && surface.tool === "meetings"}
          <div class="hero-action">
            <button class:running={activity === "recording"} type="button" onclick={() => toggleActivity("recording")}>
              <span class="record-mark"></span>
              {activity === "recording" ? "Detener grabación" : "Nueva reunión"}
            </button>
            <p>{activity === "recording" ? "12:48 · Audio del sistema y micrófono" : "Transcribe localmente y resume al terminar."}</p>
          </div>
          <div class="recent-list">
            <span class="section-kicker">RECIENTES</span>
            <article><span>Hoy</span><strong>Revisión de producto</strong><small>28 min · Resumen listo</small></article>
            <article><span>Ayer</span><strong>Planificación semanal</strong><small>42 min · 6 acuerdos</small></article>
          </div>
        {:else if surface.kind === "tool" && surface.tool === "dictation"}
          <div class="dictation-state">
            <div class:running={activity === "dictating"} class="voice-orb"><i></i><i></i><i></i><i></i><i></i></div>
            <strong>{activity === "dictating" ? "Te escucho…" : "Dictado listo"}</strong>
            <p>El texto aparecerá en la aplicación que estés usando.</p>
            <button type="button" onclick={() => toggleActivity("dictating")}>
              {activity === "dictating" ? "Terminar" : "Empezar a dictar"}
            </button>
          </div>
        {:else if surface.kind === "tool" && surface.tool === "clipboard"}
          <div class="search-field"><span aria-hidden="true">⌕</span><span>Buscar en el historial</span><kbd>⌘ K</kbd></div>
          <div class="clip-list">
            <button type="button"><span class="clip-type">TXT</span><span><strong>La propuesta actualizada queda lista…</strong><small>Hace 2 min · Outlook</small></span></button>
            <button type="button"><span class="clip-type image">IMG</span><span><strong>Captura 2026-07-31</strong><small>1440 × 900 · Hace 18 min</small></span></button>
            <button type="button"><span class="clip-type">URL</span><span><strong>arlan.me/vault/liquid-ui</strong><small>Hace 1 h · Chrome</small></span></button>
          </div>
        {:else if surface.kind === "tool" && surface.tool === "snippets"}
          <div class="snippet-grid">
            <button type="button"><small>SEGUIMIENTO</small><strong>Hola, ¿cómo estás? Quería retomar…</strong><span>Usado 14 veces</span></button>
            <button type="button"><small>REUNIÓN</small><strong>Te comparto el resumen y los acuerdos…</strong><span>Usado 8 veces</span></button>
            <button class="new-snippet" type="button"><strong>＋</strong><span>Nuevo texto</span></button>
          </div>
        {:else if surface.kind === "tool" && surface.tool === "captures"}
          <div class="capture-options">
            <button type="button"><span class="capture-icon region"></span><strong>Seleccionar área</strong><small>Arrastra para recortar</small></button>
            <button type="button"><span class="capture-icon screen"></span><strong>Pantalla completa</strong><small>Captura el escritorio</small></button>
          </div>
          <p class="surface-note">Las capturas quedan en el portapapeles y en el shelf flotante.</p>
        {:else if surface.kind === "agents"}
          <div class="agents-layout">
            <aside>
              <button class="new-chat" type="button">＋ Nueva sesión</button>
              <span class="section-kicker">HOY</span>
              <button class="session active-session" type="button"><strong>UI flotante V2</strong><small>Codex · ahora</small></button>
              <button class="session" type="button"><strong>Revisar transcripción</strong><small>Claude · 34 min</small></button>
            </aside>
            <div class="conversation">
              <header><span class="agent-avatar">C</span><span><strong>Codex</strong><small>Trabajando en atic</small></span><i></i></header>
              <div class="messages">
                <p class="user-message">Haz que la experiencia se sienta como una sola pieza viva.</p>
                <div class="agent-message"><span>Estoy unificando pill, rueda y herramientas en un renderer compartido. El lab ya no será una maqueta separada.</span><small>Renderer · 3 archivos modificados</small></div>
              </div>
              <div class="composer"><span>Escribe un mensaje…</span><kbd>↵</kbd></div>
            </div>
          </div>
        {:else}
          <div class="empty-state"><strong>Elige una herramienta</strong><p>{stateLabel}</p></div>
        {/if}
      </div>
    </section>
  </div>
</div>

<style>
  .floating-experience {
    --pearl: oklch(0.965 0.006 255);
    --pearl-deep: oklch(0.915 0.009 255);
    --ink: oklch(0.22 0.012 255);
    --muted: oklch(0.48 0.014 255);
    --signal: oklch(0.62 0.18 252);
    --record: oklch(0.61 0.2 28);
    --line: oklch(0.22 0.012 255 / 0.12);
    --soft-line: oklch(0.22 0.012 255 / 0.075);
    --surface-shadow: 0 32px 80px oklch(0.2 0.015 255 / 0.18), 0 4px 16px oklch(0.2 0.015 255 / 0.08);
    position: relative;
    width: min(760px, 100%);
    height: 620px;
    color: var(--ink);
    font-family: "Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif;
    font-size: 14px;
    isolation: isolate;
    pointer-events: none;
  }

  .floating-experience.window-layout {
    width: 100%;
    height: 100%;
  }

  .floating-ink {
    position: absolute;
    inset: 0;
    z-index: 1;
  }

  .pill-control {
    position: absolute;
    top: 32px;
    left: 50%;
    display: grid;
    grid-template-columns: 28px 1fr 22px;
    align-items: center;
    width: 154px;
    height: 50px;
    padding: 0 10px 0 7px;
    border-radius: 25px;
    color: var(--ink);
    background: transparent;
    pointer-events: auto;
    transform: translateX(-50%);
  }

  .circle-pill .pill-control {
    top: 8px;
    width: 48px;
    height: 48px;
    padding: 0;
    grid-template-columns: 1fr;
  }

  .circle-pill .pill-main {
    width: 48px;
    height: 48px;
    padding: 0;
    grid-column: 1;
    grid-template-columns: 1fr;
    place-items: center;
  }

  .circle-pill .pill-copy,
  .circle-pill .pill-glyph {
    display: none;
  }

  .pill-main {
    display: grid;
    min-width: 0;
    height: 42px;
    padding: 0 4px;
    grid-column: 1 / 3;
    grid-template-columns: 28px 1fr;
    align-items: center;
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }

  .pill-control:focus-visible,
  button:focus-visible {
    outline: 2px solid color-mix(in oklch, var(--signal) 70%, white);
    outline-offset: 3px;
  }

  .status-orb {
    position: relative;
    display: grid;
    width: 20px;
    height: 20px;
    place-items: center;
    border-radius: 50%;
    background: var(--ink);
  }

  .status-orb::after {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--pearl);
    content: "";
  }

  .status-orb.has-count::after { display: none; }
  .status-orb b { color: white; font-size: 8px; font-weight: 760; }

  .active .status-orb {
    background: var(--record);
  }

  .agent-working .status-orb { background: var(--signal); }

  .status-pulse {
    position: absolute;
    inset: -4px;
    border: 1px solid color-mix(in oklch, var(--record) 55%, transparent);
    border-radius: 50%;
    animation: quiet-pulse 1.8s ease-out infinite;
  }

  .agent-working .status-pulse {
    border-color: color-mix(in oklch, var(--signal) 55%, transparent);
  }

  .pill-copy {
    display: flex;
    min-width: 0;
    padding-left: 4px;
    flex-direction: column;
    align-items: flex-start;
    line-height: 1.05;
  }

  .pill-copy strong {
    font-size: 12px;
    font-weight: 680;
    letter-spacing: -0.01em;
  }

  .pill-copy small {
    margin-top: 4px;
    color: var(--muted);
    font-size: 9px;
  }

  .pill-glyph {
    position: relative;
    display: block;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
  }

  .pill-glyph i {
    position: absolute;
    top: 8px;
    left: 3px;
    width: 12px;
    height: 1.5px;
    border-radius: 2px;
    background: currentColor;
    transition: transform 220ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .pill-glyph i:last-child {
    transform: rotate(90deg);
  }

  .wheel-open .pill-glyph i:first-child {
    transform: rotate(45deg);
  }

  .wheel-open .pill-glyph i:last-child {
    transform: rotate(135deg);
  }

  .wheel-controls {
    position: absolute;
    top: 34px;
    left: 50%;
  }

  .wheel-control {
    position: absolute;
    top: 0;
    left: 0;
    display: grid;
    width: 52px;
    height: 52px;
    padding: 0;
    place-items: center;
    border: 0;
    border-radius: 50%;
    color: var(--ink);
    background: transparent;
    cursor: pointer;
    opacity: 0;
    pointer-events: none;
    transform: translate(-50%, 8px) scale(0.48);
    transition:
      transform 260ms cubic-bezier(0.22, 1, 0.36, 1),
      opacity 160ms ease;
  }

  .wheel-control span {
    position: absolute;
    top: calc(100% + 8px);
    left: 50%;
    padding: 4px 7px;
    border: 1px solid var(--soft-line);
    border-radius: 999px;
    color: var(--muted);
    background: color-mix(in oklch, var(--pearl) 92%, transparent);
    font-size: 9px;
    font-weight: 620;
    white-space: nowrap;
    opacity: 0;
    transform: translate(-50%, -3px);
    transition: opacity 140ms ease, transform 140ms ease;
  }

  .wheel-open .wheel-control {
    opacity: 1;
    pointer-events: auto;
    transform: translate(calc(-50% + var(--node-x)), var(--node-y)) scale(1);
  }

  .wheel-control:hover,
  .wheel-control.selected {
    color: var(--signal);
  }

  .wheel-control:hover span,
  .wheel-control:focus-visible span,
  .wheel-control.selected span {
    opacity: 1;
    transform: translate(-50%, 0);
  }

  .tool-surface {
    position: absolute;
    top: 186px;
    left: 50%;
    display: grid;
    width: 448px;
    height: 346px;
    grid-template-rows: 66px 1fr;
    overflow: hidden;
    border-radius: 34px;
    opacity: 0;
    pointer-events: none;
    transform: translateX(-50%) translateY(-20px) scale(0.92);
    transition:
      opacity 170ms ease,
      transform 260ms cubic-bezier(0.22, 1, 0.36, 1),
      top 260ms cubic-bezier(0.22, 1, 0.36, 1),
      width 260ms cubic-bezier(0.22, 1, 0.36, 1),
      height 260ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .panel-open .tool-surface {
    opacity: 1;
    pointer-events: auto;
    transform: translateX(-50%) translateY(0) scale(1);
  }

  .agent-open .tool-surface {
    top: 156px;
    width: 640px;
    height: 424px;
  }

  .opens-up .pill-control {
    top: calc(100% - 82px);
  }

  .opens-up .tool-surface {
    top: 28px;
  }

  .opens-up.agent-open .tool-surface {
    top: 40px;
  }

  .surface-header {
    display: flex;
    padding: 16px 20px 11px;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--soft-line);
  }

  .surface-identity {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .surface-identity > :global(svg) {
    color: var(--muted);
  }

  .surface-identity div {
    display: flex;
    flex-direction: column;
  }

  .surface-identity strong {
    font-size: 13px;
    font-weight: 680;
  }

  .surface-identity span {
    margin-top: 2px;
    color: var(--muted);
    font-size: 10px;
  }

  .close-control {
    display: grid;
    width: 30px;
    height: 30px;
    padding: 0;
    place-items: center;
    border: 1px solid var(--soft-line);
    border-radius: 50%;
    color: var(--muted);
    background: color-mix(in oklch, white 50%, transparent);
    cursor: pointer;
  }

  .close-control span,
  .close-control span::after {
    width: 10px;
    height: 1px;
    background: currentColor;
    content: "";
    transform: rotate(45deg);
  }

  .close-control span::after {
    display: block;
    transform: rotate(90deg);
  }

  .surface-body {
    min-height: 0;
    padding: 16px 20px 20px;
    overflow: auto;
  }

  .agent-open .surface-body {
    padding: 0;
    overflow: hidden;
  }

  .hero-action {
    display: grid;
    padding: 10px 0 18px;
    place-items: center;
    border-bottom: 1px solid var(--soft-line);
  }

  .hero-action > button,
  .dictation-state > button {
    display: flex;
    min-height: 38px;
    padding: 0 16px;
    gap: 8px;
    align-items: center;
    border: 0;
    border-radius: 19px;
    color: white;
    background: var(--ink);
    font: inherit;
    font-size: 12px;
    font-weight: 650;
    cursor: pointer;
  }

  .hero-action > button.running {
    background: var(--record);
  }

  .record-mark {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--record);
  }

  .running .record-mark {
    border-radius: 2px;
    background: white;
  }

  .hero-action p,
  .dictation-state p,
  .surface-note,
  .empty-state p {
    margin: 9px 0 0;
    color: var(--muted);
    font-size: 10px;
  }

  .section-kicker {
    color: var(--muted);
    font-size: 8px;
    font-weight: 720;
    letter-spacing: 0.12em;
  }

  .recent-list {
    padding-top: 14px;
  }

  .recent-list article {
    display: grid;
    grid-template-columns: 46px 1fr auto;
    padding: 11px 0;
    align-items: center;
    border-bottom: 1px solid var(--soft-line);
  }

  .recent-list article > span,
  .recent-list article > small,
  .clip-list small,
  .snippet-grid span,
  .session small {
    color: var(--muted);
    font-size: 9px;
  }

  .recent-list article > strong {
    font-size: 11px;
    font-weight: 620;
  }

  .dictation-state {
    display: flex;
    height: 100%;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }

  .voice-orb {
    display: flex;
    width: 68px;
    height: 68px;
    margin-bottom: 14px;
    gap: 4px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    color: var(--muted);
    background: var(--pearl-deep);
  }

  .voice-orb i {
    width: 2px;
    height: 10px;
    border-radius: 2px;
    background: currentColor;
  }

  .voice-orb i:nth-child(2), .voice-orb i:nth-child(4) { height: 19px; }
  .voice-orb i:nth-child(3) { height: 27px; }
  .voice-orb.running { color: var(--signal); }
  .voice-orb.running i { animation: voice 900ms ease-in-out infinite alternate; }
  .voice-orb.running i:nth-child(2) { animation-delay: -180ms; }
  .voice-orb.running i:nth-child(3) { animation-delay: -360ms; }
  .voice-orb.running i:nth-child(4) { animation-delay: -540ms; }

  .dictation-state > strong { font-size: 16px; letter-spacing: -0.02em; }
  .dictation-state > button { margin-top: 18px; background: var(--signal); }

  .search-field {
    display: grid;
    height: 38px;
    padding: 0 12px;
    grid-template-columns: 18px 1fr auto;
    gap: 8px;
    align-items: center;
    border: 1px solid var(--soft-line);
    border-radius: 12px;
    color: var(--muted);
    background: color-mix(in oklch, white 42%, transparent);
    font-size: 10px;
  }

  kbd {
    padding: 2px 5px;
    border: 1px solid var(--line);
    border-radius: 5px;
    color: var(--muted);
    background: color-mix(in oklch, white 60%, transparent);
    font: 9px var(--rb-mono, monospace);
  }

  .clip-list { padding-top: 9px; }
  .clip-list button {
    display: grid;
    width: 100%;
    padding: 10px 4px;
    grid-template-columns: 34px 1fr;
    gap: 10px;
    align-items: center;
    border: 0;
    border-bottom: 1px solid var(--soft-line);
    color: inherit;
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .clip-list button:hover { background: color-mix(in oklch, var(--signal) 5%, transparent); }
  .clip-list button > span:last-child { display: flex; min-width: 0; flex-direction: column; }
  .clip-list strong { overflow: hidden; font-size: 10px; font-weight: 620; text-overflow: ellipsis; white-space: nowrap; }
  .clip-type { display: grid; width: 30px; height: 30px; place-items: center; border-radius: 9px; color: var(--muted); background: var(--pearl-deep); font-size: 7px; font-weight: 720; }
  .clip-type.image { color: var(--signal); }

  .snippet-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .snippet-grid button {
    display: flex;
    min-height: 102px;
    padding: 13px;
    flex-direction: column;
    align-items: flex-start;
    border: 1px solid var(--soft-line);
    border-radius: 15px;
    color: inherit;
    background: color-mix(in oklch, white 38%, transparent);
    text-align: left;
    cursor: pointer;
  }
  .snippet-grid small { color: var(--signal); font-size: 7px; font-weight: 740; letter-spacing: 0.1em; }
  .snippet-grid strong { margin: 9px 0 auto; font-size: 10px; font-weight: 600; line-height: 1.4; }
  .snippet-grid .new-snippet { min-height: 70px; grid-column: 1 / -1; align-items: center; justify-content: center; border-style: dashed; color: var(--muted); }
  .snippet-grid .new-snippet strong { margin: 0; font-size: 18px; font-weight: 400; }

  .capture-options { display: grid; padding-top: 22px; grid-template-columns: 1fr 1fr; gap: 12px; }
  .capture-options button { display: flex; min-height: 142px; padding: 18px; flex-direction: column; align-items: center; justify-content: center; border: 1px solid var(--soft-line); border-radius: 18px; color: inherit; background: color-mix(in oklch, white 38%, transparent); cursor: pointer; }
  .capture-options strong { margin-top: 13px; font-size: 11px; }
  .capture-options small { margin-top: 4px; color: var(--muted); font-size: 9px; }
  .capture-icon { position: relative; display: block; width: 36px; height: 28px; border: 1.5px solid var(--muted); border-radius: 5px; }
  .capture-icon.region { border-style: dashed; }
  .capture-icon.screen::after { position: absolute; bottom: -7px; left: 12px; width: 12px; height: 1px; background: var(--muted); box-shadow: 0 -3px 0 -0.2px var(--muted); content: ""; }
  .surface-note { text-align: center; }

  .agents-layout {
    display: grid;
    height: 100%;
    grid-template-columns: 172px 1fr;
    overflow: hidden;
    border: 1px solid var(--soft-line);
    border-radius: 18px;
  }
  .agents-layout aside { display: flex; padding: 12px; gap: 7px; flex-direction: column; border-right: 1px solid var(--soft-line); background: color-mix(in oklch, var(--pearl-deep) 60%, transparent); }
  .new-chat { height: 34px; margin-bottom: 8px; border: 0; border-radius: 10px; color: white; background: var(--ink); font: inherit; font-size: 9px; font-weight: 650; cursor: pointer; }
  .session { display: flex; padding: 9px; flex-direction: column; align-items: flex-start; border: 0; border-radius: 10px; color: inherit; background: transparent; text-align: left; cursor: pointer; }
  .session strong { font-size: 9px; font-weight: 620; }
  .session small { margin-top: 4px; }
  .active-session { background: color-mix(in oklch, white 65%, transparent); }
  .conversation { display: grid; min-width: 0; padding: 0 14px 12px; grid-template-rows: 46px 1fr 42px; }
  .conversation > header { display: flex; gap: 8px; align-items: center; border-bottom: 1px solid var(--soft-line); }
  .conversation > header > span:nth-child(2) { display: flex; flex-direction: column; }
  .conversation > header strong { font-size: 10px; }
  .conversation > header small { color: var(--muted); font-size: 8px; }
  .conversation > header i { width: 6px; height: 6px; margin-left: auto; border-radius: 50%; background: oklch(0.66 0.16 145); }
  .agent-avatar { display: grid; width: 26px; height: 26px; place-items: center; border-radius: 8px; color: white; background: var(--signal); font-size: 10px; font-weight: 700; }
  .messages { display: flex; padding: 18px 0; gap: 14px; flex-direction: column; justify-content: flex-end; }
  .messages p { margin: 0; }
  .user-message { max-width: 72%; padding: 9px 11px; align-self: flex-end; border-radius: 13px 13px 4px 13px; color: white; background: var(--ink); font-size: 10px; line-height: 1.45; }
  .agent-message { display: flex; max-width: 82%; flex-direction: column; font-size: 10px; line-height: 1.5; }
  .agent-message small { margin-top: 7px; padding: 6px 8px; border: 1px solid var(--soft-line); border-radius: 8px; color: var(--muted); background: color-mix(in oklch, white 38%, transparent); font: 8px var(--rb-mono, monospace); }
  .composer { display: flex; padding: 0 11px; align-items: center; justify-content: space-between; border: 1px solid var(--line); border-radius: 12px; color: var(--muted); background: color-mix(in oklch, white 50%, transparent); font-size: 9px; }

  .empty-state { display: grid; height: 100%; place-items: center; align-content: center; }

  @keyframes quiet-pulse { to { opacity: 0; transform: scale(1.55); } }
  @keyframes voice { to { transform: scaleY(0.48); } }

  @media (max-width: 680px) {
    .floating-experience { transform: scale(0.86); }
  }

  @media (prefers-reduced-motion: reduce) {
    .wheel-control, .tool-surface, .pill-glyph i { transition-duration: 100ms; }
    .status-pulse, .voice-orb.running i { animation: none; }
  }
</style>
