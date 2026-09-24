<script lang="ts">
  /**
   * Un tramo de herramientas del agente, plegado en una línea.
   *
   * Lo que se quiere saber casi siempre es «qué hizo y si salió bien», no la
   * salida de cada lectura. Plegado dice eso; en vivo muestra además lo que
   * está haciendo ahora, que es lo que responde a «¿se colgó?».
   */
  import AgentCollabCard from "$lib/AgentCollabCard.svelte";
  import AgentToolCard from "$lib/AgentToolCard.svelte";
  import { t } from "$domain/i18n.svelte";
  import EditedFiles from "./EditedFiles.svelte";
  import {
    activityTabOf,
    activityTabs,
    countActivity,
    editedFiles,
    type ActivityItem,
    type ActivityTab,
  } from "./chatThread";

  let { items, live }: { items: ActivityItem[]; live: boolean } = $props();

  let open = $state(false);
  /**
   * Un tramo largo mezcla lecturas, ediciones y comandos: con pestañas se
   * mira una clase a la vez. Con una sola clase no hay pestañas.
   */
  let tab = $state<ActivityTab>("all");
  const tabs = $derived(activityTabs(items));
  const shown = $derived(
    tab === "all" ? items : items.filter((item) => activityTabOf(item) === tab),
  );

  const counts = $derived(countActivity(items));
  /** Solo al terminar el tramo: en vivo la lista cambiaría bajo el ojo. */
  const files = $derived(live ? [] : editedFiles(items));

  const summary = $derived.by(() => {
    const parts: string[] = [];
    if (counts.read) parts.push(t("page.agents.chat.act.read", { n: counts.read }));
    if (counts.edit) parts.push(t("page.agents.chat.act.edit", { n: counts.edit }));
    if (counts.run) parts.push(t("page.agents.chat.act.run", { n: counts.run }));
    if (counts.search)
      parts.push(t("page.agents.chat.act.search", { n: counts.search }));
    if (counts.agent) parts.push(t("page.agents.chat.act.agent", { n: counts.agent }));
    if (counts.other) parts.push(t("page.agents.chat.act.other", { n: counts.other }));
    if (parts.length === 0 && counts.thought)
      parts.push(t("page.agents.chat.act.thought"));
    const text = parts.join(" · ");
    return text.charAt(0).toUpperCase() + text.slice(1);
  });

  /** Lo último que está haciendo, mientras el tramo sigue vivo. */
  const now = $derived.by(() => {
    if (!live) return null;
    const last = items[items.length - 1];
    if (!last) return null;
    if (last.kind === "reasoning") return t("page.agents.chat.act.thinking");
    return last.title || last.name;
  });
</script>

<div class="activity" class:is-open={open} class:is-live={live}>
  <button
    type="button"
    class="head"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="dot" class:is-bad={counts.failed > 0} aria-hidden="true"></span>
    <span class="summary">{summary}</span>
    {#if counts.failed > 0}
      <span class="failed"
        >{t("page.agents.chat.act.failed", { n: counts.failed })}</span
      >
    {/if}
    {#if now}
      <span class="now">{now}</span>
    {/if}
    <span class="caret" aria-hidden="true"></span>
  </button>

  {#if files.length > 0}
    <EditedFiles {files} />
  {/if}

  {#if open}
    <div class="body">
      {#if tabs.length > 0}
        <div class="tabs" role="tablist">
          {#each tabs as entry (entry.id)}
            <button
              type="button"
              role="tab"
              class="tab"
              aria-selected={tab === entry.id}
              onclick={() => (tab = entry.id)}
            >
              {t(`page.agents.chat.actTab.${entry.id}`)}
              <span class="tab-count">{entry.count}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#each shown as item (item.id)}
        {#if item.kind === "tool"}
          <AgentToolCard
            name={item.name}
            title={item.title}
            toolKind={item.toolKind}
            input={item.input}
            output={item.output}
            status={item.status}
            locations={item.locations}
          />
        {:else if item.kind === "collab"}
          <AgentCollabCard
            name={item.name}
            title={item.title}
            subagentType={item.subagentType}
            status={item.status}
            summary={item.summary}
          />
        {:else if item.text.trim()}
          <p class="thought">{item.text}</p>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .activity {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    width: fit-content;
    max-width: 100%;
    border: 0;
    border-radius: 8px;
    margin-left: -8px;
    padding: 4px 8px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .head:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
  }

  .dot {
    flex-shrink: 0;
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--rb-ok) 80%, transparent);
  }

  .dot.is-bad {
    background: var(--rb-record);
  }

  /* En vivo: el punto late. Es la única señal de avance cuando está plegado. */
  .is-live .dot {
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .summary {
    flex-shrink: 0;
    font-weight: 560;
  }

  .failed {
    flex-shrink: 0;
    color: var(--rb-record);
  }

  .now {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-faint);
    font-family: var(--rb-mono);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .caret {
    flex-shrink: 0;
    width: 6px;
    height: 6px;
    margin-left: 2px;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    opacity: 0.6;
    transform: translateY(-2px) rotate(45deg);
    transition: transform 160ms cubic-bezier(0.2, 0, 0, 1);
  }

  .is-open .caret {
    transform: translateY(1px) rotate(-135deg);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 4px;
    border-left: 1px solid var(--rb-border);
    margin-left: 2px;
    padding-left: 12px;
  }

  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    margin-bottom: 2px;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 0;
    border-radius: 7px;
    padding: 3px 8px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .tab:hover {
    color: var(--rb-text);
  }

  .tab[aria-selected="true"] {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
    color: var(--rb-text);
  }

  .tab-count {
    color: var(--rb-faint);
    font-variant-numeric: tabular-nums;
  }

  .thought {
    margin: 0;
    color: var(--rb-muted);
    font-size: 12px;
    font-style: italic;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  @media (prefers-reduced-motion: reduce) {
    .is-live .dot {
      animation: none;
    }
  }
</style>
