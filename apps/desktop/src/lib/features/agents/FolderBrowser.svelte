<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /** Explorador contenido para elegir el cwd de una consola de agentes. */
  import { onMount, tick } from "svelte";
  import { listDirectories } from "$ipc/agents";
  import type { DirectoryEntry, DirectoryListing } from "$lib/types";
  import Button from "$ui/Button.svelte";
  import Icon from "$ui/Icon.svelte";
  import Modal from "$ui/Modal.svelte";
  import { ChevronUp, Folder, House, Search, Star } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";
  import {
    TYPEAHEAD_MS,
    filterEntries,
    isFav,
    isJumpKey,
    isTypingTarget,
    jumpIndex,
    leafName,
    pathsEqual,
    readFavs,
    toggleFav,
    writeFavs,
    type FolderFav,
  } from "./folderBrowse";

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
  let query = $state("");
  let favs = $state<FolderFav[]>(readFavs());
  let activePath = $state<string | null>(null);
  let listEl = $state<HTMLElement | null>(null);
  let searchEl = $state<HTMLInputElement | null>(null);
  let jumpBuffer = "";
  let jumpAt = 0;

  const canGoUp = $derived(!!listing?.parent);
  const visible = $derived(
    listing ? filterEntries(listing.entries, query) : [],
  );
  const currentFav = $derived(!!browsePath && isFav(favs, browsePath));

  async function load(path: string | null | undefined) {
    loading = true;
    error = null;
    query = "";
    jumpBuffer = "";
    activePath = null;
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
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "f" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        searchEl?.focus();
        searchEl?.select();
        return;
      }
      if (event.key === "/" && !isTypingTarget(event.target)) {
        event.preventDefault();
        searchEl?.focus();
        return;
      }
      if (event.key === "Enter" && visible.length > 0) {
        if (event.target instanceof HTMLButtonElement) return;
        const hit =
          visible.find((entry) => entry.path === activePath) ??
          (isTypingTarget(event.target) ? visible[0] : null);
        if (!hit) return;
        event.preventDefault();
        enter(hit);
        return;
      }
      if (
        (event.key === "ArrowDown" || event.key === "ArrowUp") &&
        !isTypingTarget(event.target) &&
        visible.length > 0
      ) {
        event.preventDefault();
        const from = visible.findIndex((entry) => entry.path === activePath);
        const delta = event.key === "ArrowDown" ? 1 : -1;
        const next =
          from < 0
            ? event.key === "ArrowDown"
              ? 0
              : visible.length - 1
            : (from + delta + visible.length) % visible.length;
        void revealIndex(next);
        return;
      }
      if (!isJumpKey(event) || isTypingTarget(event.target)) return;
      event.preventDefault();
      const now = event.timeStamp;
      if (now - jumpAt > TYPEAHEAD_MS) jumpBuffer = "";
      jumpAt = now;
      jumpBuffer += event.key;
      const names = visible.map((entry) => entry.name);
      const from = visible.findIndex((entry) => entry.path === activePath);
      const idx = jumpIndex(names, jumpBuffer, from);
      if (idx >= 0) void revealIndex(idx);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function revealIndex(idx: number) {
    const entry = visible[idx];
    if (!entry) return;
    activePath = entry.path;
    await tick();
    const row = listEl?.querySelector(`[data-idx="${idx}"]`);
    if (row instanceof HTMLElement) row.scrollIntoView({ block: "nearest" });
  }

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
    return name === "Inicio" || name === "Home" ? House : Folder;
  }

  function pin(entry: FolderFav, event?: Event) {
    event?.stopPropagation();
    event?.preventDefault();
    favs = toggleFav(favs, entry);
    writeFavs(favs);
  }

  function pinCurrent() {
    if (!browsePath) return;
    pin({ name: leafName(browsePath), path: browsePath });
  }
</script>

