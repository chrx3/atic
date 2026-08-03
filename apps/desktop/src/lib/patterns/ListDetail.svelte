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
    list,
    detail,
    empty,
  }: {
    hasSelection?: boolean;
    listLabel?: string;
    list: Snippet;
    detail: Snippet;
    /** Qué mostrar en el panel de detalle cuando no hay nada elegido. */
    empty?: Snippet;
  } = $props();
</script>

<div class="@container/split h-full">
  <div class="flex h-full">
    <!-- Bajo el corte, la lista cede el sitio al detalle. -->
    <nav
      aria-label={listLabel}
      class="min-h-0 w-full shrink-0 overflow-y-auto border-line
             @md/split:block @md/split:w-64 @md/split:border-r
             {hasSelection ? 'hidden @md/split:block' : 'block'}"
    >
      {@render list()}
    </nav>

    <section
      class="min-h-0 min-w-0 flex-1 overflow-y-auto
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
