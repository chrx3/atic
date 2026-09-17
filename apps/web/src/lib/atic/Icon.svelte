<script lang="ts">
  /**
   * Icono Lucide. Mismo trazo y misma retícula que la app (24×24, stroke),
   * pero pintado directo desde los datos: acá no hay morph que hacer.
   */
  import type { IconNode } from "./icons";

  let {
    icon,
    size = 20,
    strokeWidth = 1.75,
    label,
    class: className,
  }: {
    icon: IconNode;
    size?: number;
    strokeWidth?: number;
    label?: string;
    class?: string;
  } = $props();
</script>

<svg
  class={className}
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
>
  {#each icon as [tag, attrs] (tag + JSON.stringify(attrs))}
    <svelte:element this={tag} {...attrs} />
  {/each}
</svg>
