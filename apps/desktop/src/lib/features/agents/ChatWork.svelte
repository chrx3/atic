<script lang="ts">
  /**
   * Lo que un turno hizo antes de responder, plegado en una línea con cuánto
   * tardó. Plegado, el hilo se lee como pregunta → respuesta; lo que cambió
   * se sigue viendo, porque es lo que se quiere revisar sin abrir nada.
   */
  import type { Snippet } from "svelte";
  import type { TurnStatus } from "$lib/types";
  import { t } from "$domain/i18n.svelte";
  import EditedFiles from "./EditedFiles.svelte";
  import { formatDuration, type EditedFile } from "./chatThread";

  let {
    durationMs,
    status,
    costUsd,
    files,
    children,
  }: {
    durationMs: number;
    status: TurnStatus;
    costUsd: number | null;
    files: EditedFile[];
    children: Snippet;
  } = $props();

  let open = $state(false);

  const label = $derived.by(() => {
    const time = formatDuration(durationMs);
    if (status === "cancelled")
      return t("page.agents.chat.work.cancelled", { t: time });
    if (status === "failed") return t("page.agents.chat.work.failed", { t: time });
    return t("page.agents.chat.work.done", { t: time });
  });
  const cost = $derived(costUsd ? `US$ ${costUsd.toFixed(2)}` : "");
</script>

<div class="work" class:is-open={open}>
  <button
    type="button"
    class="head"
    class:is-bad={status === "failed"}
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="label">{label}</span>
    {#if cost}<span class="cost">{cost}</span>{/if}
    <span class="caret" aria-hidden="true"></span>
    <span class="rule" aria-hidden="true"></span>
  </button>

  {#if open}
    <div class="body">
      {@render children()}
    </div>
  {:else if files.length > 0}
    <EditedFiles {files} />
  {/if}
</div>

<style>
  .work {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    border: 0;
    padding: 2px 0;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition: color var(--duration-fast) ease;
  }

  .head:hover {
    color: var(--rb-text);
  }

  .head.is-bad .label {
    color: var(--rb-record);
  }

  .label {
    flex-shrink: 0;
    font-weight: 560;
    font-variant-numeric: tabular-nums;
  }

  .cost {
    flex-shrink: 0;
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .caret {
    flex-shrink: 0;
    width: 5px;
    height: 5px;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    opacity: 0.6;
    transform: rotate(-45deg);
    transition: transform var(--duration-slow) var(--ease-calm);
  }

  .is-open .caret {
    transform: translateY(-1px) rotate(45deg);
  }

  /* La línea que sigue al texto separa el trabajo de la respuesta. */
  .rule {
    flex: 1;
    height: 1px;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    border-left: 1px solid var(--rb-border);
    margin-left: 2px;
    padding-left: 14px;
  }

  @media (prefers-reduced-motion: reduce) {
    .caret {
      transition: none;
    }
  }
</style>
