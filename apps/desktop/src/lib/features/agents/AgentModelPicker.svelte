<script lang="ts">
  /**
   * Un solo selector para agente y modelo.
   *
   * Dos niveles: arriba los agentes instalados, como pestañas; abajo los
   * modelos del que esté elegido. Uno debajo de otro no se podía recorrer:
   * OpenCode sola trae decenas. Elegir un modelo del agente actual lo cambia
   * en la sesión; uno de otro agente es arrancar con él, y eso lo decide el
   * panel.
   *
   * Los modelos del agente actual son los que informó su sesión; los de los
   * demás se piden al mirarlos (`agent_list_models`, cacheado en Rust). Un
   * agente que no sabe listarlos se elige igual, con su modelo por defecto.
   */
  import { untrack } from "svelte";
  import { agentListModels } from "$ipc/agents";
  import type { AgentModel } from "$lib/types";
  import { t } from "$domain/i18n.svelte";
  import AgentLogo from "./AgentLogo.svelte";
  import ChatPopover from "./ChatPopover.svelte";
  import { AGENTS, type AgentDef } from "./agentCatalog";
  import { matchModel, modelGroups, shortModelName } from "./chatThread";

  let {
    choices,
    backendId,
    modelId,
    sessionModels,
    open,
    onToggle,
    onPickModel,
    onPickAgent,
  }: {
    choices: AgentDef[];
    backendId: string;
    modelId: string;
    sessionModels: AgentModel[];
    open: boolean;
    onToggle: (open: boolean) => void;
    onPickModel: (id: string) => void;
    onPickAgent: (agent: AgentDef, modelId?: string) => void;
  } = $props();

  /** Con más de esto la lista pide búsqueda. */
  const SEARCH_FROM = 8;

  /** Modelos de los otros agentes: `null` mientras se piden, `[]` si no hay. */
  let listed = $state<Record<string, AgentModel[] | null>>({});
  /** La pestaña que se está mirando; al abrir, la del agente actual. */
  let viewing = $state("");
  let query = $state("");

  /**
   * Lo que se ofrece. El actual va siempre, aunque la lista no lo traiga: un
   * selector que no se encuentra a sí mismo muestra el id crudo y abre vacío.
   */
  const agentsShown = $derived.by(() => {
    const base = choices.length > 0 ? choices : AGENTS;
    const current = AGENTS.find((a) => a.backend === backendId);
    return current && !base.includes(current) ? [current, ...base] : base;
  });
  const current = $derived(agentsShown.find((a) => a.backend === backendId));
  const currentModel = $derived(matchModel(sessionModels, modelId));
  const triggerModel = $derived(shortModelName(currentModel?.name ?? modelId ?? ""));

  const viewed = $derived(agentsShown.find((a) => a.backend === viewing) ?? current);
  const viewedModels = $derived.by((): AgentModel[] | null => {
    if (!viewed) return [];
    if (viewed.backend === backendId && sessionModels.length > 0) return sessionModels;
    return listed[viewed.backend] ?? null;
  });
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const models = viewedModels ?? [];
    if (!q) return models;
    return models.filter((m) => `${m.name} ${m.id}`.toLowerCase().includes(q));
  });

  /**
   * Los modelos agrupados por proveedor (`proveedor/modelo`), con el del
   * modelo en uso primero. OpenCode mezcla cientos de varios proveedores, y
   * los de uno sin credenciales fallan al primer mensaje: lo que ya funciona
   * tiene que quedar arriba.
   */
  const groups = $derived(
    modelGroups(filtered, viewed?.backend === backendId ? modelId : ""),
  );

  $effect(() => {
    if (!open) return;
    untrack(() => {
      viewing = backendId;
      query = "";
    });
  });

  $effect(() => {
    // Solo la pestaña mirada: pedir los de todos al abrir levantaba un
    // proceso por agente para mostrar una lista.
    const backend = viewed?.backend;
    if (!open || !backend) return;
    if (backend === backendId && sessionModels.length > 0) return;
    if (backend in untrack(() => listed)) return;
    listed = { ...listed, [backend]: null };
    void agentListModels(backend)
      .then((models) => (listed = { ...listed, [backend]: models }))
      .catch(() => (listed = { ...listed, [backend]: [] }));
  });

  function pick(agent: AgentDef, model?: AgentModel) {
    onToggle(false);
    if (agent.backend === backendId) {
      if (model && model.id !== currentModel?.id) onPickModel(model.id);
      return;
    }
    onPickAgent(agent, model?.id);
  }
</script>

