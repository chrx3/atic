<script lang="ts">
  /**
   * Ctrl+Shift+K: buscar un espacio guardado y abrirlo sin ir a la lista.
   * Flechas para elegir, Enter para abrir, Esc para cerrar.
   */
  import { onMount, tick } from "svelte";
  import { fly, fade } from "svelte/transition";
  import { backOut } from "svelte/easing";
  import Icon from "$ui/Icon.svelte";
  import { History } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";
  import { prefersReducedMotion } from "$lib/motion";
  import type { SavedSpace } from "./agentBoard";

  let {
    spaces,
    current = null,
    onOpen,
    onClose,
  }: {
    spaces: SavedSpace[];
    current?: string | null;
    onOpen: (space: SavedSpace) => void;
    onClose: () => void;
  } = $props();

  let query = $state("");
  let index = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  const reduced = prefersReducedMotion();

  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? spaces.filter((s) => s.name.toLowerCase().includes(q)) : spaces;
  });

  $effect(() => {
    void query;
    index = 0;
  });

  onMount(async () => {
    await tick();
    input?.focus();
  });

  function onKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (matches.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      index = (index + step + matches.length) % matches.length;
    } else if (event.key === "Enter") {
      event.preventDefault();
      const space = matches[index];
      if (space) onOpen(space);
    }
  }

  function when(ms: number): string {
    return new Date(ms).toLocaleString(undefined, {
      day: "numeric",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    });
  }
</script>

<div
  class="scrim"
  role="presentation"
  transition:fade={{ duration: reduced ? 0 : 120 }}
  onpointerdown={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div
    class="switcher"
    role="dialog"
    aria-label={t("page.agents.board.spaceSwitcher")}
    in:fly={{ y: -12, duration: reduced ? 0 : 280, easing: backOut }}
    out:fly={{ y: -6, duration: reduced ? 0 : 120 }}
  >
    <input
      bind:this={input}
      bind:value={query}
      class="search"
      placeholder={t("page.agents.board.spaceSwitcher")}
      aria-label={t("page.agents.board.spaceSwitcher")}
      onkeydown={onKey}
    />
    {#if matches.length === 0}
      <p class="empty">{t("page.agents.board.spaceSwitcherEmpty")}</p>
    {:else}
      <ul class="list" role="listbox">
        {#each matches as space, i (space.name)}
          <li role="option" aria-selected={i === index}>
            <button
              type="button"
              class="item"
              class:is-selected={i === index}
              onpointerenter={() => (index = i)}
              onclick={() => onOpen(space)}
            >
              <Icon icon={History} size={14} />
              <span class="name">{space.name}</span>
              {#if space.name === current}<span class="now">●</span>{/if}
              <span class="meta">{space.consoles.length} · {when(space.savedAt)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: start center;
    padding-top: 14vh;
    background: color-mix(in sRGB, black 28%, transparent);
  }

  .switcher {
    display: flex;
    flex-direction: column;
    width: min(520px, calc(100% - 32px));
    max-height: 60vh;
    overflow: hidden;
    border-radius: 14px;
    background: color-mix(in sRGB, var(--rb-surface) 94%, transparent);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 12%, transparent),
      0 30px 70px -20px rgb(0 0 0 / 65%);
    backdrop-filter: blur(18px) saturate(1.2);
  }

  .search {
    border: 0;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-text) 10%, transparent);
    outline: 0;
    padding: 14px 16px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 14px;
  }

  .list {
    margin: 0;
    padding: 6px;
    overflow-y: auto;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    border-radius: 8px;
    padding: 8px 10px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--duration-quick) ease;
  }

  .item.is-selected {
    background: color-mix(in sRGB, var(--accent) 18%, transparent);
    color: var(--rb-text);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--rb-text);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .now {
    color: var(--accent);
    font-size: 9px;
  }

  .meta {
    flex-shrink: 0;
    color: var(--rb-faint, var(--rb-muted));
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .empty {
    margin: 0;
    padding: 16px;
    color: var(--rb-muted);
    font-size: 12.5px;
  }
</style>
