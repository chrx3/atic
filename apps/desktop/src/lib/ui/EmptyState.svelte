<script lang="ts">
  /**
   * Lo que se ve cuando una lista no tiene nada. Reemplaza a `.rb-empty`.
   *
   * Siempre con una acción si la hay: una lista vacía sin salida deja al
   * usuario sin saber qué hacer para llenarla.
   */
  import type { Snippet } from "svelte";
  import type { ToolId } from "$core/tools";
  import ToolIcon from "$lib/ToolIcon.svelte";

  let {
    title,
    hint,
    icon,
    action,
  }: {
    title: string;
    hint?: string;
    /** Icono de herramienta, centrado encima del título. */
    icon?: ToolId;
    action?: Snippet;
  } = $props();
</script>

<div class="flex flex-col items-center justify-center gap-2 px-6 py-10 text-center">
  {#if icon}
    <div
      class="flex size-11 items-center justify-center rounded-md bg-surface-2
             text-faint"
    >
      <ToolIcon id={icon} size={22} />
    </div>
  {/if}
  <p class="text-sm text-muted">{title}</p>
  {#if hint}
    <p class="max-w-64 text-xs text-faint">{hint}</p>
  {/if}
  {#if action}
    <div class="mt-1">{@render action()}</div>
  {/if}
</div>
