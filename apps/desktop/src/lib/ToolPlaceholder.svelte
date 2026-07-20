<script lang="ts">
  import type { ToolDef } from "$lib/tools";
  import type { Snippet } from "svelte";

  let {
    tool,
    badge = "Pronto",
    children,
  }: {
    tool: ToolDef;
    badge?: string;
    children?: Snippet;
  } = $props();
</script>

<section class="atic-tool-panel" aria-label={tool.label}>
  <header class="atic-tool-head">
    <div>
      <p class="atic-tool-kicker">{badge}</p>
      <h2>{tool.label}</h2>
      <p class="atic-tool-blurb">{tool.blurb}</p>
    </div>
  </header>
  <div class="atic-tool-body">
    {#if children}
      {@render children()}
    {:else}
      <p class="atic-tool-empty">
        Esta herramienta formará parte del kit Atic. La dejamos aquí para ir
        armando el producto sin mezclarlo con reuniones.
      </p>
    {/if}
  </div>
</section>

<style>
  .atic-tool-panel {
    display: flex;
    min-height: 100%;
    flex-direction: column;
    gap: 1rem;
    padding: 1.1rem 1.15rem 1.25rem;
  }

  .atic-tool-head h2 {
    margin: 0.15rem 0 0.35rem;
    font-family: var(--rb-display);
    font-size: 1.35rem;
    font-weight: 650;
    letter-spacing: -0.03em;
  }

  .atic-tool-kicker {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .atic-tool-blurb {
    margin: 0;
    max-width: 36rem;
    color: var(--rb-muted);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .atic-tool-body {
    flex: 1 1 auto;
  }

  .atic-tool-empty {
    margin: 0;
    max-width: 28rem;
    padding: 1rem 1.05rem;
    border: 1px dashed var(--rb-border-strong);
    border-radius: var(--rb-radius);
    color: var(--rb-muted);
    background: color-mix(in srgb, var(--rb-surface) 80%, transparent);
    font-size: 0.875rem;
    line-height: 1.45;
  }
</style>
