<script lang="ts">
  /**
   * El resumen, leído como documento y no como Markdown.
   *
   * El texto lo escribe un modelo, así que es la superficie menos confiable de
   * la app: se parsea a un modelo de datos y se dibuja con elementos, nunca con
   * `@html`. Un resumen que traiga `<script>` acá es texto.
   *
   * Tolera texto a medio llegar porque durante la generación se redibuja con
   * cada token: una sección sin contenido todavía es un estado normal, no un
   * error.
   */
  import { parseSummaryDocument, type SummarySectionKind } from "$core/summary-format";
  import { t } from "$domain/i18n.svelte";

  let {
    content,
    defaultTitle,
    compact = false,
    streaming = false,
    emptyMessage,
  }: {
    content: string;
    defaultTitle?: string;
    /** Para las vistas previas: menos aire y todo un punto más chico. */
    compact?: boolean;
    streaming?: boolean;
    emptyMessage?: string;
  } = $props();

  /** El punto de cada sección es lo único con color: dice de qué tipo es. */
  const MARKER: Record<SummarySectionKind, string> = {
    summary: "bg-accent",
    decisions: "bg-ok",
    tasks: "bg-info",
    topics: "bg-muted",
    general: "bg-faint",
  };

  const fallbackTitle = $derived(defaultTitle ?? t("page.summary.docSummary"));
  const idleEmpty = $derived(emptyMessage ?? t("page.summary.emptyPlaceholder"));
  const sections = $derived(parseSummaryDocument(content, fallbackTitle));
</script>

<article
  class="overflow-hidden rounded-sm bg-surface-2 text-text"
  aria-label={t("page.summary.documentAria")}
  aria-busy={streaming}
>
  {#if sections.length === 0}
    <p class="p-4 text-sm text-muted">{idleEmpty}</p>
  {:else}
    {#each sections as section, index (section.id)}
      {@const lead = index === 0 && section.kind === "summary"}
      <!-- Hairline entre secciones, no aire: con cinco secciones el aire solo
           empuja el final fuera de la pantalla. -->
      <section
        class="border-t border-line first:border-0
               {compact ? 'px-3 py-2.5' : 'px-4 py-4'} {lead ? 'bg-elevated' : ''}"
      >
        <header class="mb-2 flex items-center gap-2">
          <span
            class="size-1.5 shrink-0 rounded-pill {MARKER[section.kind]}
                   {streaming ? 'motion-safe:animate-pulse' : ''}"
            aria-hidden="true"
          ></span>
          <h4
            class="text-pretty {compact
              ? 'text-xs'
              : 'text-sm'} font-semibold text-text"
          >
            {section.title}
          </h4>
        </header>

        {#if section.blocks.length === 0}
          <p class="pl-3.5 text-xs text-faint">
            {streaming ? t("page.summary.completing") : t("page.summary.emptySection")}
          </p>
        {:else}
          <div class="flex max-w-[72ch] flex-col gap-2 {compact ? 'pl-3.5' : 'pl-4'}">
            {#each section.blocks as block, blockIndex (`${section.id}-${blockIndex}`)}
              {#if block.type === "paragraph"}
                <p
                  class="text-pretty leading-relaxed {compact
                    ? 'text-xs'
                    : 'text-sm'} {lead ? 'text-text' : 'text-muted'}"
                >
                  {block.text}
                </p>
              {:else}
                <ul class="flex list-none flex-col gap-1.5">
                  {#each block.items as item, itemIndex (`${section.id}-${blockIndex}-${itemIndex}`)}
                    <li
                      class="grid grid-cols-[0.875rem_minmax(0,1fr)] items-start gap-2
                             leading-relaxed text-muted {compact
                        ? 'text-xs'
                        : 'text-sm'}"
                    >
                      {#if item.checked === null}
                        <!-- Viñeta o número: solo marca dónde empieza el ítem. -->
                        <span
                          class="text-center font-mono text-xs text-faint"
                          data-numeric
                          aria-hidden="true"
                        >
                          {block.ordered ? `${itemIndex + 1}.` : "•"}
                        </span>
                      {:else}
                        <!-- Casilla: el modelo la marcó, no se puede tocar. Va
                             como texto y no como `<input disabled>` para que no
                             invite a un clic que no hace nada. -->
                        <span
                          class="mt-0.5 grid size-3.5 place-items-center rounded-xs
                                 text-micro font-bold
                                 {item.checked
                            ? 'bg-accent text-on-accent'
                            : 'bg-line-strong text-transparent'}"
                          role="img"
                          aria-label={item.checked ? t("page.summary.done") : t("page.summary.pending")}
                        >
                          ✓
                        </span>
                      {/if}
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
