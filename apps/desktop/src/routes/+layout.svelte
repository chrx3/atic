<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import Titlebar from "$lib/Titlebar.svelte";
  import "../app.css";

  let { children } = $props();

  const showTitlebar = $derived(page.url.pathname !== "/pill");

  onMount(() => {
    // Evita el menú contextual de Chromium (Inspect, Imprimir, etc.).
    const block = (e: Event) => e.preventDefault();
    document.addEventListener("contextmenu", block);
    return () => document.removeEventListener("contextmenu", block);
  });
</script>

{#if showTitlebar}
  <div class="rb-shell">
    <a class="rb-skip-link" href="#main-content">Ir al contenido</a>
    <Titlebar />
    <div class="rb-shell-body">
      {@render children()}
    </div>
  </div>
{:else}
  {@render children()}
{/if}

<style>
  .rb-shell {
    display: flex;
    height: 100dvh;
    flex-direction: column;
    background: var(--rb-bg0);
    overflow: hidden;
  }

  .rb-shell-body {
    min-height: 0;
    flex: 1 1 auto;
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
</style>
