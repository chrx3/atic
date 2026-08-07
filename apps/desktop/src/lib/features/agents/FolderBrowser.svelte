<script lang="ts">
  /**
   * Explorador de carpetas interno (cwd de agentes).
   * Contained en `.demo` para no pelear con always-on-top / overlay-dismiss.
   */
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

  const ACCENT = "#da7756";

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
    } catch (e) {
      error =
        typeof e === "string"
          ? e
          : e instanceof Error
            ? e.message
            : String(e);
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

  function jumpRoot(entry: DirectoryEntry) {
    if (loading) return;
    void load(entry.path);
  }

  function confirm() {
    const path = listing?.path?.trim() || browsePath.trim();
    if (!path) return;
    onPick(path);
  }

  function rootIcon(name: string) {
    return name === "Inicio" ? House : Folder;
  }
</script>

<div class="folder-root" style="--accent: {ACCENT}">
  <Modal
    title="Elegir carpeta"
    subtitle={listing?.path || initialPath || undefined}
    size="sm"
    contained
    scrollBody={false}
    onClose={onClose}
  >
    <div class="body">
      {#if listing && listing.roots.length > 0}
        <div class="roots" role="group" aria-label="Carpetas frecuentes">
          {#each listing.roots as root (root.path)}
            <button
              type="button"
              class="root-chip"
              class:is-on={listing.path === root.path}
              title={root.path}
              disabled={loading}
              onclick={() => jumpRoot(root)}
            >
              <Icon icon={rootIcon(root.name)} size={12} />
              <span>{root.name}</span>
            </button>
          {/each}
        </div>
      {/if}

      <div class="nav">
        <button
          type="button"
          class="up"
          disabled={!canGoUp || loading}
          title="Subir un nivel"
          aria-label="Subir"
          onclick={goUp}
        >
          <Icon icon={ChevronUp} size={14} />
          <span>Subir</span>
        </button>
        <p class="path" title={listing?.path || ""}>
          {listing?.path || (loading ? "…" : "")}
        </p>
      </div>

      <div class="list" aria-label="Subcarpetas" aria-busy={loading}>
        {#if loading && !listing}
          <p class="empty">Cargando…</p>
        {:else if error && !listing}
          <div class="err">
            <p class="empty is-err">{error}</p>
            <button type="button" class="retry" onclick={() => load(initialPath || null)}>
              Reintentar
            </button>
          </div>
        {:else if listing && listing.entries.length === 0}
          <p class="empty">Sin subcarpetas</p>
        {:else if listing}
          <ul class="entries">
            {#each listing.entries as entry (entry.path)}
              <li>
                <button
                  type="button"
                  class="entry"
                  title={entry.path}
                  disabled={loading}
                  onclick={() => enter(entry)}
                >
                  <Icon icon={Folder} size={14} />
                  <span class="entry-t">{entry.name}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if error && listing}
          <p class="soft-err" role="status">{error}</p>
        {/if}
      </div>
    </div>

    {#snippet actions()}
      <Button variant="ghost" onclick={onClose}>Cancelar</Button>
      <Button
        variant="primary"
        disabled={!listing?.path || loading}
        onclick={confirm}
      >
        Usar esta carpeta
      </Button>
    {/snippet}
  </Modal>
</div>

<style>
  .folder-root {
    display: contents;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
    min-height: 0;
    height: min(52dvh, 320px);
  }

  .roots {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    flex-shrink: 0;
  }

  .root-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.22rem 0.55rem;
    border: 1px solid var(--rb-border);
    border-radius: 999px;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.68rem;
    cursor: pointer;
    transition:
      color var(--duration-quick, 150ms) ease,
      border-color var(--duration-quick, 150ms) ease,
      background-color var(--duration-quick, 150ms) ease;
  }

  .root-chip:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
  }

  .root-chip.is-on {
    color: var(--accent, #da7756);
    border-color: color-mix(in srgb, var(--accent, #da7756) 45%, var(--rb-border));
    background: color-mix(in srgb, var(--accent, #da7756) 12%, transparent);
  }

  .root-chip:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .nav {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    min-width: 0;
    flex-shrink: 0;
  }

  .up {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    flex-shrink: 0;
    padding: 0.28rem 0.5rem;
    border: 1px solid var(--rb-border);
    border-radius: 0.4rem;
    background: transparent;
    color: var(--rb-text);
    font-size: 0.72rem;
    cursor: pointer;
  }

  .up:hover:not(:disabled) {
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
  }

  .up:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .path {
    margin: 0;
    min-width: 0;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.65rem;
    color: var(--rb-faint);
    direction: rtl;
    text-align: left;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    border: 1px solid var(--rb-border);
    border-radius: 0.5rem;
    background: color-mix(in srgb, var(--rb-surface-2, transparent) 55%, transparent);
  }

  .entries {
    list-style: none;
    margin: 0;
    padding: 0.25rem;
  }

  .entry {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.45rem;
    padding: 0.4rem 0.5rem;
    border: 0;
    border-radius: 0.35rem;
    background: transparent;
    color: var(--rb-text);
    font-size: 0.8rem;
    text-align: left;
    cursor: pointer;
  }

  .entry:hover:not(:disabled) {
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .entry:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .entry-t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty {
    margin: 0;
    padding: 1.25rem 0.75rem;
    text-align: center;
    font-size: 0.78rem;
    color: var(--rb-faint);
  }

  .empty.is-err {
    color: var(--rb-text);
  }

  .err {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem;
  }

  .retry {
    padding: 0.28rem 0.55rem;
    border: 1px solid var(--rb-border);
    border-radius: 0.4rem;
    background: transparent;
    font-size: 0.72rem;
    color: var(--rb-text);
    cursor: pointer;
  }

  .soft-err {
    margin: 0;
    padding: 0.35rem 0.55rem 0.5rem;
    font-size: 0.68rem;
    color: var(--rb-faint);
  }
</style>
