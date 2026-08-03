<script lang="ts">
  /** Un bloque de preferencias con título. Las filas se separan solas. */
  import type { Snippet } from "svelte";

  let {
    title,
    hint,
    children,
  }: {
    title: string;
    hint?: string;
    children: Snippet;
  } = $props();
</script>

<section class="flex flex-col gap-1">
  <h3 class="text-micro text-faint uppercase">{title}</h3>
  {#if hint}
    <p class="text-xs text-faint">{hint}</p>
  {/if}
  <!-- El separador entre filas, sin uno colgando al final. -->
  <div class="mt-1 flex flex-col divide-y divide-line">
    {@render children()}
  </div>
</section>
