<script lang="ts">
  /** Lanzador compacto y shell persistente de las consolas locales. */
  import ConsolePanel from "./ConsolePanel.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import AgentLogo from "./AgentLogo.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Folder, SquareTerminal, X } from "$lib/icons";
  import { onMount } from "svelte";
  import { AGENTS_REVEAL_CONSOLE, cliOnPath } from "$ipc/agents";
  import { AGENTS } from "./agentCatalog";

  type LauncherView = "setup" | "console";

  let {
    onHeaderPointerDown,
    onClose,
    onViewChange,
    onBrowserChange,
    onToggleMaximize,
    onToggleMinimize,
    maximized = false,
    minimized = false,
    shown = false,
  }: {
    onHeaderPointerDown?: (e: PointerEvent) => void;
    onClose?: () => void;
    onViewChange?: (view: LauncherView) => void;
    onBrowserChange?: (open: boolean) => void;
    onToggleMaximize?: () => void;
    onToggleMinimize?: () => void;
    maximized?: boolean;
    minimized?: boolean;
    /** El float está a la vista: si hay consolas vivas, mostrarlas. */
    shown?: boolean;
  } = $props();

  /** El tope duro lo pone Rust (MAX_CONSOLES). */
  const MAX_INSTANCES = 6;

  let selected = $state<string>(AGENTS[0].cli);
  let count = $state(1);
  let cwd = $state("");
  let browsing = $state(false);
  let view = $state<LauncherView>("setup");
  let hasConsole = $state(false);
  let onPath = $state<Record<string, boolean>>({});
  let pathReady = $state(false);

  const chosen = $derived(AGENTS.find((agent) => agent.cli === selected) ?? AGENTS[0]);
  const missingCli = $derived(pathReady && onPath[selected] === false);
  const seeds = $derived(
    Array.from({ length: Math.max(1, Math.min(count, MAX_INSTANCES)) }, () => ({
      kind: "local" as const,
      label: chosen.name,
      command: chosen.cli,
    })),
  );
  const launchLabel = $derived(
    hasConsole ? "Volver a consolas" : `Abrir ${chosen.name}`,
  );

  function showView(next: LauncherView) {
    view = next;
    onViewChange?.(next);
  }

  function revealLiveConsole() {
    if (hasConsole) showView("console");
  }

  let wasShown = false;
  $effect(() => {
    const justOpened = shown && !wasShown;
    wasShown = shown;
    if (justOpened) revealLiveConsole();
  });

  function launch() {
    if (missingCli) return;
    if (!hasConsole) hasConsole = true;
    showView("console");
  }

  function backToSetup() {
    showView("setup");
  }

  function resetSessions() {
    // Al desmontar ConsolePanel su onDestroy cierra todas las PTYs.
    hasConsole = false;
    showView("setup");
  }

  function setBrowsing(open: boolean) {
    browsing = open;
    onBrowserChange?.(open);
  }

  onMount(() => {
    void Promise.all(
      AGENTS.map(async (agent) => {
        try {
          return [agent.cli, await cliOnPath(agent.cli)] as const;
        } catch {
          return [agent.cli, true] as const;
        }
      }),
    ).then((rows) => {
      onPath = Object.fromEntries(rows);
      pathReady = true;
    });
    const onReveal = () => revealLiveConsole();
    window.addEventListener(AGENTS_REVEAL_CONSOLE, onReveal);
    return () => window.removeEventListener(AGENTS_REVEAL_CONSOLE, onReveal);
  });
</script>

