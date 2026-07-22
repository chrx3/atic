<script lang="ts">
  import type { Snippet } from "svelte";
  import Titlebar from "$lib/Titlebar.svelte";
  import type { UiTheme } from "$lib/theme";
  import { TOOLS, toolById, type ToolId } from "$lib/tools";

  let {
    activeTool = $bindable("meetings"),
    theme = "system",
    onToggleTheme,
    onOpenSettings,
    children,
  }: {
    activeTool?: ToolId;
    theme?: UiTheme;
    onToggleTheme?: () => void;
    onOpenSettings?: () => void;
    children: Snippet;
  } = $props();

  const tool = $derived(toolById(activeTool));
</script>

<div class="atic-shell">
  <Titlebar {tool} {theme} {onToggleTheme} {onOpenSettings} />

  <div class="atic-shell-body">
    <nav class="atic-rail" aria-label="Herramientas">
      {#each TOOLS as item (item.id)}
        <button
          type="button"
          class="atic-rail-btn"
          class:active={activeTool === item.id}
          class:soon={item.comingSoon}
          title="{item.label} — {item.short}"
          aria-label={item.label}
          aria-current={activeTool === item.id ? "page" : undefined}
          onclick={() => (activeTool = item.id)}
        >
          <span class="atic-rail-icon" aria-hidden="true">
            {#if item.id === "meetings"}
              <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                <rect x="2" y="4" width="10" height="8" rx="1.5" stroke="currentColor" stroke-width="1.3" />
                <path d="M12 7.5l4-2v7l-4-2v-3Z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
              </svg>
            {:else if item.id === "dictation"}
              <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                <rect x="6.5" y="2.5" width="5" height="8" rx="2.5" stroke="currentColor" stroke-width="1.3" />
                <path d="M4.5 8.5a4.5 4.5 0 0 0 9 0M9 13v2.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
              </svg>
            {:else if item.id === "clipboard"}
              <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                <rect x="4.5" y="3.5" width="9" height="12" rx="1.5" stroke="currentColor" stroke-width="1.3" />
                <path d="M7 3.5h4v1.8H7V3.5Z" stroke="currentColor" stroke-width="1.2" />
                <path d="M7 8h4M7 10.5h4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
              </svg>
            {:else if item.id === "snippets"}
              <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                <path d="M4 4.5h10M4 8h7M4 11.5h9M4 15h6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
                <rect x="3" y="2.5" width="12" height="13" rx="1.5" stroke="currentColor" stroke-width="1.3" />
              </svg>
            {:else}
              <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                <rect x="2.5" y="3.5" width="13" height="10" rx="1.5" stroke="currentColor" stroke-width="1.3" />
                <circle cx="6.5" cy="7.5" r="1.2" fill="currentColor" />
                <path d="M2.8 12.2l3.6-3.2 2.2 1.8 3-3.4 3.6 4.8" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
              </svg>
            {/if}
          </span>
          <span class="atic-rail-label">{item.label}</span>
        </button>
      {/each}
    </nav>

    <div class="atic-shell-main">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .atic-shell {
    display: flex;
    height: 100dvh;
    flex-direction: column;
    overflow: hidden;
    color: var(--rb-text);
    background: var(--rb-bg0);
  }

  .atic-shell-body {
    display: flex;
    min-height: 0;
    flex: 1 1 auto;
  }

  .atic-rail {
    display: flex;
    width: 4.5rem;
    flex: 0 0 auto;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.55rem 0.4rem;
    border-right: 1px solid var(--rb-border);
    background: color-mix(in srgb, var(--rb-sidebar) 88%, var(--rb-surface));
  }

  .atic-rail-btn {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.2rem;
    width: 100%;
    border: 0;
    border-radius: 0.55rem;
    padding: 0.45rem 0.2rem 0.4rem;
    color: var(--rb-muted);
    background: transparent;
    cursor: pointer;
    transition:
      background 0.14s ease,
      color 0.14s ease;
  }

  .atic-rail-btn:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .atic-rail-btn.active {
    color: var(--rb-text);
    background: var(--rb-surface);
    box-shadow: 0 1px 0 var(--rb-border), 0 6px 16px rgba(34, 34, 29, 0.06);
  }

  .atic-rail-btn.soon:not(.active) {
    opacity: 0.72;
  }

  .atic-rail-btn:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .atic-rail-icon {
    display: inline-flex;
    width: 1.5rem;
    height: 1.5rem;
    align-items: center;
    justify-content: center;
  }

  .atic-rail-label {
    max-width: 100%;
    overflow: hidden;
    font-size: 0.625rem;
    font-weight: 600;
    letter-spacing: 0.01em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .atic-shell-main {
    min-width: 0;
    min-height: 0;
    flex: 1 1 auto;
    overflow: auto;
    overscroll-behavior: contain;
  }
</style>
