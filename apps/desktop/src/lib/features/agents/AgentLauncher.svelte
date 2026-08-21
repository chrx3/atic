<script lang="ts">
  /**
   * Lanzador de agentes y shell persistente del float.
   *
   * El lanzador y las consolas son dos vistas de la misma superficie. Volver
   * atrás no mata las PTYs: solo las deja detrás del configurador. Para crear
   * una tanda nueva hay una acción explícita que sí cierra esas sesiones.
   */
  import ConsolePanel from "./ConsolePanel.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Folder, SquareTerminal, X } from "$lib/icons";

  let {
    onHeaderPointerDown,
    onClose,
  }: {
    onHeaderPointerDown?: (e: PointerEvent) => void;
    onClose?: () => void;
  } = $props();

  type AgentDef = {
    cli: string;
    name: string;
    hint: string;
  };

  const AGENTS: AgentDef[] = [
    { cli: "claude", name: "Claude Code", hint: "claude" },
    { cli: "opencode", name: "OpenCode", hint: "opencode" },
    { cli: "codex", name: "Codex", hint: "codex" },
    { cli: "cursor-agent", name: "Cursor", hint: "cursor-agent" },
  ];

  /** El tope duro lo pone Rust (MAX_CONSOLES). */
  const MAX_INSTANCES = 6;

  let selected = $state<string>(AGENTS[0].cli);
  let count = $state(1);
  let cwd = $state("");
  let browsing = $state(false);
  let view = $state<"setup" | "console">("setup");
  let hasConsole = $state(false);

  const chosen = $derived(AGENTS.find((a) => a.cli === selected) ?? AGENTS[0]);
  const seeds = $derived(
    Array.from({ length: Math.max(1, Math.min(count, MAX_INSTANCES)) }, () => ({
      kind: "local" as const,
      label: chosen.name,
      command: chosen.cli,
    })),
  );

  function initials(name: string): string {
    return name
      .split(/\s+/)
      .map((part) => part[0])
      .join("")
      .slice(0, 2)
      .toUpperCase();
  }

  function launch() {
    // Una configuración existente vuelve a su consola viva; no crea otra
    // tanda accidentalmente ni deja procesos duplicados.
    if (hasConsole) {
      view = "console";
      return;
    }
    hasConsole = true;
    view = "console";
  }

  function backToSetup() {
    view = "setup";
  }

  function resetSessions() {
    // Al desmontar ConsolePanel su onDestroy cierra todas las PTYs.
    hasConsole = false;
    view = "setup";
  }
</script>

