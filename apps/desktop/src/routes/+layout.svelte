<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { applyTheme, readCachedTheme, THEME_STORAGE_KEY } from "$lib/theme";
  import "../app.css";

  let { children } = $props();

  // Ventanas flotantes: sin chrome de app (fondo transparente).
  const isFloating = $derived(
    ["/pill", "/capture-shelf", "/capture-overlay", "/launcher", "/overlay"].includes(
      page.url.pathname,
    ),
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

    // Interruptor del banco de pruebas líquido (solo dev).
    //
    // La ventana overlay nace `focusable(false)`, así que no recibe teclas: el
    // atajo tiene que vivir en una ventana normal y viajar por `localStorage`,
    // igual que el tema. Ctrl+Alt+L.
    const onLabKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.altKey || event.key.toLowerCase() !== "l") return;
      event.preventDefault();
      const on = localStorage.getItem("atic-liquid-lab") === "1";
      if (on) localStorage.removeItem("atic-liquid-lab");
      else localStorage.setItem("atic-liquid-lab", "1");
    };
    if (import.meta.env.DEV) window.addEventListener("keydown", onLabKey);

    return () => {
      document.removeEventListener("contextmenu", block);
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("keydown", onLabKey);
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
