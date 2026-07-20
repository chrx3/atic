<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import "../app.css";

  let { children } = $props();

  // Ventanas flotantes: sin chrome de app (fondo transparente).
  const isFloating = $derived(
    ["/pill", "/capture-shelf", "/capture-overlay"].includes(page.url.pathname),
  );

  onMount(() => {
    // Evita el menú contextual de Chromium (Inspect, Imprimir, etc.).
    const block = (e: Event) => e.preventDefault();
    document.addEventListener("contextmenu", block);
    return () => document.removeEventListener("contextmenu", block);
  });
</script>

{#if isFloating}
  {@render children()}
{:else}
  <a class="rb-skip-link" href="#main-content">Ir al contenido</a>
  {@render children()}
{/if}
