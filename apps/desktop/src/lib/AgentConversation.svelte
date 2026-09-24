<script lang="ts">
  /**
   * La conversación: los bloques de un hilo, en orden.
   *
   * Lo tuyo va en burbuja a la derecha; lo del agente, como texto corrido a
   * todo el ancho de la columna, porque es lo que se lee. Las herramientas
   * llegan ya juntas en bloques de actividad (`chatThread.toBlocks`): acá no
   * se decide qué agrupar, solo cómo se ve.
   *
   * No sabe de sesiones, permisos pendientes ni composer: eso es del panel.
   */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import AgentMessage from "$lib/AgentMessage.svelte";
  import AgentConversation from "$lib/AgentConversation.svelte";
  import ChatActivity from "$lib/features/agents/ChatActivity.svelte";
  import ChatWork from "$lib/features/agents/ChatWork.svelte";
  import type { ChatBlock } from "$lib/features/agents/chatThread";
  import { t } from "$domain/i18n.svelte";

  let { blocks }: { blocks: ChatBlock[] } = $props();

  const SUMMARY_PREFIX = "Resumen del contexto";
</script>

{#each blocks as block (block.kind === "activity" || block.kind === "work" ? block.id : block.item.id)}
  {#if block.kind === "user"}
    <div class="user">
      {#if block.item.origin?.files?.length}
        <div class="shots">
          {#each block.item.origin.files as file (file)}
            <img src={convertFileSrc(file)} alt="" draggable="false" />
          {/each}
        </div>
      {/if}
      {#if block.item.text.trim()}
        <div class="bubble">
          <AgentMessage text={block.item.text} />
        </div>
      {/if}
      {#if block.item.origin?.via}
        <!-- Por dónde entró: dictado, captura, portapapeles. Es lo propio de
             Atic; en otra GUI todo vendría del teclado. -->
        <span class="via">{block.item.origin.via}</span>
      {/if}
    </div>
  {:else if block.kind === "text"}
    <div class="text" class:is-live={block.item.streaming}>
      <AgentMessage text={block.item.text} />
    </div>
  {:else if block.kind === "activity"}
    <ChatActivity items={block.items} live={block.live} />
  {:else if block.kind === "work"}
    <ChatWork
      durationMs={block.durationMs}
      status={block.status}
      costUsd={block.costUsd}
      files={block.files}
    >
      <AgentConversation blocks={block.blocks} />
    </ChatWork>
  {:else if block.kind === "plan"}
    <div class="plan">
      <p class="plan-h">{t("page.agents.chat.plan")}</p>
      {#each block.item.entries as entry, i (i)}
        <div class="plan-e" data-s={entry.status}>
          <span class="plan-b" aria-hidden="true"></span>
          <span>{entry.text}</span>
        </div>
      {/each}
    </div>
  {:else if block.item.text.startsWith(SUMMARY_PREFIX)}
    <div class="summary" role="note">
      <p class="summary-h">{t("page.agents.chat.contextSummary")}</p>
      <p class="summary-b">{block.item.text.slice(SUMMARY_PREFIX.length).trim()}</p>
    </div>
  {:else}
    <p class="notice">{block.item.text}</p>
  {/if}
{/each}

<style>
  .user {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    margin: 6px 0 2px;
  }

  .bubble {
    max-width: min(85%, 560px);
    border-radius: 16px 16px 4px 16px;
    padding: 8px 12px;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    overflow-wrap: anywhere;
  }

  .shots {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
  }

  .shots img {
    max-width: 180px;
    max-height: 120px;
    border-radius: 10px;
    object-fit: cover;
    outline: 1px solid rgba(255, 255, 255, 0.1);
    outline-offset: -1px;
  }

  :global([data-theme-base="light"]) .shots img {
    outline-color: rgba(0, 0, 0, 0.1);
  }

  .via {
    color: var(--rb-faint);
    font-size: 11px;
  }

  .text {
    text-wrap: pretty;
  }

  /* Texto en vivo: el cursor al final dice que sigue escribiendo. */
  .text.is-live :global(.md > :last-child)::after {
    content: "";
    display: inline-block;
    width: 0.45em;
    height: 1em;
    margin-left: 2px;
    border-radius: 1px;
    background: var(--accent);
    vertical-align: text-bottom;
    animation: blink 1s steps(2, start) infinite;
  }

  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  .plan {
    display: flex;
    flex-direction: column;
    gap: 4px;
    border-radius: 12px;
    padding: 10px 12px;
    background: color-mix(in sRGB, var(--rb-text) 4%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .plan-h {
    margin: 0 0 2px;
    color: var(--rb-muted);
    font-size: 11px;
    font-weight: 600;
  }

  .plan-e {
    display: flex;
    align-items: baseline;
    gap: 8px;
    color: var(--rb-muted);
    font-size: 12.5px;
  }

  .plan-b {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    box-shadow: inset 0 0 0 1.5px var(--rb-faint);
    transform: translateY(-1px);
  }

  .plan-e[data-s="in_progress"] {
    color: var(--rb-text);
  }

  .plan-e[data-s="in_progress"] .plan-b {
    box-shadow: inset 0 0 0 1.5px var(--accent);
    background: color-mix(in sRGB, var(--accent) 35%, transparent);
  }

  .plan-e[data-s="completed"] {
    color: var(--rb-faint);
    text-decoration: line-through;
  }

  .plan-e[data-s="completed"] .plan-b {
    box-shadow: none;
    background: var(--rb-ok);
  }

  .summary {
    border-radius: 12px;
    padding: 10px 12px;
    background: color-mix(in sRGB, var(--accent) 7%, transparent);
  }

  .summary-h {
    margin: 0 0 4px;
    color: var(--rb-muted);
    font-size: 11px;
    font-weight: 600;
  }

  .summary-b {
    margin: 0;
    color: var(--rb-muted);
    font-size: 12.5px;
    white-space: pre-wrap;
  }

  .notice {
    margin: 0;
    color: var(--rb-faint);
    font-size: 12px;
  }

  @media (prefers-reduced-motion: reduce) {
    .text.is-live :global(.md > :last-child)::after {
      animation: none;
    }
  }
</style>
