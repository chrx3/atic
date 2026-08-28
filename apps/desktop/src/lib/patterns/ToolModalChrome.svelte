<script lang="ts">
  /**
   * Proveedor de chrome `modal` para tools embebidas en ToolWorkspace.
   *
   * Vive como componente propio (no solo setContext en el modal) porque el
   * body del Modal se renderiza vía snippet: el contexto tiene que colgar de
   * un ancestro real en el árbol donde montan las tools.
   *
   * El wrapper `h-full min-h-0` cierra la cadena flex hacia `ToolPage` para
   * que las listas/grids puedan scrollear dentro del panel.
   */
  import type { Snippet } from "svelte";
  import { provideToolPageChrome } from "./toolPageChrome";

  let { children }: { children: Snippet } = $props();

  provideToolPageChrome("modal");
</script>

<div class="flex h-full min-h-0 flex-col">
  {@render children()}
</div>