<div class="folder-root">
  <Modal
    title={t("page.agents.folderPick.title")}
    subtitle={listing?.path || initialPath || undefined}
    size="xl"
    contained
    fill
    scrollBody={false}
    {onClose}
  >
    <div class="browser" class:is-loading={loading}>
      {#if listing && listing.roots.length > 0}
        <nav class="roots" aria-label={t("page.agents.folderPick.roots")}>
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

      {#if favs.length > 0}
        <nav class="favs" aria-label={t("page.agents.folderPick.favs")}>
          {#each favs as fav (fav.path)}
            <div class="fav" class:is-on={pathsEqual(browsePath, fav.path)}>
              <button
                type="button"
                class="fav-go"
                use:tip={fav.path}
                disabled={loading}
                onclick={() => load(fav.path)}
              >
                <Icon icon={Folder} size={12} />
                <span>{fav.name}</span>
              </button>
              <button
                type="button"
                class="star is-on"
                use:tip={t("page.agents.folderPick.favRemove")}
                aria-label={t("page.agents.folderPick.favRemove")}
                onclick={(event) => pin(fav, event)}
              >
                <Icon icon={Star} size={12} fill="currentColor" />
              </button>
            </div>
          {/each}
        </nav>
      {/if}

      <div class="location">
        <button
          type="button"
          class="up"
          disabled={!canGoUp || loading}
          use:tip={t("page.agents.folderPick.upAria")}
          aria-label={t("page.agents.folderPick.upAria")}
          onclick={goUp}
        >
          <Icon icon={ChevronUp} size={14} />
          <span>{t("page.agents.folderPick.up")}</span>
        </button>
        <p class="path" use:tip={listing?.path || ""}>
          {listing?.path || (loading ? t("page.agents.folderPick.loading") : "")}
        </p>
        <button
          type="button"
          class="star"
          class:is-on={currentFav}
          disabled={!browsePath || loading}
          use:tip={currentFav
            ? t("page.agents.folderPick.favRemove")
            : t("page.agents.folderPick.favCurrent")}
          aria-label={currentFav
            ? t("page.agents.folderPick.favRemove")
            : t("page.agents.folderPick.favCurrent")}
          aria-pressed={currentFav}
          onclick={pinCurrent}
        >
          <Icon icon={Star} size={14} fill={currentFav ? "currentColor" : "none"} />
        </button>
      </div>

      <label class="search">
        <Icon icon={Search} size={13} />
        <input
          bind:this={searchEl}
          type="search"
          bind:value={query}
          placeholder={t("page.agents.folderPick.search")}
          aria-label={t("page.agents.folderPick.searchAria")}
          autocomplete="off"
          spellcheck="false"
        />
      </label>

      <div
        class="list"
        bind:this={listEl}
        aria-label={t("page.agents.folderPick.entries")}
        aria-busy={loading}
      >
        {#if loading && !listing}
          <p class="state">{t("page.agents.folderPick.loading")}</p>
        {:else if error && !listing}
          <div class="error">
            <p>{error}</p>
            <button
              type="button"
              class="retry"
              onclick={() => load(initialPath || null)}
            >
              {t("page.agents.folderPick.retry")}
            </button>
          </div>
        {:else if listing && listing.entries.length === 0}
          <div class="empty">
            <Icon icon={Folder} size={18} />
            <span>{t("page.agents.folderPick.empty")}</span>
          </div>
        {:else if listing && visible.length === 0}
          <div class="empty">
            <Icon icon={Search} size={18} />
            <span>{t("page.agents.folderPick.noMatch")}</span>
          </div>
        {:else if listing}
          <ul class="entries">
            {#each visible as entry, idx (entry.path)}
              <li>
                <div class="row" class:is-active={entry.path === activePath}>
                  <button
                    type="button"
                    class="entry"
                    data-idx={idx}
                    use:tip={entry.path}
                    disabled={loading}
                    onclick={() => enter(entry)}
                  >
                    <span class="folder-icon"><Icon icon={Folder} size={14} /></span>
                    <span>{entry.name}</span>
                  </button>
                  <button
                    type="button"
                    class="star"
                    class:is-on={isFav(favs, entry.path)}
                    use:tip={isFav(favs, entry.path)
                      ? t("page.agents.folderPick.favRemove")
                      : t("page.agents.folderPick.favAdd")}
                    aria-label={isFav(favs, entry.path)
                      ? t("page.agents.folderPick.favRemove")
                      : t("page.agents.folderPick.favAdd")}
                    aria-pressed={isFav(favs, entry.path)}
                    onclick={(event) =>
                      pin({ name: entry.name, path: entry.path }, event)}
                  >
                    <Icon
                      icon={Star}
                      size={13}
                      fill={isFav(favs, entry.path) ? "currentColor" : "none"}
                    />
                  </button>
                </div>
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
        <Button variant="ghost" full onclick={onClose}>{t("chrome.cancel")}</Button>
        <Button
          variant="primary"
          full
          disabled={!listing?.path || loading}
          onclick={confirm}
        >
          {t("page.agents.folderPick.use")}
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
    flex: 1;
    min-height: 0;
    height: 100%;
    flex-direction: column;
    gap: 0.7rem;
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

  .favs {
    display: flex;
    flex: 0 0 auto;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .fav {
    display: flex;
    min-width: 0;
    max-width: min(100%, 16rem);
    flex: 0 0 auto;
    align-items: center;
    border: 1px solid var(--rb-border);
    border-radius: var(--rb-radius-xs);
    background: color-mix(in sRGB, var(--rb-text) 4%, transparent);
    white-space: nowrap;
  }

  .fav.is-on {
    border-color: color-mix(in sRGB, var(--rb-record) 45%, var(--rb-border));
    background: var(--rb-record-soft);
    color: var(--rb-record);
  }

  .fav-go {
    display: flex;
    min-width: 0;
    flex: 1 1 auto;
    min-height: 1.85rem;
    align-items: center;
    gap: 0.3rem;
    border: 0;
    padding: 0.2rem 0.2rem 0.2rem 0.45rem;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 0.68rem;
    cursor: pointer;
  }

  .fav-go span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .search {
    display: flex;
    min-width: 0;
    flex: 0 0 auto;
    align-items: center;
    gap: 0.4rem;
    border: 1px solid var(--rb-border);
    border-radius: var(--rb-radius-xs);
    padding: 0 0.55rem;
    color: var(--rb-muted);
    background: color-mix(in sRGB, var(--rb-surface-2) 58%, transparent);
  }

  .search input {
    width: 100%;
    min-width: 0;
    height: 2rem;
    border: 0;
    padding: 0;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.74rem;
    outline: none;
  }

  .search input::placeholder {
    color: var(--rb-faint);
  }

  .search:focus-within {
    border-color: color-mix(in sRGB, var(--rb-text) 28%, var(--rb-border));
    color: var(--rb-text);
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
    min-height: 12rem;
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

  .row {
    display: flex;
    min-width: 0;
    align-items: center;
    border-radius: var(--rb-radius-xs);
  }

  .row:hover,
  .row.is-active {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .row.is-active {
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rb-text) 12%, transparent);
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
    padding: 0.36rem 0.15rem 0.36rem 0.5rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.76rem;
    text-align: left;
    cursor: pointer;
  }

  .star {
    position: relative;
    display: grid;
    width: 2rem;
    height: 2rem;
    flex: 0 0 auto;
    place-items: center;
    border: 0;
    border-radius: var(--rb-radius-xs);
    padding: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .star::before {
    content: "";
    position: absolute;
    inset: -4px;
  }

  .star:hover:not(:disabled),
  .star.is-on {
    color: var(--rb-text);
  }

  .star:active:not(:disabled) {
    transform: scale(0.96);
  }

  .fav .star {
    width: 1.7rem;
    height: 1.7rem;
    color: var(--rb-text);
  }

  .fav .star::before {
    inset: 0;
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
    .entry,
    .fav-go {
      min-height: 2.75rem;
    }

    .star {
      width: 2.5rem;
      height: 2.5rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .root,
    .up,
    .retry,
    .star {
      transition: none;
    }
  }
</style>
