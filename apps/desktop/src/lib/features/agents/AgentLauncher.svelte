<script lang="ts">
  /** Lanzador compacto y shell persistente de las consolas locales. */
  import ConsolePanel from "./ConsolePanel.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Folder, SquareTerminal, X } from "$lib/icons";

  type LauncherView = "setup" | "console";

  let {
    onHeaderPointerDown,
    onClose,
    onViewChange,
    onBrowserChange,
  }: {
    onHeaderPointerDown?: (e: PointerEvent) => void;
    onClose?: () => void;
    onViewChange?: (view: LauncherView) => void;
    onBrowserChange?: (open: boolean) => void;
  } = $props();

  type AgentDef = { cli: string; name: string };

  const AGENTS: AgentDef[] = [
    { cli: "claude", name: "Claude Code" },
    { cli: "opencode", name: "OpenCode" },
    { cli: "codex", name: "Codex" },
    { cli: "cursor-agent", name: "Cursor" },
  ];

  /** El tope duro lo pone Rust (MAX_CONSOLES). */
  const MAX_INSTANCES = 6;

  let selected = $state<string>(AGENTS[0].cli);
  let count = $state(1);
  let cwd = $state("");
  let browsing = $state(false);
  let view = $state<LauncherView>("setup");
  let hasConsole = $state(false);

  const chosen = $derived(AGENTS.find((agent) => agent.cli === selected) ?? AGENTS[0]);
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

  function initials(name: string): string {
    return name
      .split(/\s+/)
      .map((part) => part[0])
      .join("")
      .slice(0, 2)
      .toUpperCase();
  }

  function showView(next: LauncherView) {
    view = next;
    onViewChange?.(next);
  }

  function launch() {
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
          aria-label="Cerrar"
          title="Cerrar"
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
            role="radio"
            aria-checked={selected === agent.cli}
            title={agent.name}
            onclick={() => (selected = agent.cli)}
          >
            <span class="agent-mark">{initials(agent.name)}</span>
            <span class="agent-name">{agent.name}</span>
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
            aria-label="Cerrar sesiones activas"
            title="Cerrar sesiones activas"
            onclick={resetSessions}
          >
            <Icon icon={X} size={13} />
          </button>
        {/if}

        <button type="button" class="launch" onclick={launch}>
          {#if hasConsole}<Icon icon={SquareTerminal} size={14} />{/if}
          <span>{launchLabel}</span>
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
    --agent-accent: var(--rb-record);

    position: relative;
    display: flex;
    min-height: 0;
    flex: 1;
    overflow: hidden;
    border-radius: inherit;
    background: var(--rb-surface);
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
    transform: translateY(0);
    transition:
      opacity 160ms ease,
      transform 160ms ease,
      visibility 160ms ease;
  }

  .is-hidden {
    visibility: hidden;
    pointer-events: none;
    opacity: 0;
    transform: translateY(4px);
  }

  .drag-rail {
    display: flex;
    min-height: 2rem;
    flex: 0 0 2rem;
    align-items: center;
    justify-content: flex-end;
    padding: 0.2rem 0.42rem 0.15rem 0.7rem;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-border) 72%, transparent);
    background: var(--rb-surface);
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
    width: 1.6rem;
    height: 1.6rem;
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

  .agent-picker {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    overflow: hidden;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 82%, transparent);
    border-radius: 0.72rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 54%, transparent);
  }

  .agent-option {
    display: flex;
    min-width: 0;
    min-height: 3rem;
    align-items: center;
    gap: 0.5rem;
    border: 0;
    border-right: 1px solid color-mix(in sRGB, var(--rb-border) 76%, transparent);
    padding: 0.42rem 0.55rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color 140ms ease,
      box-shadow 140ms ease;
  }

  .agent-option:last-child {
    border-right: 0;
  }

  .agent-option:hover {
    background: color-mix(in sRGB, var(--rb-text) 4%, transparent);
  }

  .agent-option.is-on {
    position: relative;
    z-index: 1;
    background: color-mix(in sRGB, var(--agent-accent) 10%, var(--rb-surface));
    box-shadow: inset 0 0 0 1px var(--agent-accent);
  }

  .agent-mark {
    display: grid;
    width: 1.75rem;
    height: 1.75rem;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 0.48rem;
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-muted);
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.6rem;
    font-weight: 760;
  }

  .agent-option.is-on .agent-mark {
    background: color-mix(in sRGB, var(--agent-accent) 17%, transparent);
    color: var(--agent-accent);
  }

  .agent-name {
    min-width: 0;
    overflow: hidden;
    font-size: 0.72rem;
    font-weight: 660;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    border: 1px solid color-mix(in sRGB, var(--rb-border) 84%, transparent);
    border-radius: 0.62rem;
    font: inherit;
  }

  .folder {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.48rem;
    padding: 0.4rem 0.62rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 55%, transparent);
    color: var(--rb-muted);
    text-align: left;
    cursor: pointer;
  }

  .folder:hover {
    border-color: color-mix(in sRGB, var(--agent-accent) 42%, var(--rb-border));
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
    grid-template-columns: 2rem minmax(4.7rem, auto) 2rem;
    align-items: stretch;
    overflow: hidden;
    background: transparent;
  }

  .stepper button {
    border: 0;
    background: transparent;
    color: var(--rb-text);
    font-size: 0.95rem;
    cursor: pointer;
  }

  .stepper button:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--agent-accent) 9%, transparent);
  }

  .stepper button:disabled {
    color: var(--rb-faint);
    cursor: default;
    opacity: 0.45;
  }

  .count {
    display: grid;
    place-items: center;
    color: var(--rb-text);
    font-size: 0.68rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .reset {
    width: 2.45rem;
    min-height: 2.45rem;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 84%, transparent);
    border-radius: 0.62rem;
  }

  .launch {
    display: inline-flex;
    min-width: 10rem;
    align-items: center;
    justify-content: center;
    gap: 0.42rem;
    padding: 0.42rem 0.75rem;
    border-color: var(--agent-accent);
    background: var(--agent-accent);
    color: var(--rb-on-accent);
    font-size: 0.7rem;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color 140ms ease,
      transform 140ms ease;
  }

  .launch:hover {
    background: color-mix(in sRGB, var(--agent-accent) 87%, var(--rb-text));
    transform: translateY(-1px);
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

    .agent-picker {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .agent-option {
      min-height: 2.25rem;
      padding: 0.25rem 0.42rem;
    }

    .agent-option:nth-child(2) {
      border-right: 0;
    }

    .agent-option:nth-child(-n + 2) {
      border-bottom: 1px solid color-mix(in sRGB, var(--rb-border) 76%, transparent);
    }

    .agent-mark {
      width: 1.45rem;
      height: 1.45rem;
      border-radius: 0.38rem;
      font-size: 0.54rem;
    }

    .launch-row {
      grid-template-columns: minmax(0, 1fr) auto auto;
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

    .agent-name,
    .folder > span:not(.chevron),
    .launch {
      font-size: 0.66rem;
    }

    .stepper {
      grid-template-columns: 1.75rem minmax(4.3rem, auto) 1.75rem;
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
    .launch {
      transition: none;
    }
  }
</style>
