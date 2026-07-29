<script lang="ts">
  /**
   * Overlay del launcher tipo Spotlight.
   * Atajo global → buscar apps / acciones → Enter abre.
   */
  import { onMount, tick } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ArrowDown from "reicon-svelte/icons/ArrowDown.svelte";
  import ArrowDownLeft from "reicon-svelte/icons/ArrowDownLeft.svelte";
  import ArrowUp from "reicon-svelte/icons/ArrowUp.svelte";
  import Clipboard from "reicon-svelte/icons/Clipboard.svelte";
  import Code from "reicon-svelte/icons/Code.svelte";
  import Crop from "reicon-svelte/icons/Crop.svelte";
  import DocumentText from "reicon-svelte/icons/DocumentText.svelte";
  import Microphone from "reicon-svelte/icons/Microphone.svelte";
  import Search from "reicon-svelte/icons/Search.svelte";
  import Settings from "reicon-svelte/icons/Settings.svelte";
  import Window from "reicon-svelte/icons/Window.svelte";
  import X from "reicon-svelte/icons/X.svelte";
  import {
    hideLauncher,
    launcherRun,
    launcherSearch,
    onLauncherOpened,
  } from "$lib/api";
  import type { LauncherHit } from "$lib/types";
  import { applyTheme, readCachedTheme } from "$lib/theme";

  let query = $state("");
  let hits = $state<LauncherHit[]>([]);
  let selected = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);
  let searching = $state(false);
  let error = $state("");

  let searchGen = 0;

  // Los iconos de `reicon-svelte` son componentes de clase (Svelte 4), no el
  // `Component` funcional de Svelte 5: tiparlos con `Component` no compila.
  // Como todos comparten firma, uno cualquiera sirve de tipo para el resto.
  type Icon = typeof Microphone;

  const ACTION_ICONS: Record<string, Icon> = {
    "action:dictation": Microphone,
    "action:capture": Crop,
    "action:clipboard": Clipboard,
    "action:snippets": DocumentText,
    "action:agents": Code,
    "action:settings": Settings,
  };

  function iconFor(hit: LauncherHit): Icon {
    return ACTION_ICONS[hit.id] ?? (hit.kind === "app" ? Window : Search);
  }

  async function refresh(q: string) {
    const gen = ++searchGen;
    searching = true;
    error = "";
    try {
      const next = await launcherSearch(q);
      if (gen !== searchGen) return;
      hits = next;
      selected = 0;
    } catch (e) {
      if (gen !== searchGen) return;
      error = e instanceof Error ? e.message : String(e);
      hits = [];
    } finally {
      if (gen === searchGen) searching = false;
    }
  }

  async function clearQuery() {
    query = "";
    await refresh("");
    await tick();
    inputEl?.focus();
  }

  async function focusInput() {
    query = "";
    await refresh("");
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }

  async function runSelected() {
    const hit = hits[selected];
    if (!hit) return;
    try {
      await launcherRun(hit.id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      await hideLauncher();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (hits.length === 0) return;
      selected = (selected + 1) % hits.length;
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (hits.length === 0) return;
      selected = (selected - 1 + hits.length) % hits.length;
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      await runSelected();
    }
  }

  onMount(() => {
    applyTheme(readCachedTheme());
    const win = getCurrentWindow();
    const unsubs: UnlistenFn[] = [];
    let cancelled = false;

    void (async () => {
      unsubs.push(await onLauncherOpened(() => void focusInput()));
      unsubs.push(
        await win.onFocusChanged(({ payload: focused }) => {
          if (!focused) void hideLauncher();
        }),
      );
      if (!cancelled) void focusInput();
    })();

    return () => {
      cancelled = true;
      for (const u of unsubs) u();
    };
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="launcher" role="dialog" aria-label="Buscar">
  <div class="launcher-panel">
    <div class="launcher-search">
      <span class="launcher-icon" aria-hidden="true">
        <Search size={18} color="currentColor" />
      </span>
      <input
        bind:this={inputEl}
        class="launcher-input"
        type="text"
        placeholder="Buscar apps y acciones…"
        autocomplete="off"
        spellcheck="false"
        bind:value={query}
        oninput={() => void refresh(query)}
      />
      {#if searching}
        <span class="launcher-status" aria-hidden="true">…</span>
      {:else if query}
        <button
          type="button"
          class="launcher-clear"
          aria-label="Limpiar búsqueda"
          onclick={() => void clearQuery()}
        >
          <X size={14} color="currentColor" />
        </button>
      {/if}
    </div>

    {#if error}
      <p class="launcher-error" role="alert">{error}</p>
    {/if}

    <ul class="launcher-list" role="listbox" aria-label="Resultados">
      {#each hits as hit, i (hit.id)}
        {@const HitIcon = iconFor(hit)}
        <li>
          <button
            type="button"
            class="launcher-row"
            class:selected={i === selected}
            role="option"
            aria-selected={i === selected}
            onmouseenter={() => (selected = i)}
            onclick={() => {
              selected = i;
              void runSelected();
            }}
          >
            <span class="launcher-tile" data-kind={hit.kind} aria-hidden="true">
              <HitIcon size={16} color="currentColor" />
            </span>
            <span class="launcher-text">
              <span class="launcher-title">{hit.title}</span>
              <span class="launcher-sub">{hit.subtitle}</span>
            </span>
          </button>
        </li>
      {:else}
        <li class="launcher-empty">
          <span class="launcher-empty-icon" aria-hidden="true">
            <Search size={22} color="currentColor" />
          </span>
          <span>Sin resultados</span>
        </li>
      {/each}
    </ul>

    <footer class="launcher-footer">
      <span class="launcher-hint">
        <span class="launcher-kbd" aria-hidden="true">
          <ArrowUp size={11} color="currentColor" />
          <ArrowDown size={11} color="currentColor" />
        </span>
        navegar
      </span>
      <span class="launcher-hint">
        <span class="launcher-kbd" aria-hidden="true">
          <ArrowDownLeft size={11} color="currentColor" />
        </span>
        abrir
      </span>
      <span class="launcher-hint">
        <span class="launcher-kbd" aria-hidden="true">
          <X size={11} color="currentColor" />
        </span>
        cerrar
      </span>
    </footer>
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
  }

  .launcher {
    box-sizing: border-box;
    width: 100vw;
    height: 100vh;
    padding: 12px;
    font-family: var(--rb-font);
    color: var(--rb-text);
    -webkit-font-smoothing: antialiased;
  }

  .launcher-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    border-radius: 20px;
    background: color-mix(in srgb, var(--rb-surface-elevated) 92%, transparent);
    border: 1px solid var(--rb-border);
    box-shadow:
      0 1px 0 rgba(255, 255, 255, 0.45) inset,
      0 18px 48px rgba(0, 0, 0, 0.18);
    backdrop-filter: blur(18px);
    overflow: hidden;
    animation: launcher-liquid-in 220ms cubic-bezier(0.34, 1.18, 0.64, 1);
  }
  @keyframes launcher-liquid-in {
    from {
      opacity: 0.9;
      filter: blur(2px);
      transform: scale(0.97);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .launcher-panel {
      animation: none;
    }
  }

  .launcher-search {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--rb-hairline);
  }

  .launcher-icon {
    flex: 0 0 auto;
    display: inline-flex;
    color: var(--rb-muted);
  }

  .launcher-input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    font: inherit;
    font-size: 17px;
    letter-spacing: -0.01em;
    color: var(--rb-text);
  }

  .launcher-input::placeholder {
    color: var(--rb-faint);
  }

  .launcher-status {
    color: var(--rb-faint);
    font-variant-numeric: tabular-nums;
  }

  .launcher-clear {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: 8px;
    padding: 0;
    background: var(--rb-surface-2);
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color 120ms cubic-bezier(0.2, 0, 0, 1),
      color 120ms cubic-bezier(0.2, 0, 0, 1);
  }

  .launcher-clear:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-surface-2) 70%, var(--rb-border));
  }

  .launcher-error {
    margin: 0;
    padding: 8px 16px;
    font-size: 12px;
    color: var(--rb-record);
    background: var(--rb-record-soft);
  }

  .launcher-list {
    list-style: none;
    margin: 0;
    padding: 8px;
    flex: 1;
    overflow: auto;
  }

  .launcher-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    border: 0;
    border-radius: 12px;
    padding: 10px 12px;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color 120ms cubic-bezier(0.2, 0, 0, 1),
      transform 120ms cubic-bezier(0.2, 0, 0, 1);
  }

  .launcher-row:active {
    transform: scale(0.98);
  }

  .launcher-row.selected {
    background: color-mix(in srgb, var(--rb-accent) 8%, transparent);
  }

  .launcher-tile {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 10px;
    color: var(--rb-muted);
    background: var(--rb-surface-2);
  }

  .launcher-tile[data-kind="action"] {
    color: var(--rb-ok);
    background: var(--rb-ok-soft);
  }

  .launcher-row.selected .launcher-tile {
    color: var(--rb-accent);
    background: color-mix(in srgb, var(--rb-accent) 14%, var(--rb-surface-2));
  }

  .launcher-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .launcher-title {
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .launcher-sub {
    font-size: 12px;
    color: var(--rb-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .launcher-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 36px 12px;
    text-align: center;
    color: var(--rb-faint);
    font-size: 13px;
  }

  .launcher-empty-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    color: var(--rb-faint);
    background: var(--rb-surface-2);
  }

  .launcher-footer {
    display: flex;
    gap: 14px;
    padding: 8px 16px 10px;
    border-top: 1px solid var(--rb-hairline);
    font-size: 11px;
    color: var(--rb-faint);
  }

  .launcher-hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .launcher-kbd {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    color: var(--rb-muted);
  }
</style>
