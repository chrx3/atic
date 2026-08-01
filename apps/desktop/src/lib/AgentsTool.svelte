<script lang="ts">
  import FloatingAgents from "$lib/floating-v2/FloatingAgents.svelte";
</script>

<div class="agents-center">
  <FloatingAgents />
</div>

<style>
  .agents-center {
    --pearl-ink: var(--rb-text);
    --pearl-muted: var(--rb-muted);
    --pearl-line: var(--rb-hairline);
    --pearl-soft: color-mix(in oklch, var(--rb-text) 5.5%, transparent);
    --pearl-signal: oklch(0.62 0.18 252);
    display: flex;
    min-height: 28rem;
    flex: 1;
    overflow: hidden;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  @container atic-main (max-width: 36.999rem) {
    .agents-center {
      min-height: 34rem;
      border-radius: var(--rb-radius-sm);
    }
  }
</style>