<div class="agent-views">
  <section
    class="launcher-view"
    class:is-hidden={view !== "setup"}
    aria-hidden={view !== "setup" ? "true" : undefined}
    inert={view !== "setup"}
    aria-label="Abrir agentes"
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class="drag-rail"
      aria-label="Mover ventana"
      onpointerdown={(event) => {
        if (onHeaderPointerDown && !(event.target as HTMLElement).closest("button")) {
          onHeaderPointerDown(event);
        }
      }}
    >
      {#if hasConsole}
        <span class="live-status" role="status">
          <span class="live-dot" aria-hidden="true"></span>
          Consolas activas
        </span>
      {/if}
      {#if onClose}
        <button
          type="button"
          class="close"
          aria-label="Esconder ventana"
          title="Esconder ventana. Las consolas siguen corriendo."
          onclick={onClose}
        >
          <Icon icon={X} size={13} />
        </button>
      {/if}
    </header>

    <div class="setup">
      <div class="agent-picker" role="radiogroup" aria-label="Agente">
        {#each AGENTS as agent (agent.cli)}
          <button
            type="button"
            class="agent-option"
            class:is-on={selected === agent.cli}
            class:is-missing={pathReady && onPath[agent.cli] === false}
            role="radio"
            aria-checked={selected === agent.cli}
            aria-label={
              pathReady && onPath[agent.cli] === false
                ? `${agent.name} (no está en el PATH)`
                : agent.name
            }
            title={
              pathReady && onPath[agent.cli] === false
                ? `${agent.name} no está en el PATH`
                : agent.name
            }
            onclick={() => (selected = agent.cli)}
          >
            <span class="agent-logo"><AgentLogo agent={agent.cli} size={22} /></span>
          </button>
        {/each}
      </div>

      <div class="launch-row">
        <button
          type="button"
          class="folder"
          title={cwd || "Carpeta de inicio del usuario"}
          onclick={() => setBrowsing(true)}
        >
          <Icon icon={Folder} size={15} />
          <span>{cwd.trim() || "Carpeta de inicio"}</span>
          <span class="chevron" aria-hidden="true">›</span>
        </button>

        <div class="stepper" role="group" aria-label="Cantidad de consolas">
          <button
            type="button"
            aria-label="Menos consolas"
            disabled={count <= 1}
            onclick={() => (count = Math.max(1, count - 1))}
          >
            −
          </button>
          <span class="count">{count} {count === 1 ? "consola" : "consolas"}</span>
          <button
            type="button"
            aria-label="Más consolas"
            disabled={count >= MAX_INSTANCES}
            onclick={() => (count = Math.min(MAX_INSTANCES, count + 1))}
          >
            +
          </button>
        </div>

        {#if hasConsole}
          <button
            type="button"
            class="reset"
            aria-label="Cerrar y matar las consolas"
            title="Cierra las consolas y mata los procesos"
            onclick={resetSessions}
          >
            <Icon icon={X} size={13} />
          </button>
        {/if}

          <button type="button" class="launch" disabled={missingCli} onclick={launch}>
            {#if hasConsole}<Icon icon={SquareTerminal} size={14} />{/if}
            <span>{missingCli ? `${chosen.name} no está instalado` : launchLabel}</span>
          <span class="arrow" aria-hidden="true">→</span>
        </button>
      </div>
    </div>
  </section>

  {#if hasConsole}
    <div
      class="console-view"
      class:is-hidden={view !== "console"}
      aria-hidden={view !== "console" ? "true" : undefined}
      inert={view !== "console"}
    >
      <ConsolePanel
        initialTabs={seeds}
        localCwd={cwd}
        onBack={backToSetup}
        {onClose}
        {onToggleMaximize}
        {onToggleMinimize}
        {maximized}
        {minimized}
        onBarPointerDown={onHeaderPointerDown}
      />
    </div>
  {/if}
</div>

{#if browsing}
  <FolderBrowser
    initialPath={cwd}
    onPick={(path) => {
      cwd = path;
      setBrowsing(false);
    }}
    onClose={() => setBrowsing(false)}
  />
{/if}

<style>
  .agent-views {
    --agent-accent: var(--accent, var(--rb-accent, var(--rb-text)));

    position: relative;
    display: flex;
    min-height: 0;
    flex: 1;
    overflow: hidden;
    border-radius: inherit;
    background: transparent;
    container-name: agents-launcher;
    container-type: inline-size;
  }

  .launcher-view,
  .console-view {
    position: absolute;
    inset: 0;
    display: flex;
    min-height: 0;
    flex-direction: column;
    opacity: 1;
  }

  .launcher-view {
    z-index: 1;
    overflow: hidden;
    border-radius: inherit;
    background: var(--skin);
  }

  .console-view {
    z-index: 0;
  }

  .is-hidden {
    pointer-events: none;
  }

  .launcher-view.is-hidden {
    visibility: hidden;
    opacity: 0;
    z-index: 0;
  }

  /* No opacity 0, visibility:hidden ni transform: en WebView2 el canvas
     de xterm se congela y la consola queda en beige vacío. Se queda detrás
     del lanzador (fondo --skin) para que el renderer siga vivo. */

  .drag-rail {
    display: flex;
    min-height: 2rem;
    flex: 0 0 2rem;
    align-items: center;
    justify-content: flex-end;
    padding: 0.2rem 0.42rem 0.15rem 0.7rem;
    border-bottom: 0;
    background: transparent;
    cursor: move;
  }

  .live-status {
    display: inline-flex;
    min-width: 0;
    align-items: center;
    gap: 0.35rem;
    margin-right: auto;
    color: var(--rb-muted);
    font-size: 0.625rem;
    font-weight: 600;
  }

  .live-dot {
    width: 0.35rem;
    height: 0.35rem;
    flex: 0 0 auto;
    border-radius: 999px;
    background: var(--rb-ok);
  }

  .close,
  .reset {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    border: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .close {
    width: 2.25rem;
    height: 2.25rem;
    border-radius: 0.45rem;
  }

  .close:hover,
  .reset:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
  }

  .setup {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.65rem;
    padding: 0.75rem;
    overflow: auto;
  }

  /* Sin marco ni separadores: en las esquinas redondeadas del contenedor los
     bordes se perdían y ensuciaban. El único color es el acento del elegido;
     el resto se lee por hover. Las celdas se reparten TODO el ancho. */
  .agent-picker {
    display: grid;
    width: 100%;
    grid-template-columns: repeat(auto-fit, minmax(2.5rem, 1fr));
    gap: 0.35rem;
    align-self: stretch;
  }

  .agent-option {
    display: grid;
    min-height: 2.75rem;
    place-items: center;
    border: 0;
    border-radius: 0.6rem;
    padding: 0;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      box-shadow var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .agent-option:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .agent-option:active {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .agent-option.is-on {
    background: color-mix(in sRGB, var(--agent-accent) 12%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--agent-accent) 72%, transparent);
  }

  .agent-option.is-missing {
    opacity: 0.42;
  }

  .agent-option.is-missing.is-on {
    opacity: 0.72;
  }

  .agent-logo {
    display: grid;
    width: 1.4rem;
    height: 1.4rem;
    place-items: center;
    color: var(--rb-text);
    transition: transform var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .agent-option:hover .agent-logo,
  .agent-option.is-on .agent-logo {
    transform: scale(1.08);
  }

  .agent-option:active .agent-logo {
    transform: scale(0.96);
  }

  .launch-row {
    display: grid;
    grid-template-columns: minmax(9rem, 1fr) auto auto auto;
    gap: 0.55rem;
    align-items: stretch;
  }

  .folder,
  .stepper,
  .launch {
    min-height: 2.45rem;
    border: 0; /* mismo lenguaje que el selector de agentes: sin bordes, fondos suaves */
    border-radius: 0.62rem;
    font: inherit;
  }

  .folder {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.48rem;
    padding: 0.4rem 0.62rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 62%, transparent);
    color: var(--rb-muted);
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      color var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .folder:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .folder > span:not(.chevron) {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    font-size: 0.7rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chevron {
    color: var(--rb-faint);
    font-size: 1rem;
    line-height: 1;
  }

  .stepper {
    display: grid;
    grid-template-columns: 1.9rem max-content 1.9rem;
    flex: none;
    align-items: stretch;
    gap: 0.2rem;
    min-width: max-content;
    padding: 0.25rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 62%, transparent);
  }

  .stepper button {
    display: grid;
    width: 1.9rem;
    place-items: center;
    border: 0;
    border-radius: 0.42rem;
    background: transparent;
    color: var(--rb-text);
    font-size: 0.95rem;
    line-height: 1;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      transform var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .stepper button:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .stepper button:active:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);
    transform: scale(0.92);
  }

  .stepper button:disabled {
    color: var(--rb-faint);
    cursor: default;
    opacity: 0.4;
  }

  .count {
    display: grid;
    place-items: center;
    padding-inline: 0.5rem;
    color: var(--rb-text);
    font-size: 0.68rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .reset {
    width: 2.45rem;
    min-height: 2.45rem;
    border: 0;
    border-radius: 0.62rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 62%, transparent);
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      color var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .reset:hover {
    background: color-mix(in sRGB, var(--rb-record) 14%, transparent);
    color: var(--rb-record);
  }

  .launch {
    display: inline-flex;
    min-width: 10rem;
    align-items: center;
    justify-content: center;
    gap: 0.42rem;
    padding: 0.42rem 0.75rem;
    background: var(--agent-accent);
    color: var(--rb-on-accent);
    font-size: 0.7rem;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      transform var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .launch:disabled {
    cursor: not-allowed;
    opacity: 0.55;
    transform: none;
  }

  .launch:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--agent-accent) 87%, var(--rb-text));
    transform: translateY(-1px);
  }

  .launch:active:not(:disabled) {
    transform: translateY(0) scale(0.99);
  }

  .launch .arrow {
    margin-left: 0.12rem;
    font-size: 0.9rem;
  }

  button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  @container agents-launcher (width <= 35rem) {
    .setup {
      gap: 0.48rem;
      padding: 0.55rem;
    }

    .launch-row {
      grid-template-columns: max-content auto minmax(0, 1fr);
      gap: 0.4rem;
    }

    .folder {
      grid-column: 1 / -1;
    }

    .stepper {
      grid-column: 1;
    }

    .launch {
      grid-column: 3;
      min-width: 8.75rem;
    }

    .reset {
      grid-column: 2;
    }
  }

  @container agents-launcher (width <= 28rem) {
    .drag-rail {
      min-height: 1.8rem;
      flex-basis: 1.8rem;
    }

    .folder > span:not(.chevron),
    .launch {
      font-size: 0.66rem;
    }

    .stepper button {
      width: 1.65rem;
    }

    .launch {
      min-width: 7.8rem;
      padding-inline: 0.55rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .launcher-view,
    .console-view,
    .agent-option,
    .agent-logo,
    .folder,
    .stepper button,
    .reset,
    .launch {
      transition: none;
      transform: none;
    }
  }
</style>
