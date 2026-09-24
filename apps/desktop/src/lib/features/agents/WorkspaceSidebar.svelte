<script lang="ts">
  /**
   * La barra lateral de la ventana de agentes: empezar, lo vivo y lo reciente.
   *
   * Chats y terminales en una sola lista, con su estado: lo que importa es
   * «qué agentes tengo y cuál me necesita», no de qué tipo es cada uno. Solo
   * dibuja; qué hace cada clic lo decide la ventana.
   */
  import { t } from "$domain/i18n.svelte";
  import { formatListWhen } from "$core/format";
  import Icon from "$ui/Icon.svelte";
  import {
    Folder,
    MessageSquare,
    Plus,
    Search,
    Settings,
    SquareTerminal,
    X,
  } from "$lib/icons";
  import type { StoredThread } from "$lib/types";
  import AgentLogo from "./AgentLogo.svelte";
  import ChatPopover from "./ChatPopover.svelte";
  import type { AgentDef } from "./agentCatalog";
  import { groupByDate, type ItemStatus, type WorkspaceItem } from "./agentWorkspace";

  let {
    items,
    active,
    statusOf,
    titleOf,
    folderOf,
    choices,
    cwd,
    recent,
    newOpen,
    onNewToggle,
    onNew,
    onSelect,
    onClose,
    onPickFolder,
    onResume,
    onSettings,
  }: {
    items: WorkspaceItem[];
    active: string | null;
    statusOf: (item: WorkspaceItem) => ItemStatus;
    /** Lo que se pidió, no el nombre del agente: igual que la barra de arriba. */
    titleOf: (item: WorkspaceItem) => string;
    folderOf: (item: WorkspaceItem) => string;
    choices: AgentDef[];
    cwd: string;
    recent: StoredThread[];
    newOpen: boolean;
    onNewToggle: (open: boolean) => void;
    onNew: (agent: AgentDef | null, kind: "chat" | "terminal") => void;
    onSelect: (key: string) => void;
    onClose: (key: string) => void;
    onPickFolder: () => void;
    onResume: (thread: StoredThread) => void;
    onSettings: () => void;
  } = $props();

  function folderName(path: string): string {
    if (!path) return t("page.agents.window.home");
    return (
      path
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() || path
    );
  }

  /** Bajo «Hoy» o «Ayer» sobra repetir el día: basta la hora. */
  function whenIn(bucket: string, epochSecs: number): string {
    if (bucket !== "today" && bucket !== "yesterday") return formatListWhen(epochSecs);
    return new Date(epochSecs * 1000).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function cliOf(backendId: string): string {
    return choices.find((a) => a.backend === backendId)?.cli ?? backendId;
  }

  let query = $state("");
  const needle = $derived(query.trim().toLowerCase());

  function matches(...texts: string[]): boolean {
    return !needle || texts.some((text) => text.toLowerCase().includes(needle));
  }

  const shownItems = $derived(
    items.filter((item) => matches(titleOf(item), item.label, folderOf(item))),
  );
  const recentGroups = $derived(
    groupByDate(recent.filter((th) => matches(th.preview, th.cwd, th.backendName))),
  );

  /** Solo lo que pide ojos lleva punto: una sesión lista no. */
  const DOT: Partial<Record<ItemStatus, string>> = {
    working: "is-working",
    waiting: "is-waiting",
    failed: "is-failed",
    unread: "is-unread",
  };
</script>

<aside class="side">
  <div class="top">
    <ChatPopover
      open={newOpen}
      onToggle={onNewToggle}
      label={t("page.agents.window.new")}
      width={272}
      placement="down"
    >
      {#snippet trigger()}
        <Icon icon={Plus} size={14} />
        <span class="new-label">{t("page.agents.window.new")}</span>
      {/snippet}

      {#each choices as agent (agent.cli)}
        <div class="new-row">
          <span class="new-agent">
            <AgentLogo agent={agent.cli} size={15} />
            {agent.name}
          </span>
          <button
            type="button"
            class="new-kind is-chat"
            title={t("page.agents.window.newChat", { name: agent.name })}
            onclick={() => onNew(agent, "chat")}
          >
            <Icon icon={MessageSquare} size={12} />
            {t("page.agents.window.chat")}
          </button>
          <button
            type="button"
            class="new-kind"
            title={t("page.agents.window.newTerminal", { name: agent.name })}
            onclick={() => onNew(agent, "terminal")}
          >
            <Icon icon={SquareTerminal} size={12} />
          </button>
        </div>
      {/each}
      <button type="button" class="new-shell" onclick={() => onNew(null, "terminal")}>
        <Icon icon={SquareTerminal} size={13} />
        {t("page.agents.window.shell")}
      </button>
    </ChatPopover>

    <label class="search">
      <Icon icon={Search} size={13} />
      <input
        type="search"
        bind:value={query}
        placeholder={t("page.agents.window.search")}
        aria-label={t("page.agents.window.search")}
        onkeydown={(e) => {
          if (e.key === "Escape" && query) {
            e.stopPropagation();
            query = "";
          }
        }}
      />
    </label>
  </div>

  <nav class="list" aria-label={t("page.agents.window.sessions")}>
    {#each shownItems as item (item.key)}
      {@const status = statusOf(item)}
      {@const title = titleOf(item)}
      {@const folder = folderOf(item)}
      <div class="row" class:is-active={item.key === active}>
        <button
          type="button"
          class="item"
          aria-current={item.key === active ? "true" : undefined}
          onclick={() => onSelect(item.key)}
        >
          <span class="logo">
            <AgentLogo agent={item.cli} size={18} />
            {#if DOT[status]}<span class="dot {DOT[status]}" aria-hidden="true"
              ></span>{/if}
          </span>
          <span class="copy">
            <span class="name" {title}>{title}</span>
            <span class="status" class:is-attn={status === "waiting"}>
              {#if folder}{folderName(folder)} ·
              {/if}{t(`page.agents.window.status.${status}`)}
            </span>
          </span>
        </button>
        <button
          type="button"
          class="close"
          aria-label={t("page.agents.window.close", { name: item.label })}
          onclick={() => onClose(item.key)}
        >
          <Icon icon={X} size={11} />
        </button>
      </div>
    {:else}
      {#if !needle}
        <p class="quiet">{t("page.agents.window.noSessions")}</p>
      {/if}
    {/each}
  </nav>

  <section class="recent" aria-label={t("page.agents.window.recent")}>
    {#each recentGroups as group (group.bucket)}
      <p class="heading">{t(`page.agents.window.${group.bucket}`)}</p>
      {#each group.items as thread (thread.id)}
        <button
          type="button"
          class="thread"
          title={thread.preview}
          onclick={() => onResume(thread)}
        >
          <AgentLogo agent={cliOf(thread.backendId)} size={13} />
          <span class="thread-copy">
            <span class="thread-text">{thread.preview}</span>
            {#if thread.cwd}
              <span class="thread-folder">{folderName(thread.cwd)}</span>
            {/if}
          </span>
          <span class="thread-when">{whenIn(group.bucket, thread.updatedAt)}</span>
        </button>
      {/each}
    {:else}
      {#if needle}
        <p class="quiet">{t("page.agents.window.noMatches", { q: query.trim() })}</p>
      {/if}
    {/each}
  </section>

  <footer class="foot">
    <button
      type="button"
      class="folder"
      title={`${t("page.agents.window.pickFolder")}: ${cwd || t("page.agents.window.home")}`}
      onclick={onPickFolder}
    >
      <Icon icon={Folder} size={13} />
      <span>{folderName(cwd)}</span>
    </button>
    <button
      type="button"
      class="gear"
      aria-label={t("settings.agents.modalTitle")}
      title={t("settings.agents.modalTitle")}
      onclick={onSettings}
    >
      <Icon icon={Settings} size={14} />
    </button>
  </footer>
</aside>

<style>
  .side {
    display: flex;
    flex-shrink: 0;
    flex-direction: column;
    gap: 10px;
    width: 260px;
    min-width: 0;
    height: 100%;
    box-sizing: border-box;
    padding: 12px 10px;
    background: var(--rb-surface);
    box-shadow: inset -1px 0 0 color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .top {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  /* El trigger del popover: aquí es el botón principal de la barra. */
  .top :global(.trigger) {
    width: 100%;
    height: 34px;
    justify-content: flex-start;
    gap: 8px;
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
    font-size: 13px;
    font-weight: 600;
  }

  .new-label {
    flex: 1;
    text-align: left;
  }

  .new-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 2px 2px 6px;
  }

  .new-agent {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 12.5px;
    font-weight: 560;
  }

  .new-kind,
  .new-shell {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    border: 0;
    border-radius: 7px;
    padding: 0 8px;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .new-kind:hover,
  .new-shell:hover {
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);
    color: var(--rb-text);
  }

  .new-kind.is-chat {
    background: color-mix(in sRGB, var(--accent) 16%, transparent);
    color: var(--rb-text);
  }

  .new-shell {
    width: 100%;
    margin-top: 6px;
    background: transparent;
  }

  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    border-radius: 8px;
    padding: 0 10px;
    background: color-mix(in sRGB, var(--rb-text) 4%, transparent);
    color: var(--rb-faint);
    transition: box-shadow var(--duration-fast) ease;
  }

  .search:focus-within {
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--accent) 55%, transparent);
    color: var(--rb-muted);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    padding: 0;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    outline: none;
  }

  .search input::placeholder {
    color: var(--rb-faint);
  }

  .search input::-webkit-search-cancel-button {
    display: none;
  }

  .foot {
    display: flex;
    gap: 2px;
    padding-top: 8px;
    border-top: 1px solid color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .folder {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 30px;
    border: 0;
    border-radius: 8px;
    padding: 0 10px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .gear {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 30px;
    height: 30px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .gear:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
  }

  .folder:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
  }

  .folder span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list {
    display: flex;
    flex: 0 1 auto;
    flex-direction: column;
    gap: 2px;
    min-height: 0;
    max-height: 45%;
    overflow-y: auto;
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: 10px;
    transition: background-color 120ms ease;
  }

  .row:hover {
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
  }

  .row.is-active {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
  }

  /* La elegida lleva una marca del color del acento al borde. */
  .row.is-active::before {
    position: absolute;
    top: 12px;
    bottom: 12px;
    left: 2px;
    width: 3px;
    border-radius: 999px;
    background: var(--accent);
    content: "";
  }

  .item {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    border: 0;
    padding: 7px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .logo {
    position: relative;
    display: grid;
    flex-shrink: 0;
    place-items: center;
  }

  .dot {
    position: absolute;
    right: -3px;
    bottom: -2px;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    box-shadow: 0 0 0 2px var(--rb-surface);
  }

  .dot.is-working {
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }

  .dot.is-waiting {
    background: var(--rb-warn);
  }

  .dot.is-failed {
    background: var(--rb-record);
  }

  .dot.is-unread {
    background: var(--rb-ok);
  }

  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }

  .copy {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .name,
  .status {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: 12.5px;
    font-weight: 600;
  }

  .status {
    color: var(--rb-muted);
    font-size: 11px;
  }

  .status.is-attn {
    color: var(--rb-warn);
  }

  .close {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 24px;
    height: 24px;
    margin-right: 4px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-faint);
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .row:hover .close,
  .close:focus-visible {
    opacity: 1;
  }

  .close:hover {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
    color: var(--rb-text);
  }

  .quiet {
    margin: 4px 8px;
    color: var(--rb-faint);
    font-size: 12px;
  }

  .recent {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 1px;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
  }

  .heading {
    margin: 12px 8px 4px;
    color: var(--rb-faint);
    font-size: 11px;
    font-weight: 600;
  }

  .heading:first-child {
    margin-top: 4px;
  }

  .thread {
    display: flex;
    align-items: center;
    gap: 9px;
    border: 0;
    border-radius: 8px;
    padding: 5px 8px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }

  .thread:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
  }

  .thread-copy {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .thread-text,
  .thread-folder {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .thread-folder {
    color: var(--rb-faint);
    font-size: 11px;
  }

  .thread-when {
    flex-shrink: 0;
    color: var(--rb-faint);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }

  @media (prefers-reduced-motion: reduce) {
    .dot.is-working {
      animation: none;
    }
  }
</style>
