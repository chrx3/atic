<script lang="ts">
  /**
   * El estado de una consola en un punto: late mientras trabaja, se marca
   * cuando espera un permiso y se pone verde cuando respondió sin que se la
   * mirara. Con `label` dice además qué pasa; lista y callada no dice nada.
   */
  import { t } from "$domain/i18n.svelte";
  import type { ConsoleState } from "./consoleStatus";

  let {
    status,
    attention,
    label = false,
  }: {
    status: ConsoleState;
    /** Terminó un turno fuera de la vista y todavía no se miró. */
    attention: boolean;
    label?: boolean;
  } = $props();

  const tone = $derived(
    status === "ready" && attention ? "unread" : (status as ConsoleState | "unread"),
  );
  const text = $derived(t(`page.agents.window.status.${tone}`));
  const loud = $derived(tone !== "ready");
</script>

<span class="state is-{tone}" title={text}>
  {#if tone !== "ended"}
    <span class="dot" aria-hidden="true"></span>
  {/if}
  {#if label && loud}
    <span class="text">{text}</span>
  {:else}
    <span class="sr">{text}</span>
  {/if}
</span>

<style>
  .state {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: 6px;
    color: var(--rb-muted);
    font-size: 11px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: color-mix(in sRGB, var(--rb-text) 22%, transparent);
  }

  .is-working .dot {
    background: var(--accent);
    animation: pulse 1.2s ease-in-out infinite;
  }

  .is-waiting {
    color: var(--rb-text);
  }

  .is-waiting .dot {
    background: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in sRGB, var(--accent) 30%, transparent);
  }

  .is-unread {
    color: var(--rb-text);
  }

  .is-unread .dot {
    background: var(--rb-ok);
  }

  .is-ended {
    color: var(--rb-faint);
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .is-working .dot {
      animation: none;
    }
  }
</style>
