<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { applyTheme, readCachedTheme, THEME_STORAGE_KEY } from "$lib/theme";
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

    // Cada ventana es un WebView con su propio document: `data-theme` hay que
    // ponerlo en todas. Sin esto, pill / shelf / overlay se quedaban siempre
    // en los tokens claros aunque la app estuviera en oscuro.
    applyTheme(readCachedTheme());

    // localStorage es compartido entre ventanas del mismo origen: `storage`
    // avisa a las flotantes cuando la principal cambia el tema.
    const onStorage = (event: StorageEvent) => {
      if (event.key === null || event.key === THEME_STORAGE_KEY) {
        applyTheme(readCachedTheme());
      }
    };
    window.addEventListener("storage", onStorage);

    // Con tema "system", seguir al SO también en las flotantes.
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onScheme = () => {
      if (readCachedTheme() === "system") applyTheme("system");
    };
    mq.addEventListener("change", onScheme);

    return () => {
      document.removeEventListener("contextmenu", block);
      window.removeEventListener("storage", onStorage);
      mq.removeEventListener("change", onScheme);
    };
  });
</script>

{#if isFloating}
  {@render children()}
{:else}
  <a class="rb-skip-link" href="#main-content">Ir al contenido</a>
  {@render children()}
{/if}
