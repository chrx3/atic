<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
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

    /**
     * Atajos de desarrollo. Solo en dev.
     *
     * Ctrl+Alt+L — banco de pruebas líquido. La ventana overlay nace
     *   `focusable(false)`, así que no recibe teclas: el interruptor vive acá y
     *   viaja por `localStorage`, igual que el tema.
     * Ctrl+Alt+M — alterna entre la UI actual y la reescrita. Ninguna ventana
     *   tiene barra de direcciones, así que sin esto la pantalla nueva no se
     *   puede abrir donde importa, que es dentro de la app.
     */
    const onDevKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.altKey) return;
      const key = event.key.toLowerCase();

      if (key === "l") {
        event.preventDefault();
        const on = localStorage.getItem("atic-liquid-lab") === "1";
        if (on) localStorage.removeItem("atic-liquid-lab");
        else localStorage.setItem("atic-liquid-lab", "1");
        return;
      }

      if (key === "m") {
        event.preventDefault();
        const path = window.location.pathname;
        void goto(path.startsWith("/dev/main") ? "/" : "/dev/main");
      }
    };
    if (import.meta.env.DEV) window.addEventListener("keydown", onDevKey);

    return () => {
      document.removeEventListener("contextmenu", block);
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("keydown", onDevKey);
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
