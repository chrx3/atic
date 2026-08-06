<script lang="ts">
  /**
   * Sistema de iconos de Atic.
   *
   * Retícula de 24, área viva 4–20, trazo de 1.25 uniforme, remates y uniones
   * rectas. Vocabulario cerrado: círculo, línea y arco. El único relleno del
   * sistema es el punto de grabación de Reuniones, que actúa como acento.
   *
   * Además de las herramientas, cubre glifos de ajustes (general, atajos,
   * audio, resúmenes) con el mismo vocabulario.
   */

  import type { ToolId } from "$lib/tools";

  /** Glifos que no son una tool pero viven en ajustes. */
  export type SettingsIconId =
    | "general"
    | "shortcuts"
    | "audio"
    | "summary"
    | "settings";

  export type IconId = ToolId | SettingsIconId;

  let {
    id,
    size = 20,
    strokeWidth = 1.25,
  }: {
    id: IconId;
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
  {:else if id === "captures"}
    <!-- Capturas: marcas de recorte. -->
    <path d="M4 9V4h5" />
    <path d="M15 4h5v5" />
    <path d="M20 15v5h-5" />
    <path d="M9 20H4v-5" />
  {:else if id === "general" || id === "settings"}
    <!-- Ajustes / general: tres sliders (línea + tope). -->
    <path d="M4 7h16" />
    <path d="M4 12h16" />
    <path d="M4 17h16" />
    <circle cx="9" cy="7" r="1.75" fill="currentColor" stroke="none" />
    <circle cx="15" cy="12" r="1.75" fill="currentColor" stroke="none" />
    <circle cx="11" cy="17" r="1.75" fill="currentColor" stroke="none" />
  {:else if id === "shortcuts"}
    <!-- Atajos: tecla. -->
    <rect x="3.5" y="6" width="17" height="12" rx="2" />
    <path d="M7 10h2.5" />
    <path d="M11.5 10H17" />
    <path d="M7 14h10" />
  {:else if id === "audio"}
    <!-- Audio: tres barras de nivel. -->
    <path d="M7 8v8" />
    <path d="M12 5v14" />
    <path d="M17 9v6" />
  {:else}
    <!-- Resúmenes: hoja con medidas. -->
    <path d="M7 3.5h7l4 4V20.5H7z" />
    <path d="M14 3.5V8h4.5" />
    <path d="M10 12h6" />
    <path d="M10 15.5h4" />
  {/if}
</svg>