<div class="agent-views">
  <section
    class="launcher-view"
    class:is-hidden={view !== "setup"}
    aria-hidden={view !== "setup" ? "true" : undefined}
    inert={view !== "setup"}
    aria-label="Lanzador de agentes"
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class="head"
      onpointerdown={(e) => {
        if (onHeaderPointerDown && !(e.target as HTMLElement).closest("button"))
          onHeaderPointerDown(e);
      }}
    >
      <div class="brand">
        <span class="brand-mark"><Icon icon={SquareTerminal} size={14} /></span>
        <span class="brand-copy">
          <span class="title">Agentes</span>
          <span class="subtitle">Terminales locales</span>
        </span>
      </div>
      {#if hasConsole}
        <span class="live-badge"><span class="live-dot"></span>Sesiones activas</span>
      {/if}
      {#if onClose}
        <button
          type="button"
          class="close"
          aria-label="Cerrar"
          title="Cerrar"
          onclick={onClose}
        >
          <Icon icon={X} size={14} />
        </button>
      {/if}
    </header>

    <div class="launcher-body">
      <div class="intro">
        <p class="eyebrow">ATic · agentes CLI</p>
        <h1>Levanta tu equipo</h1>
        <p>
          Elige un agente, define cuántas consolas quieres y abre el proyecto en su
          carpeta de trabajo.
        </p>
      </div>

      {#if hasConsole}
        <p class="session-note" role="status">
          Las consolas siguen ejecutándose. Puedes volver a ellas cuando quieras.
        </p>
      {/if}

      <div class="setup-grid">
        <div class="agent-section">
          <div class="section-heading">
            <span class="section-label">Agente</span>
            <span class="section-meta">{chosen.hint}</span>
          </div>
          <div class="grid" role="radiogroup" aria-label="Agentes disponibles">
            {#each AGENTS as a (a.cli)}
              <button
                type="button"
                class="card"
                class:is-on={selected === a.cli}
                role="radio"
                aria-checked={selected === a.cli}
                onclick={() => (selected = a.cli)}
              >
                <span class="agent-mark">{initials(a.name)}</span>
                <span class="card-copy">
                  <span class="name">{a.name}</span>
                  <span class="cli">{a.hint}</span>
                </span>
                <span class="check" aria-hidden="true">✓</span>
              </button>
            {/each}
          </div>
        </div>

        <aside class="summary" aria-label="Configuración de la sesión">
          <div class="summary-top">
            <span class="section-label">Sesión nueva</span>
            <span class="summary-count">{count}</span>
          </div>

          <div class="count-row">
            <div>
              <strong>{count === 1 ? "Una consola" : `${count} consolas`}</strong>
              <span>del mismo agente</span>
            </div>
            <div class="stepper" role="group" aria-label="Cuántas instancias">
              <button
                type="button"
                aria-label="Menos instancias"
                disabled={count <= 1}
                onclick={() => (count = Math.max(1, count - 1))}
              >
                −
              </button>
              <span class="n">{count}</span>
              <button
                type="button"
                aria-label="Más instancias"
                disabled={count >= MAX_INSTANCES}
                onclick={() => (count = Math.min(MAX_INSTANCES, count + 1))}
              >
                +
              </button>
            </div>
          </div>

          <div class="folder-block">
            <span class="section-label">Carpeta de trabajo</span>
            <button
              type="button"
              class="pick"
              title={cwd || "Carpeta de inicio del usuario"}
              onclick={() => (browsing = true)}
            >
              <Icon icon={Folder} size={14} />
              <span class="pick-p">{cwd.trim() || "Carpeta de inicio"}</span>
              <span class="pick-arrow">›</span>
            </button>
          </div>

          <div class="summary-foot">
            <span class="summary-key">PTY local</span>
            <span class="summary-hint">Cada consola conserva su propia sesión</span>
          </div>
        </aside>
      </div>
    </div>

    <footer class="foot">
      {#if hasConsole}
        <button type="button" class="secondary" onclick={() => (view = "console")}>
          <Icon icon={SquareTerminal} size={13} />
          Volver a las consolas
        </button>
        <button type="button" class="reset" onclick={resetSessions}>
          Cerrar sesiones
        </button>
      {:else}
        <button type="button" class="go" onclick={launch}>
          Levantar {count}
          {count === 1 ? "consola" : "consolas"}
          <span>de {chosen.name}</span>
          <span class="go-arrow">→</span>
        </button>
      {/if}
    </footer>
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
    onPick={(p) => {
      cwd = p;
      browsing = false;
    }}
    onClose={() => (browsing = false)}
  />
{/if}

<style>
  .agent-views {
    --agent-accent: var(--rb-record);

    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: var(--rb-surface);
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
      opacity 180ms ease,
      transform 180ms ease,
      visibility 180ms ease;
  }

  .is-hidden {
    visibility: hidden;
    pointer-events: none;
    opacity: 0;
    transform: translateY(6px);
  }

  .head {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.65rem;
    min-height: 3.15rem;
    padding: 0.45rem 0.7rem;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-border) 75%, transparent);
    background: var(--rb-surface);
  }

  .brand {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 0.55rem;
  }

  .brand-mark {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border-radius: 0.6rem;
    background: color-mix(in sRGB, var(--agent-accent) 18%, transparent);
    color: var(--agent-accent);
  }

  .brand-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.08rem;
  }

  .title {
    color: var(--rb-text);
    font-size: 0.78rem;
    font-weight: 700;
  }

  .subtitle,
  .eyebrow,
  .section-label,
  .section-meta,
  .summary-foot,
  .summary-hint,
  .cli {
    font-size: 0.6rem;
  }

  .subtitle,
  .section-meta,
  .summary-hint {
    color: var(--rb-faint);
  }

  .live-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--rb-ok);
    font-size: 0.62rem;
    font-weight: 650;
  }

  .live-dot {
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 999px;
    background: var(--rb-ok);
  }

  .close {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border: 0;
    border-radius: 0.5rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .close:hover {
    background: color-mix(in sRGB, var(--rb-record) 12%, transparent);
    color: var(--rb-record);
  }

  .launcher-body {
    display: flex;
    flex: 1;
    min-height: 0;
    flex-direction: column;
    gap: 1.05rem;
    padding: 1.35rem 1.45rem;
    overflow-y: auto;
  }

  .intro {
    max-width: 34rem;
  }

  .eyebrow {
    margin: 0 0 0.38rem;
    color: var(--agent-accent);
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    color: var(--rb-text);
    font-size: 1.35rem;
    font-weight: 760;
    letter-spacing: -0.025em;
    text-wrap: balance;
  }

  .intro > p:last-child {
    max-width: 42rem;
    margin: 0.42rem 0 0;
    color: var(--rb-muted);
    font-size: 0.76rem;
    line-height: 1.45;
  }

  .session-note {
    margin: -0.2rem 0 0;
    color: var(--rb-ok);
    font-size: 0.68rem;
  }

  .setup-grid {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(15rem, 0.8fr);
    gap: 1rem;
    align-items: stretch;
  }

  .agent-section,
  .summary {
    min-width: 0;
  }

  .section-heading,
  .summary-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.45rem;
  }

  .section-label {
    color: var(--rb-text);
    font-weight: 700;
  }

  .section-meta {
    font-family: var(--rb-mono, ui-monospace, monospace);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.5rem;
  }

  .card {
    position: relative;
    display: flex;
    min-height: 4.35rem;
    align-items: center;
    gap: 0.65rem;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 82%, transparent);
    border-radius: 0.7rem;
    padding: 0.65rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 70%, transparent);
    color: var(--rb-text);
    text-align: left;
    cursor: pointer;
    transition:
      border-color 150ms ease,
      background-color 150ms ease,
      transform 150ms ease;
  }

  .card:hover {
    border-color: color-mix(in sRGB, var(--agent-accent) 42%, var(--rb-border));
    transform: translateY(-1px);
  }

  .card.is-on {
    border-color: color-mix(in sRGB, var(--agent-accent) 62%, transparent);
    background: color-mix(in sRGB, var(--agent-accent) 13%, var(--rb-surface));
  }

  .agent-mark {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 2.1rem;
    height: 2.1rem;
    border-radius: 0.55rem;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.62rem;
    font-weight: 760;
  }

  .card.is-on .agent-mark {
    background: color-mix(in sRGB, var(--agent-accent) 22%, transparent);
    color: var(--agent-accent);
  }

  .card-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.15rem;
  }

  .name {
    overflow: hidden;
    font-size: 0.74rem;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cli {
    overflow: hidden;
    color: var(--rb-faint);
    font-family: var(--rb-mono, ui-monospace, monospace);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .check {
    display: grid;
    place-items: center;
    width: 1.1rem;
    height: 1.1rem;
    margin-left: auto;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 80%, transparent);
    border-radius: 999px;
    color: transparent;
    font-size: 0.65rem;
  }

  .card.is-on .check {
    border-color: var(--agent-accent);
    background: var(--agent-accent);
    color: var(--rb-on-accent);
  }

  .summary {
    display: flex;
    flex-direction: column;
    border-left: 1px solid color-mix(in sRGB, var(--rb-border) 70%, transparent);
    padding: 0.1rem 0 0.1rem 1rem;
  }

  .summary-count {
    color: var(--agent-accent);
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 1rem;
    font-weight: 760;
    font-variant-numeric: tabular-nums;
  }

  .count-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    padding: 0.55rem 0;
  }

  .count-row strong,
  .count-row span {
    display: block;
  }

  .count-row strong {
    color: var(--rb-text);
    font-size: 0.72rem;
  }

  .count-row span {
    margin-top: 0.15rem;
    color: var(--rb-muted);
    font-size: 0.62rem;
  }

  .stepper {
    display: inline-flex;
    align-items: center;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 85%, transparent);
    border-radius: 0.55rem;
    overflow: hidden;
  }

  .stepper button {
    width: 2rem;
    height: 2rem;
    border: 0;
    background: transparent;
    color: var(--rb-text);
    font-size: 1rem;
    cursor: pointer;
  }

  .stepper button:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--agent-accent) 12%, transparent);
  }

  .stepper button:disabled {
    color: var(--rb-faint);
    cursor: default;
    opacity: 0.45;
  }

  .n {
    min-width: 1.7rem;
    color: var(--rb-text);
    font-size: 0.76rem;
    font-weight: 760;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }

  .folder-block {
    display: flex;
    flex-direction: column;
    gap: 0.42rem;
    margin-top: 0.55rem;
  }

  .pick {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.45rem;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 82%, transparent);
    border-radius: 0.55rem;
    padding: 0.48rem 0.55rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 72%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.68rem;
    text-align: left;
    cursor: pointer;
  }

  .pick:hover {
    border-color: color-mix(in sRGB, var(--agent-accent) 42%, transparent);
    color: var(--rb-text);
  }

  .pick-p {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pick-arrow {
    color: var(--rb-faint);
    font-size: 1.1rem;
    line-height: 0.8;
  }

  .summary-foot {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    margin-top: auto;
    padding-top: 0.8rem;
  }

  .summary-key {
    color: var(--rb-text);
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.58rem;
    font-weight: 650;
  }

  .foot {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: flex-end;
    gap: 0.55rem;
    padding: 0.7rem 1.45rem 1rem;
    border-top: 1px solid color-mix(in sRGB, var(--rb-border) 70%, transparent);
    background: var(--rb-surface);
  }

  .go,
  .secondary,
  .reset {
    display: inline-flex;
    min-height: 2.35rem;
    align-items: center;
    gap: 0.4rem;
    border-radius: 0.6rem;
    padding: 0.45rem 0.75rem;
    font: inherit;
    font-size: 0.7rem;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color 150ms ease,
      border-color 150ms ease,
      color 150ms ease,
      transform 150ms ease;
  }

  .go {
    border: 1px solid var(--agent-accent);
    background: var(--agent-accent);
    color: var(--rb-on-accent);
  }

  .go span:not(.go-arrow) {
    font-weight: 560;
    opacity: 0.84;
  }

  .go-arrow {
    margin-left: 0.25rem;
    font-size: 1rem;
  }

  .go:hover,
  .secondary:hover,
  .reset:hover {
    transform: translateY(-1px);
  }

  .go:hover {
    background: color-mix(in sRGB, var(--agent-accent) 86%, var(--rb-text));
  }

  .secondary,
  .reset {
    border: 1px solid color-mix(in sRGB, var(--rb-border) 88%, transparent);
    background: transparent;
    color: var(--rb-text);
  }

  .secondary:hover {
    border-color: color-mix(in sRGB, var(--agent-accent) 42%, transparent);
    background: color-mix(in sRGB, var(--agent-accent) 10%, transparent);
  }

  .reset {
    color: var(--rb-muted);
    font-size: 0.64rem;
    font-weight: 600;
  }

  .reset:hover {
    border-color: color-mix(in sRGB, var(--rb-record) 45%, transparent);
    color: var(--rb-record);
  }

  @media (width <= 42rem) {
    .launcher-body {
      padding-inline: 1rem;
    }

    .setup-grid {
      grid-template-columns: 1fr;
    }

    .summary {
      border-top: 1px solid color-mix(in sRGB, var(--rb-border) 70%, transparent);
      border-left: 0;
      padding: 0.9rem 0 0;
    }

    .summary-foot {
      margin-top: 0.25rem;
    }

    .foot {
      padding-inline: 1rem;
    }
  }

  @media (width <= 28rem) {
    .grid {
      grid-template-columns: 1fr;
    }

    .subtitle,
    .live-badge {
      display: none;
    }

    .foot {
      align-items: stretch;
      flex-direction: column;
    }

    .go,
    .secondary,
    .reset {
      justify-content: center;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .launcher-view,
    .console-view,
    .card,
    .go,
    .secondary,
    .reset {
      transition: none;
    }
  }
</style>
