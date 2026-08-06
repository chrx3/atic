<script lang="ts">
  /**
   * Autocomplete de slash commands del agente.
   *
   * El catálogo viene del handshake (`name`, `description`, `argumentHint`):
   * acá solo se lista y se elige. Abre hacia arriba, encima del composer.
   */
  import type { SlashCommand } from "$lib/types";

  let {
    commands,
    activeIndex = 0,
    emptyHint = "Sin coincidencias",
    onPick,
    onHover,
  }: {
    /** Ya filtrados por el padre. */
    commands: SlashCommand[];
    activeIndex?: number;
    emptyHint?: string;
    onPick: (cmd: SlashCommand) => void;
    onHover?: (index: number) => void;
  } = $props();

  let rootEl = $state<HTMLDivElement | null>(null);

  // Teclado: el índice activo puede salir del viewport del listbox.
  $effect(() => {
    if (!rootEl || commands.length === 0) return;
    const selected = rootEl.querySelector<HTMLElement>(
      `[role="option"][data-idx="${activeIndex}"]`,
    );
    selected?.scrollIntoView({ block: "nearest" });
  });
</script>

<div class="slash" role="listbox" aria-label="Comandos" bind:this={rootEl}>
  {#if commands.length > 0}
    <ul class="slash-list">
      {#each commands as cmd, i (cmd.name)}
        <li>
          <button
            type="button"
            role="option"
            class="slash-opt"
            class:is-on={i === activeIndex}
            aria-selected={i === activeIndex}
            data-idx={i}
            onclick={() => onPick(cmd)}
            onpointerenter={() => onHover?.(i)}
          >
            <span class="slash-name">/{cmd.name}</span>
            {#if cmd.argumentHint}
              <span class="slash-hint">{cmd.argumentHint}</span>
            {/if}
            {#if cmd.description}
              <span class="slash-desc">{cmd.description}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="slash-empty">{emptyHint}</p>
  {/if}
</div>

<style>
  .slash {
    position: absolute;
    right: 0;
    bottom: calc(100% + 0.35rem);
    left: 0;
    z-index: 12;
    max-height: 14rem;
    overflow: auto;
    border: 1px solid var(--rb-border);
    border-radius: 12px;
    background: var(--rb-surface);
    box-shadow: 0 10px 28px color-mix(in srgb, var(--rb-text) 16%, transparent);
  }

  .slash-list {
    margin: 0;
    padding: 0.2rem;
    list-style: none;
  }

  .slash-empty {
    margin: 0;
    padding: 0.5rem 0.6rem;
    color: var(--rb-muted);
    font-size: 0.7rem;
    line-height: 1.35;
  }

  .slash-opt {
    display: grid;
    width: 100%;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
    column-gap: 0.35rem;
    row-gap: 0.05rem;
    align-items: baseline;
    border: 0;
    border-radius: 9px;
    padding: 0.28rem 0.4rem;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--duration-quick, 120ms) var(--ease-smooth-out, ease);
  }

  .slash-opt.is-on,
  .slash-opt:hover {
    background: color-mix(
      in srgb,
      var(--accent, #da7756) 10%,
      var(--rb-surface-2, transparent)
    );
  }

  .slash-name {
    grid-column: 1;
    grid-row: 1;
    color: var(--accent, #da7756);
    font-size: 0.75rem;
    font-weight: 650;
  }

  .slash-hint {
    grid-column: 2;
    grid-row: 1;
    color: var(--rb-faint, var(--rb-muted));
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.64rem;
    opacity: 0.85;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .slash-desc {
    grid-column: 1 / -1;
    grid-row: 2;
    color: var(--rb-muted);
    font-size: 0.66rem;
    line-height: 1.3;
    text-wrap: pretty;
  }
</style>
