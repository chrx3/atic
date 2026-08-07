<script lang="ts">
  /**
   * Lo que se ve cuando una lista no tiene nada. Reemplaza a `.rb-empty`.
   *
   * Siempre con una acción si la hay: una lista vacía sin salida deja al
   * usuario sin saber qué hacer para llenarla.
   *
   * `compact` achica icono y aire — pensado para paneles partidos / modales
   * donde el vacío no debe comer media columna.
   */
  import type { Snippet } from "svelte";
  import type { ToolId } from "$core/tools";
  import ToolIcon from "$lib/ToolIcon.svelte";

  let {
    title,
    hint,
    icon,
    compact = false,
    action,
  }: {
    title: string;
    hint?: string;
    /** Icono de herramienta, centrado encima del título. */
    icon?: ToolId;
    /** Menos padding e icono chico: listas densas y modales. */
    compact?: boolean;
    action?: Snippet;
  } = $props();
</script>

<div
  class="flex flex-col items-center justify-center text-center
         {compact ? 'gap-1.5 px-3 py-4' : 'gap-2 px-5 py-6'}"
>
  {#if icon}
    <div
      class="flex items-center justify-center rounded-md bg-surface-2 text-muted
             {compact ? 'size-7' : 'size-10'}"
    >
      <ToolIcon id={icon} size={compact ? 14 : 20} />
    </div>
  {/if}
  <p class="text-sm text-muted text-pretty">{title}</p>
  {#if hint}
    <p class="max-w-56 text-xs text-faint text-pretty">{hint}</p>
  {/if}
  {#if action}
    <div class="mt-0.5">{@render action()}</div>
  {/if}
</div>
