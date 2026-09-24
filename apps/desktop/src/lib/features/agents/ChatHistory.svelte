<script lang="ts">
  /**
   * El historial de conversaciones, a mano desde el composer.
   *
   * Todas las de todos los agentes: el chat es multi-agente, y buscar «lo que
   * hablé ayer sobre el login» no debería exigir acordarse de con quién. Las
   * que no se pueden retomar de forma fiable se ven, pero no se abren.
   */
  import { agentThreads } from "$ipc/agents";
  import type { StoredThread } from "$lib/types";
  import { t } from "$domain/i18n.svelte";
  import { formatListWhen } from "$core/format";
  import Icon from "$ui/Icon.svelte";
  import { History } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import ChatPopover from "./ChatPopover.svelte";
  import { AGENTS } from "./agentCatalog";
  import { canResume } from "./chatResume";

  let {
    open,
    onToggle,
    onPick,
  }: {
    open: boolean;
    onToggle: (open: boolean) => void;
    onPick: (thread: StoredThread) => void;
  } = $props();

  const SHOWN_MAX = 60;
  let threads = $state<StoredThread[] | null>(null);
  let query = $state("");

  $effect(() => {
    if (!open) return;
    query = "";
    threads = null;
    void agentThreads()
      // Sin texto del usuario no hay nada que reconocer ni que retomar.
      .then((list) => (threads = list.filter((th) => !th.parent && th.preview.trim())))
      .catch(() => (threads = []));
  });

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = threads ?? [];
    const hits = q
      ? list.filter((th) => `${th.preview} ${th.cwd}`.toLowerCase().includes(q))
      : list;
    return hits.slice(0, SHOWN_MAX);
  });

  function cliOf(backendId: string): string {
    return AGENTS.find((a) => a.backend === backendId)?.cli ?? backendId;
  }

  function folderName(path: string): string {
    return (
      path
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() ?? path
    );
  }

  function pick(thread: StoredThread) {
    onToggle(false);
    onPick(thread);
  }
</script>

<ChatPopover {open} {onToggle} label={t("page.agents.chat.history")} width={340}>
  {#snippet trigger()}
    <Icon icon={History} size={14} />
  {/snippet}

  <input
    class="search"
    type="search"
    placeholder={t("page.agents.chat.historySearch")}
    bind:value={query}
  />
  {#if threads === null}
    <p class="quiet">{t("page.agents.chat.loading")}</p>
  {:else if shown.length === 0}
    <p class="quiet">{t("page.agents.chat.historyEmpty")}</p>
  {:else}
    {#each shown as thread (thread.id)}
      {@const ok = canResume(thread)}
      <button
        type="button"
        class="row"
        disabled={!ok}
        title={ok
          ? undefined
          : t("page.agents.chat.cannotResume", { name: thread.backendName })}
        onclick={() => pick(thread)}
      >
        <AgentLogo agent={cliOf(thread.backendId)} size={14} />
        <span class="text">
          <span class="preview">{thread.preview}</span>
          <span class="meta">
            {thread.cwd ? folderName(thread.cwd) : thread.backendName} · {formatListWhen(
              thread.updatedAt,
            )}
          </span>
        </span>
      </button>
    {/each}
  {/if}
</ChatPopover>

<style>
  .search {
    box-sizing: border-box;
    width: 100%;
    margin-bottom: 4px;
    border: 0;
    border-radius: 8px;
    padding: 7px 10px;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    outline: 0;
  }

  .search:focus {
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--accent) 45%, transparent);
  }

  .quiet {
    margin: 0;
    padding: 8px;
    color: var(--rb-faint);
    font-size: 12px;
  }

  .row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    width: 100%;
    border: 0;
    border-radius: 8px;
    padding: 7px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row :global(.agent-logo) {
    margin-top: 2px;
  }

  .row:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .row:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .preview {
    overflow: hidden;
    font-size: 12.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
</style>
