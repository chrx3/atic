<script lang="ts">
  /**
   * Sistema de iconos de Atic.
   *
   * Retícula de 24, área viva 4–20, trazo de 1.25 uniforme, remates y uniones
   * rectas. Vocabulario cerrado: círculo, línea y arco. El único relleno del
   * sistema es el punto de grabación de Reuniones, que actúa como acento.
   */

  import type { ToolId } from "$lib/tools";

  let {
    id,
    size = 20,
    strokeWidth = 1.25,
  }: {
    id: ToolId;
    size?: number;
    strokeWidth?: number;
  } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="butt"
  stroke-linejoin="miter"
  aria-hidden="true"
>
  {#if id === "meetings"}
    <!-- Grabación: círculo con punto macizo. -->
    <circle cx="12" cy="12" r="7" />
    <circle cx="12" cy="12" r="2.25" fill="currentColor" stroke="none" />
  {:else if id === "dictation"}
    <!-- Micrófono: cápsula, horquilla y pie. -->
    <rect x="9.5" y="3" width="5" height="11" rx="2.5" />
    <path d="M6.5 12a5.5 5.5 0 0 0 11 0" />
    <path d="M12 17.5V20" />
  {:else if id === "agents"}
    <!-- Agentes: cursor de consola sobre una ventana. -->
    <rect x="3" y="4.5" width="18" height="15" rx="2" />
    <path d="M7 10l2.5 2.5L7 15" />
    <path d="M12.5 15.5H17" />
  {:else if id === "clipboard"}
    <!-- Historial: hoja trasera enmarcando la de delante, sin cruzarla. -->
    <path d="M4 15V4h11" />
    <rect x="9" y="9" width="11" height="11" />
  {:else if id === "snippets"}
    <!-- Fragmentos: bloque de texto reducido a tres medidas. -->
    <path d="M5 7h14" />
    <path d="M5 12h9" />
    <path d="M5 17h12" />
  {:else}
    <!-- Capturas: marcas de recorte. -->
    <path d="M4 9V4h5" />
    <path d="M15 4h5v5" />
    <path d="M20 15v5h-5" />
    <path d="M9 20H4v-5" />
  {/if}
</svg>
