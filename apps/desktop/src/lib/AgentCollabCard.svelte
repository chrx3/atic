<script lang="ts">
  import type { ToolStatus } from "$lib/types";

  let {
    name,
    title,
    subagentType,
    status,
    summary,
  }: {
    name: string;
    title: string;
    subagentType: string;
    status: ToolStatus;
    summary: string;
  } = $props();

  let open = $state(false);

  const done = $derived(status === "completed" || status === "failed");
  const failed = $derived(status === "failed");
  const displayTitle = $derived(title || name);
</script>

<div class="collab" class:is-failed={failed}>
  <button
    type="button"
    class="collab-head"
    onclick={() => (open = !open)}
    aria-expanded={open}
  >
    <span
      class="collab-status"
      class:is-running={!done}
      class:is-done={done && !failed}
      class:is-failed={failed}
    ></span>
    <span class="collab-copy">
      <span class="collab-kind">Subagente · {subagentType}</span>
      <span class="collab-title">{displayTitle}</span>
    </span>
    <span class="collab-caret" aria-hidden="true">{open ? "⌃" : "⌄"}</span>
  </button>

  {#if (done || open) && summary}
    <p class="collab-summary">{summary}</p>
  {/if}
</div>

<style>
  .collab {
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--card);
    overflow: hidden;
  }
  .collab.is-failed {
    border-color: color-mix(in srgb, var(--del) 55%, var(--line));
  }

  .collab-head {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.5rem;
    border: 0;
    padding: 0.42rem 0.6rem;
    background: transparent;
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    text-align: left;
    cursor: pointer;
  }
  .collab-head:hover {
    background: var(--hover);
  }

  .collab-status {
    width: 0.4rem;
    height: 0.4rem;
    flex-shrink: 0;
    border-radius: 999px;
    background: var(--faint);
  }
  .collab-status.is-running {
    background: var(--coral);
    animation: collab-pulse 1.4s ease-in-out infinite;
  }
  .collab-status.is-done {
    background: var(--add);
  }
  .collab-status.is-failed {
    background: var(--del);
  }

  .collab-copy {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.08rem;
  }
  .collab-kind {
    color: var(--faint);
    font-size: 0.625rem;
  }
  .collab-title {
    overflow: hidden;
    color: var(--text);
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .collab-caret {
    flex-shrink: 0;
    color: var(--faint);
    font-size: 0.625rem;
  }

  .collab-summary {
    margin: 0;
    border-top: 1px solid var(--line);
    padding: 0.45rem 0.6rem 0.5rem;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  @keyframes collab-pulse {
    0%,
    100% {
      opacity: 0.3;
    }
    50% {
      opacity: 1;
    }
  }
</style>
