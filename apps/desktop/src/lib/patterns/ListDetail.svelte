<script lang="ts">
  /**
   * Lista a un lado, detalle al otro. El patrón que se repite en grabaciones,
   * historial del portapapeles y fragmentos, reimplementado en cada uno.
   *
   * Se parte por CONTAINER query y no por viewport: la misma herramienta se
   * dibuja en una ventana de 820 px y en un panel de 312 dentro de la pill, y
   * lo que decide si entran dos columnas es el ancho del contenedor, no el de
   * la pantalla.
   *
   * Debajo del corte no se apilan las dos: se muestra el detalle si hay algo
   * seleccionado y la lista si no. Apilarlas deja el detalle fuera de vista sin
   * que nada lo anuncie.
   */
  import type { Snippet } from "svelte";

  let {
    hasSelection = false,
    listLabel = "Lista",
    listCount,
    list,
    detail,
    empty,
  }: {
    hasSelection?: boolean;
    listLabel?: string;
    /** Cuántos ítems hay en la lista; se muestra junto al rótulo. */
    listCount?: number;
    list: Snippet;
    detail: Snippet;
    /** Qué mostrar en el panel de detalle cuando no hay nada elegido. */
    empty?: Snippet;
  } = $props();
</script>

<div class="@container/split h-full min-h-0">
  <div class="flex h-full min-h-0">
    <!-- Bajo el corte, la lista cede el sitio al detalle. -->
    <nav
      aria-label={listLabel}
      class="flex min-h-0 w-full shrink-0 flex-col overflow-hidden border-line
             @md/split:w-56 @md/split:border-r
             {hasSelection ? 'hidden @md/split:flex' : 'flex'}"
    >
      {#if listLabel}
        <div
          class="flex shrink-0 items-center justify-between border-b border-line
                 px-3 py-1.5"
        >
          <span class="text-micro text-muted uppercase">{listLabel}</span>
          {#if listCount !== undefined}
            <span class="text-micro text-muted tabular-nums">{listCount}</span>
          {/if}
        </div>
      {/if}

      <div
        class="min-h-0 flex-1 overflow-y-auto
               [&_li_button]:flex [&_li_button]:w-full [&_li_button]:flex-col
               [&_li_button]:gap-0.5 [&_li_button]:px-3 [&_li_button]:py-1.5
               [&_li_button]:text-left
               [&_li_button]:transition-colors [&_li_button]:duration-(--duration-quick)
               [&_li_button]:ease-calm
               [&_li_button:hover]:bg-surface-2
               [&_li_button[aria-current=true]]:bg-surface-2
               [&_li_button[aria-current=true]]:shadow-[inset_2px_0_0_0_var(--rb-record)]
               [&_ul]:flex [&_ul]:flex-col [&_ul]:divide-y [&_ul]:divide-line"
      >
        {@render list()}
      </div>
    </nav>

    <section
      class="min-h-0 min-w-0 flex-1 overflow-y-auto p-3
             [&_h3]:text-md [&_h3]:font-semibold [&_h3]:text-text
             {hasSelection ? 'block' : 'hidden @md/split:block'}"
    >
      {#if hasSelection}
        {@render detail()}
      {:else if empty}
        {@render empty()}
      {/if}
    </section>
  </div>
</div>
