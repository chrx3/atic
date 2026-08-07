<script lang="ts">
  /**
   * Una fila de controles con un separador debajo.
   *
   * `role="toolbar"` hace que un lector de pantalla la anuncie como un grupo y
   * que las flechas naveguen dentro. Por eso pide `label`: «barra de
   * herramientas» a secas no dice de qué.
   */
  import type { Snippet } from "svelte";

  let {
    label,
    children,
    end,
  }: {
    label: string;
    children: Snippet;
    /** Alineado a la derecha, separado del resto. */
    end?: Snippet;
  } = $props();
</script>

<div
  role="toolbar"
  aria-label={label}
  aria-orientation="horizontal"
  class="flex shrink-0 items-center gap-1.5 border-b border-line px-3 py-1.5"
>
  {@render children()}
  {#if end}
    <div class="ml-auto flex shrink-0 items-center gap-1.5">{@render end()}</div>
  {/if}
</div>
