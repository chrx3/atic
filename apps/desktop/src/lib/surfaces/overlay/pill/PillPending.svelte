<script module lang="ts">
  import type { PendingPermission } from "$lib/agentSessions.svelte";

  /** Un pendiente de algún agente, con de quién es. */
  export type PendingItem = {
    sessionId: string;
    backendId: string;
    agentName: string;
    permission: PendingPermission;
  };
</script>

<script lang="ts">
  /**
   * Lo que los agentes esperan de ti, junto a la pill: permisos, preguntas y
   * planes, de todos los agentes a la vez.
   *
   * Antes la pill mostraba el primer permiso del primer agente y nada más, y
   * no sabía contestar preguntas. Acá se pagina entre todos («1 / 3 · Codex»)
   * con las mismas tarjetas del chat, y «Abrir» lleva a esa sesión en la
   * ventana de agentes.
   */
  import type { PermissionDecision } from "$core/types";
  import { t } from "$domain/i18n.svelte";
  import AgentLogo from "$features/agents/AgentLogo.svelte";
  import ChatPermission from "$features/agents/ChatPermission.svelte";
  import ChatPlan from "$features/agents/ChatPlan.svelte";
  import ChatQuestion from "$features/agents/ChatQuestion.svelte";
  import {
    QUESTION_TOOL,
    parseQuestions,
    withAnswers,
  } from "$features/agents/chatQuestions";

  let {
    items,
    busy = false,
    onDecide,
    onAnswer,
    onOpen,
  }: {
    items: PendingItem[];
    busy?: boolean;
    onDecide: (item: PendingItem, decision: PermissionDecision) => void;
    onAnswer: (item: PendingItem, updatedInput: unknown) => void;
    onOpen: (item: PendingItem) => void;
  } = $props();

  let index = $state(0);
  const at = $derived(Math.min(index, Math.max(items.length - 1, 0)));
  const item = $derived(items[at] ?? null);
  const questions = $derived(
    item?.permission.tool === QUESTION_TOOL
      ? parseQuestions(item.permission.input)
      : null,
  );
  const isPlan = $derived(item?.permission.tool === "ExitPlanMode");

  function move(step: number) {
    if (items.length < 2) return;
    index = (at + step + items.length) % items.length;
  }
</script>

{#if item}
  <div class="pending" data-no-drag>
    <header class="head">
      <AgentLogo agent={item.backendId} size={14} />
      <span class="who">{item.agentName}</span>
      {#if items.length > 1}
        <span class="pager">
          <button
            type="button"
            aria-label={t("page.agents.chat.previous")}
            onclick={() => move(-1)}>‹</button
          >
          {at + 1} / {items.length}
          <button
            type="button"
            aria-label={t("page.agents.chat.next")}
            onclick={() => move(1)}>›</button
          >
        </span>
      {/if}
      <button type="button" class="open" onclick={() => onOpen(item)}>
        {t("pill.pendingOpen")}
      </button>
    </header>

    {#key item.permission.id}
      {#if questions}
        <ChatQuestion
          {questions}
          agentName={item.agentName}
          {busy}
          allowOwn={item.backendId !== "cursor"}
          onAnswer={(picked, written) =>
            onAnswer(
              item,
              withAnswers(item.permission.input, questions, picked, written),
            )}
          onSkip={() => onDecide(item, "deny")}
        />
      {:else if isPlan}
        <ChatPlan
          input={item.permission.input}
          agentName={item.agentName}
          {busy}
          onDecide={(decision) => onDecide(item, decision)}
        />
      {:else}
        <ChatPermission
          permission={item.permission}
          agentName={item.agentName}
          {busy}
          onDecide={(decision) => onDecide(item, decision)}
        />
      {/if}
    {/key}
  </div>
{/if}

<style>
  .pending {
    /* Los tokens del chat, para que sus tarjetas se vean igual acá. */
    --coral: var(--accent);
    --text: var(--rb-text);
    --dim: var(--rb-muted);
    --faint: var(--rb-faint);
    --line: var(--rb-border);
    --card: var(--rb-surface-2);
    --code: var(--rb-surface-2);

    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: min(460px, 70vh);
    overflow-y: auto;
    border-radius: 16px;
    padding: 10px;
    background: var(--rb-surface);
    color: var(--rb-text);
    font-family: var(--rb-font);
    font-size: 13px;
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 10%, transparent),
      0 16px 40px -16px rgba(0, 0, 0, 0.55);
  }

  /* El plan se lee entero en la ventana: acá, lo justo para decidir. */
  .pending :global(.plan .body) {
    max-height: 160px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 2px;
  }

  .who {
    min-width: 0;
    overflow: hidden;
    font-size: 12.5px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pager {
    display: flex;
    align-items: center;
    gap: 2px;
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .pager button,
  .open {
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    cursor: pointer;
  }

  .pager button {
    width: 22px;
    height: 22px;
    font-size: 14px;
  }

  .open {
    margin-left: auto;
    padding: 3px 8px;
    font-size: 11.5px;
  }

  .pager button:hover,
  .open:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }
</style>
