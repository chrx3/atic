<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /** Explorador contenido para elegir el cwd de una consola de agentes. */
  import { onMount } from "svelte";
  import { listDirectories } from "$ipc/agents";
  import type { DirectoryEntry, DirectoryListing } from "$lib/types";
  import Button from "$ui/Button.svelte";
  import Icon from "$ui/Icon.svelte";
  import Modal from "$ui/Modal.svelte";
  import { ChevronUp, Folder, House } from "$lib/icons";

  let {
    initialPath = "",
    onPick,
    onClose,
  }: {
    initialPath?: string;
    onPick: (path: string) => void;
    onClose: () => void;
  } = $props();

  let listing = $state<DirectoryListing | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let browsePath = $state("");

  const canGoUp = $derived(!!listing?.parent);

  async function load(path: string | null | undefined) {
    loading = true;
    error = null;
    try {
      const next = await listDirectories(path?.trim() || null);
      listing = next;
      browsePath = next.path;
    } catch (cause) {
      error =
        typeof cause === "string"
          ? cause
          : cause instanceof Error
            ? cause.message
            : String(cause);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load(initialPath || null);
  });

  function goUp() {
    if (!listing?.parent || loading) return;
    void load(listing.parent);
  }

  function enter(entry: DirectoryEntry) {
    if (loading) return;
    void load(entry.path);
  }

  function confirm() {
    const path = listing?.path?.trim() || browsePath.trim();
    if (path) onPick(path);
  }

  function rootIcon(name: string) {
    return name === "Inicio" ? House : Folder;
  }
</script>

