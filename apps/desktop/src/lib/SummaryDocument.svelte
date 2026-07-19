<script lang="ts">
  import { parseSummaryDocument } from "$lib/summary-format";

  let {
    content,
    defaultTitle = "Resumen",
    compact = false,
    streaming = false,
    emptyMessage = "El resumen aparecerá aquí.",
  }: {
    content: string;
    defaultTitle?: string;
    compact?: boolean;
    streaming?: boolean;
    emptyMessage?: string;
  } = $props();

  const sections = $derived(parseSummaryDocument(content, defaultTitle));
</script>

<article
  class="rb-summary-document rb-select-text"
  class:is-compact={compact}
  class:is-streaming={streaming}
  aria-label="Contenido del resumen"
  aria-busy={streaming}
>
  {#if sections.length === 0}
    <p class="rb-summary-empty">{emptyMessage}</p>
  {:else}
    {#each sections as section, sectionIndex (section.id)}
      <section
        class="rb-summary-section"
        class:is-lead={sectionIndex === 0 && section.kind === "summary"}
        data-kind={section.kind}
      >
        <header>
          <span class="rb-summary-marker" aria-hidden="true"></span>
          <h4>{section.title}</h4>
        </header>

        {#if section.blocks.length === 0}
          <p class="rb-summary-empty is-section">
            {streaming ? "Completando sección…" : "Sin contenido"}
          </p>
        {:else}
          <div class="rb-summary-blocks">
            {#each section.blocks as block, blockIndex (`${section.id}-${blockIndex}`)}
              {#if block.type === "paragraph"}
                <p>{block.text}</p>
              {:else}
                <ul class:is-ordered={block.ordered}>
                  {#each block.items as item, itemIndex (`${section.id}-${blockIndex}-${itemIndex}`)}
                    <li class:has-checkbox={item.checked !== null}>
                      <span
                        class="rb-summary-list-marker"
                        class:is-checked={item.checked === true}
                        aria-hidden="true"
                      >
                        {item.checked === true
                          ? "✓"
                          : item.checked === false
                            ? ""
                            : block.ordered
                              ? `${itemIndex + 1}.`
                              : "•"}
                      </span>
                      <span>{item.text}</span>
                    </li>
                  {/each}
                </ul>
              {/if}
            {/each}
          </div>
        {/if}
      </section>
    {/each}
  {/if}
</article>

<style>
  .rb-summary-document {
    overflow: hidden;
    border-radius: var(--rb-radius);
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 3.5%, transparent);
  }

  .rb-summary-section {
    padding: 1rem 1.1rem;
  }

  .rb-summary-section + .rb-summary-section {
    margin-top: 0.2rem;
  }

  .rb-summary-section.is-lead {
    padding-block: 1.15rem;
    background: color-mix(in srgb, var(--rb-accent) 9%, transparent);
  }

  .rb-summary-section header {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    margin-bottom: 0.55rem;
  }

  .rb-summary-marker {
    width: 0.45rem;
    height: 0.45rem;
    flex: 0 0 auto;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rb-muted) 65%, transparent);
  }

  [data-kind="summary"] .rb-summary-marker {
    background: var(--rb-accent);
  }

  [data-kind="decisions"] .rb-summary-marker {
    background: var(--rb-ok);
  }

  [data-kind="tasks"] .rb-summary-marker {
    background: var(--rb-info);
  }

  .rb-summary-section h4 {
    color: var(--rb-text);
    font-size: 0.8125rem;
    font-weight: 650;
    line-height: 1.3;
    text-wrap: balance;
  }

  .rb-summary-blocks {
    display: flex;
    max-width: 72ch;
    flex-direction: column;
    gap: 0.55rem;
    padding-left: 1rem;
  }

  .rb-summary-blocks p,
  .rb-summary-empty {
    color: var(--rb-muted);
    font-size: 0.8125rem;
    line-height: 1.65;
    text-wrap: pretty;
  }

  .is-lead .rb-summary-blocks p {
    color: color-mix(in srgb, var(--rb-text) 88%, var(--rb-accent));
    font-size: 0.875rem;
  }

  .rb-summary-blocks ul {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rb-summary-blocks li {
    display: grid;
    grid-template-columns: 0.85rem minmax(0, 1fr);
    gap: 0.45rem;
    align-items: start;
    color: var(--rb-muted);
    font-size: 0.8125rem;
    line-height: 1.55;
  }

  .rb-summary-list-marker {
    color: var(--rb-faint);
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  li.has-checkbox .rb-summary-list-marker {
    width: 0.85rem;
    height: 0.85rem;
    margin-top: 0.18rem;
    border-radius: 3px;
    background: color-mix(in srgb, var(--rb-text) 9%, transparent);
    color: #fbfbf8;
    font-size: 0.65rem;
    font-weight: 800;
    line-height: 0.85rem;
  }

  li.has-checkbox .rb-summary-list-marker.is-checked {
    background: var(--rb-accent);
  }

  .rb-summary-empty {
    margin: 0;
    padding: 1rem;
  }

  .rb-summary-empty.is-section {
    padding: 0 0 0 1rem;
    color: var(--rb-faint);
    font-size: 0.75rem;
  }

  .rb-summary-document.is-compact .rb-summary-section {
    padding: 0.75rem 0.85rem;
  }

  .rb-summary-document.is-compact .rb-summary-blocks {
    padding-left: 0;
  }

  .rb-summary-document.is-compact .rb-summary-section h4,
  .rb-summary-document.is-compact .rb-summary-blocks p,
  .rb-summary-document.is-compact .rb-summary-blocks li {
    font-size: 0.75rem;
  }

  .rb-summary-document.is-streaming .rb-summary-marker {
    animation: rb-summary-pulse 1.4s ease-in-out infinite;
  }

  @keyframes rb-summary-pulse {
    50% {
      opacity: 0.38;
    }
  }

  @media (max-width: 36rem) {
    .rb-summary-section {
      padding: 0.9rem;
    }

    .rb-summary-blocks {
      padding-left: 0;
    }

    .rb-summary-blocks p,
    .rb-summary-blocks li,
    .is-lead .rb-summary-blocks p {
      font-size: 0.875rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .rb-summary-document.is-streaming .rb-summary-marker {
      animation: none;
    }
  }
</style>
