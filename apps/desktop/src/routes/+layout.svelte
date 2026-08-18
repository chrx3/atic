<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { installDesktopChromeGuards } from "$lib/desktopChrome";
  import { getConfig, onUiTheme } from "$ipc/config";
  import {
    applyConfigTheme,
    applyTheme,
    readCachedTheme,
    THEME_STORAGE_KEY,
  } from "$lib/theme";
  import "../app.css";

  let { children } = $props();

  // Ventanas flotantes: sin chrome de app (fondo transparente).
  const isFloating = $derived(
    [
      "/pill",
      "/capture-shelf",
      "/capture-overlay",
      "/capture-annotate",
      "/launcher",
      "/overlay",
    ].includes(page.url.pathname),
  );

  onMount(() => {
    // Evita el menú contextual de Chromium (Inspect, Imprimir, etc.).
    const block = (e: Event) => e.preventDefault();
    document.addEventListener("contextmenu", block);
    const stopChrome = installDesktopChromeGuards();

    // Cada ventana es un WebView con su propio document: `data-theme` hay que
    // ponerlo en todas. El overlay además tiene un perfil WebView2 propio
    // (`overlay-webview`): localStorage no cruza desde main, así que el
    // cache solo evita un destello acá. La fuente de verdad es la config.
    applyTheme(readCachedTheme());

    // Suscribirse antes de leer: si el tema cambia mientras llega `get_config`,
    // una respuesta vieja no debe pintar encima del evento.
    let themeFromEvent = false;
    const pendingTheme = onUiTheme((theme) => {
      themeFromEvent = true;
      applyConfigTheme(theme);
    });
    void pendingTheme
      .catch(() => {})
      .then(() => getConfig())
      .then((cfg) => {
        if (themeFromEvent) return;
        applyConfigTheme(cfg.ui_theme);
      })
      .catch(() => {
        // Fuera de Tauri, o el webview nació antes del manage.
      });

    // Por si alguna ventana sí comparte origen (labs en el browser).
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
     * Ctrl+Alt+L — lab del OVERLAY (pill/agentes sandbox).
     * Ctrl+Alt+F — launcher lab (sliders en vivo sobre el overlay).
     * Ctrl+Alt+P — picker lab (rueda+cards; titlebar / Esc).
     * Ctrl+Alt+M — UI legacy.
     */
    // Si quedó pegado de una sesión anterior, liberar el overlay al arrancar.
    if (import.meta.env.DEV) {
      localStorage.removeItem("atic-liquid-lab");
      localStorage.removeItem("atic-launcher-lab-open");
    }

    const onDevKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.altKey) return;
      const key = event.key.toLowerCase();

      if (key === "l") {
        event.preventDefault();
        const on = localStorage.getItem("atic-liquid-lab") === "1";
        if (on) localStorage.removeItem("atic-liquid-lab");
        else localStorage.setItem("atic-liquid-lab", "1");
        window.dispatchEvent(
          new StorageEvent("storage", {
            key: "atic-liquid-lab",
            newValue: on ? null : "1",
          }),
        );
        return;
      }

      if (key === "f") {
        event.preventDefault();
        const on = localStorage.getItem("atic-launcher-lab-open") === "1";
        if (on) localStorage.removeItem("atic-launcher-lab-open");
        else localStorage.setItem("atic-launcher-lab-open", "1");
        window.dispatchEvent(
          new StorageEvent("storage", {
            key: "atic-launcher-lab-open",
            newValue: on ? null : "1",
          }),
        );
        return;
      }

      if (key === "m") {
        event.preventDefault();
        const path = window.location.pathname;
        void goto(path.startsWith("/legacy") ? "/" : "/legacy");
      }
    };
    if (import.meta.env.DEV) window.addEventListener("keydown", onDevKey);

    return () => {
      stopChrome();
      document.removeEventListener("contextmenu", block);
      window.removeEventListener("storage", onStorage);
      window.removeEventListener("keydown", onDevKey);
      mq.removeEventListener("change", onScheme);
      void pendingTheme.then((off) => off()).catch(() => {});
    };
  });
</script>

{#if isFloating}
  {@render children()}
{:else}
  <a class="rb-skip-link" href="#main-content">Ir al contenido</a>
  {@render children()}
{/if}