<div class="folder-root">
  <Modal
    title="Elegir carpeta"
    subtitle={listing?.path || initialPath || undefined}
    size="md"
    contained
    scrollBody={false}
    {onClose}
  >
    <div class="browser" class:is-loading={loading}>
      {#if listing && listing.roots.length > 0}
        <nav class="roots" aria-label="Ubicaciones frecuentes">
          {#each listing.roots as root (root.path)}
            <button
              type="button"
              class="root"
              class:is-on={listing.path === root.path}
              aria-current={listing.path === root.path ? "location" : undefined}
              use:tip={root.path}
              disabled={loading}
              onclick={() => load(root.path)}
            >
              <Icon icon={rootIcon(root.name)} size={13} />
              <span>{root.name}</span>
            </button>
          {/each}
        </nav>
      {/if}

      <div class="location">
        <button
          type="button"
          class="up"
          disabled={!canGoUp || loading}
          use:tip={"Subir un nivel"}
          aria-label="Subir un nivel"
          onclick={goUp}
        >
          <Icon icon={ChevronUp} size={14} />
          <span>Subir</span>
        </button>
        <p class="path" use:tip={listing?.path || ""}>
          {listing?.path || (loading ? "Cargando…" : "")}
        </p>
      </div>

      <div class="list" aria-label="Subcarpetas" aria-busy={loading}>
        {#if loading && !listing}
          <p class="state">Cargando carpetas…</p>
        {:else if error && !listing}
          <div class="error">
            <p>{error}</p>
            <button
              type="button"
              class="retry"
              onclick={() => load(initialPath || null)}
            >
              Reintentar
            </button>
          </div>
        {:else if listing && listing.entries.length === 0}
          <div class="empty">
            <Icon icon={Folder} size={18} />
            <span>Esta carpeta no tiene subcarpetas</span>
          </div>
        {:else if listing}
          <ul class="entries">
            {#each listing.entries as entry (entry.path)}
              <li>
                <button
                  type="button"
                  class="entry"
                  use:tip={entry.path}
                  disabled={loading}
                  onclick={() => enter(entry)}
                >
                  <span class="folder-icon"><Icon icon={Folder} size={14} /></span>
                  <span>{entry.name}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}

        {#if error && listing}
          <p class="soft-error" role="status">{error}</p>
        {/if}
      </div>
    </div>

    {#snippet actions()}
      <div class="actions">
        <Button variant="ghost" full onclick={onClose}>Cancelar</Button>
        <Button
          variant="primary"
          full
          disabled={!listing?.path || loading}
          onclick={confirm}
        >
          Usar esta carpeta
        </Button>
      </div>
    {/snippet}
  </Modal>
</div>

<style>
  .folder-root {
    display: contents;
  }

  .browser {
    display: flex;
    height: min(54dvh, 20rem);
    min-height: 0;
    flex-direction: column;
    gap: 0.6rem;
    container: folder-browser / inline-size;
  }

  .roots {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(6rem, 100%), 1fr));
    flex: 0 0 auto;
    gap: 0.4rem;
  }

  .root,
  .up,
  .retry {
    border: 1px solid var(--rb-border);
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      background-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .root {
    display: flex;
    min-width: 0;
    min-height: 2rem;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    border-radius: var(--rb-radius-xs);
    padding: 0.32rem 0.5rem;
    font-size: 0.68rem;
  }

  .root span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .root:hover:not(:disabled),
  .up:hover:not(:disabled),
  .retry:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
    color: var(--rb-text);
  }

  .root.is-on {
    border-color: color-mix(in sRGB, var(--rb-record) 45%, var(--rb-border));
    background: var(--rb-record-soft);
    color: var(--rb-record);
  }

  .location {
    display: flex;
    min-width: 0;
    flex: 0 0 auto;
    align-items: center;
    gap: 0.45rem;
  }

  .up {
    display: inline-flex;
    min-height: 2rem;
    flex: 0 0 auto;
    align-items: center;
    gap: 0.28rem;
    border-radius: var(--rb-radius-xs);
    padding: 0.3rem 0.55rem;
    color: var(--rb-text);
    font-size: 0.7rem;
  }

  .path {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    margin: 0;
    color: var(--rb-muted);
    direction: rtl;
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.64rem;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list {
    min-height: 0;
    flex: 1;
    overflow: auto;
    border: 1px solid var(--rb-border);
    border-radius: var(--rb-radius-sm);
    background: color-mix(in sRGB, var(--rb-surface-2) 58%, transparent);
    overscroll-behavior: contain;
  }

  .entries {
    margin: 0;
    padding: 0.3rem;
    list-style: none;
  }

  .entry {
    display: flex;
    width: 100%;
    min-width: 0;
    min-height: 2.2rem;
    align-items: center;
    gap: 0.5rem;
    border: 0;
    border-radius: var(--rb-radius-xs);
    padding: 0.36rem 0.5rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.76rem;
    text-align: left;
    cursor: pointer;
  }

  .entry:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .entry span:last-child {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-icon {
    display: grid;
    width: 1.55rem;
    height: 1.55rem;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 0.38rem;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-muted);
  }

  .state,
  .soft-error {
    margin: 0;
    padding: 1rem 0.75rem;
    color: var(--rb-faint);
    font-size: 0.72rem;
    text-align: center;
  }

  .empty,
  .error {
    display: flex;
    min-height: 100%;
    align-items: center;
    justify-content: center;
    gap: 0.45rem;
    padding: 1rem;
    color: var(--rb-faint);
    font-size: 0.72rem;
    text-align: center;
  }

  .error {
    flex-direction: column;
    color: var(--rb-text);
  }

  .error p {
    max-width: 32ch;
    margin: 0;
  }

  .retry {
    min-height: 2rem;
    border-radius: var(--rb-radius-xs);
    padding: 0.3rem 0.6rem;
    color: var(--rb-text);
    font-size: 0.7rem;
  }

  .soft-error {
    padding-block: 0.45rem;
  }

  .actions {
    display: grid;
    width: min(100%, 21rem);
    grid-template-columns: minmax(0, 0.8fr) minmax(0, 1.2fr);
    gap: 0.5rem;
  }

  button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  button:disabled,
  .browser.is-loading .entry {
    cursor: default;
    opacity: 0.5;
  }

  @container folder-browser (width <= 22rem) {
    .browser {
      gap: 0.45rem;
    }

    .roots {
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 0.3rem;
    }

    .root {
      padding-inline: 0.35rem;
    }

    .up span {
      display: none;
    }

    .up {
      width: 2rem;
      justify-content: center;
      padding: 0;
    }
  }

  @media (pointer: coarse) {
    .root,
    .up,
    .retry,
    .entry {
      min-height: 2.75rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .root,
    .up,
    .retry {
      transition: none;
    }
  }
</style>