<ChatPopover {open} {onToggle} label={t("page.agents.chat.agentModel")} width={320}>
  {#snippet trigger()}
    <AgentLogo agent={current?.cli ?? backendId} size={14} />
    <span class="trigger-text" title={current?.name}>
      {triggerModel || current?.name || backendId}
    </span>
    <span class="caret" aria-hidden="true"></span>
  {/snippet}

  <div class="sticky">
    <div class="tabs" role="tablist">
      {#each agentsShown as agent (agent.backend)}
        <button
          type="button"
          role="tab"
          class="tab"
          aria-selected={agent.backend === viewed?.backend}
          title={agent.name}
          onclick={() => {
            viewing = agent.backend;
            query = "";
          }}
        >
          <AgentLogo agent={agent.cli} size={15} />
          {#if agent.backend === backendId}
            <span class="tab-dot" aria-hidden="true"></span>
          {/if}
        </button>
      {/each}
    </div>
    {#if viewed}
      <div class="viewed">
        <span class="viewed-name">{viewed.name}</span>
        {#if viewed.backend === backendId}
          <span class="viewed-tag">{t("page.agents.chat.currentAgent")}</span>
        {:else}
          <button type="button" class="use" onclick={() => pick(viewed)}>
            {t("page.agents.chat.useAgent")}
          </button>
        {/if}
      </div>
    {/if}
    {#if (viewedModels?.length ?? 0) > SEARCH_FROM}
      <input
        class="search"
        type="search"
        placeholder={t("page.agents.chat.searchModels")}
        bind:value={query}
      />
    {/if}
  </div>

  {#if viewedModels === null}
    <p class="quiet">{t("page.agents.chat.loadingModels")}</p>
  {:else if viewedModels.length === 0}
    <p class="quiet">{t("page.agents.chat.noModels")}</p>
  {:else}
    {#each groups as group (group.provider)}
      {#if groups.length > 1}
        <p class="provider">{group.provider || viewed?.name}</p>
      {/if}
      {#each group.models as model (model.id)}
        {@const selected =
          viewed?.backend === backendId && model.id === currentModel?.id}
        <button
          type="button"
          class="model"
          class:is-selected={selected}
          aria-pressed={selected}
          title={model.id}
          onclick={() => viewed && pick(viewed, model)}
        >
          <span class="model-name">{shortModelName(model.name || model.id)}</span>
          {#if model.description}
            <span class="model-desc">{model.description}</span>
          {/if}
        </button>
      {/each}
    {/each}
  {/if}
</ChatPopover>

<style>
  .trigger-text {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-text);
    font-weight: 560;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .caret {
    flex-shrink: 0;
    width: 5px;
    height: 5px;
    margin-left: 2px;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    transform: translateY(-2px) rotate(45deg);
  }

  /* Pestañas, agente y búsqueda quedan quietos; lo que corre es la lista. */
  .sticky {
    position: sticky;
    top: -6px;
    z-index: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: -6px -6px 4px;
    padding: 6px 6px 6px;
    background: var(--rb-surface-elevated, var(--rb-surface));
    box-shadow: 0 1px 0 color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .tabs {
    display: flex;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tab {
    position: relative;
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 34px;
    height: 30px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    cursor: pointer;
    opacity: 0.6;
    transition:
      background-color 120ms ease,
      opacity 120ms ease;
  }

  .tab:hover {
    opacity: 1;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .tab[aria-selected="true"] {
    opacity: 1;
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  /* El agente de esta ficha, aunque se esté mirando otro. */
  .tab-dot {
    position: absolute;
    right: 5px;
    bottom: 4px;
    width: 5px;
    height: 5px;
    border-radius: 999px;
    background: var(--accent);
  }

  .viewed {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 0 4px 0 8px;
  }

  .viewed-name {
    font-size: 12.5px;
    font-weight: 600;
  }

  .viewed-tag {
    margin-left: auto;
    color: var(--rb-faint);
    font-size: 11px;
  }

  .use {
    margin-left: auto;
    border: 0;
    border-radius: 7px;
    padding: 4px 10px;
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .search {
    box-sizing: border-box;
    width: 100%;
    border: 0;
    border-radius: 8px;
    padding: 6px 10px;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12px;
    outline: 0;
  }

  .search:focus {
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--accent) 45%, transparent);
  }

  .model {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    border: 0;
    border-radius: 7px;
    padding: 6px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .model:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .model.is-selected {
    background: color-mix(in sRGB, var(--accent) 14%, transparent);
  }

  .model-name {
    overflow: hidden;
    font-size: 12.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .model-desc {
    overflow: hidden;
    color: var(--rb-faint);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .provider {
    margin: 8px 8px 2px;
    color: var(--rb-faint);
    font-size: 11px;
    font-weight: 600;
  }

  .quiet {
    margin: 0;
    padding: 8px;
    color: var(--rb-faint);
    font-size: 12px;
  }
</style>
