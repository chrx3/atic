<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import AticMark from "$lib/AticMark.svelte";
  import type { ToolDef } from "$lib/tools";
  import {
    themeLabel,
    type UiTheme,
  } from "$lib/theme";

  let {
    tool,
    theme = "system",
    onToggleTheme,
    onOpenSettings,
  }: {
    tool: ToolDef;
    theme?: UiTheme;
    onToggleTheme?: () => void;
    onOpenSettings?: () => void;
  } = $props();

  let maximized = $state(false);

  onMount(() => {
    let win: ReturnType<typeof getCurrentWindow>;
    try {
      win = getCurrentWindow();
    } catch {
      // Preview web sin ventana nativa: sin botones de ventana.
      return;
    }
    let cancelled = false;

    void (async () => {
      try {
        const value = await win.isMaximized();
        if (!cancelled) maximized = value;
      } catch {
        // Preview web sin ventana nativa.
      }
    })();

    const unlistenPromise = win.onResized(async () => {
      try {
        maximized = await win.isMaximized();
      } catch {
        // ignore
      }
    });

    return () => {
      cancelled = true;
      void unlistenPromise.then((unlisten) => unlisten());
    };
  });

  function appWindow() {
    return getCurrentWindow();
  }

  async function minimize() {
    await appWindow().minimize();
  }

  async function toggleMaximize() {
    await appWindow().toggleMaximize();
    maximized = await appWindow().isMaximized();
  }

  async function close() {
    await appWindow().close();
  }

  const themeTitle = $derived(`Tema: ${themeLabel(theme)} (clic para cambiar)`);
</script>

<header class="atic-titlebar" aria-label="Barra de ventana">
  <div class="atic-titlebar-lead" data-tauri-drag-region>
    <!-- La marca es identidad, no navegación: volver al inicio vive en el
         breadcrumb del contenido, para no duplicar el mismo control. -->
    <span class="atic-mark" data-tauri-drag-region>
      <AticMark size={18} strokeWidth={1.5} />
    </span>
    <div class="atic-titlebar-copy" data-tauri-drag-region>
      <strong data-tauri-drag-region>Atic</strong>
      <span data-tauri-drag-region>{tool.label}</span>
    </div>
  </div>

  <div
    class="atic-titlebar-drag"
    data-tauri-drag-region
    role="presentation"
    ondblclick={toggleMaximize}
  ></div>

  <div class="atic-titlebar-actions">
    {#if onToggleTheme}
      <button
        type="button"
        class="atic-titlebar-btn"
        aria-label={themeTitle}
        title={themeTitle}
        onclick={onToggleTheme}
      >
        {#if theme === "dark"}
          <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
            <path
              d="M11.5 9.2A5.2 5.2 0 0 1 6.8 4.5 4.6 4.6 0 1 0 11.5 9.2Z"
              fill="none"
              stroke="currentColor"
              stroke-width="1.25"
              stroke-linejoin="round"
            />
          </svg>
        {:else if theme === "light"}
          <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="2.6" fill="none" stroke="currentColor" stroke-width="1.25" />
            <path
              d="M8 1.8v1.4M8 12.8v1.4M1.8 8h1.4M12.8 8h1.4M3.4 3.4l1 1M11.6 11.6l1 1M12.6 3.4l-1 1M4.4 11.6l-1 1"
              stroke="currentColor"
              stroke-width="1.25"
              stroke-linecap="round"
            />
          </svg>
        {:else}
          <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="5.2" fill="none" stroke="currentColor" stroke-width="1.25" />
            <path d="M8 2.8v10.4" stroke="currentColor" stroke-width="1.25" />
            <path
              d="M8 2.8a5.2 5.2 0 0 1 0 10.4"
              fill="currentColor"
              opacity="0.35"
            />
          </svg>
        {/if}
      </button>
    {/if}

    {#if onOpenSettings}
      <button
        type="button"
        class="atic-titlebar-btn"
        aria-label="Ajustes"
        title="Ajustes"
        onclick={onOpenSettings}
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M6.5 2.5h3l.4 1.6a4.5 4.5 0 0 1 1.2.7l1.5-.6.1.2 1.5 2.6-.1.2-1.3.9c.1.4.1.8 0 1.2l1.3.9.1.2-1.5 2.6-.1.2-1.5-.6a4.5 4.5 0 0 1-1.2.7L9.5 13.5h-3l-.4-1.6a4.5 4.5 0 0 1-1.2-.7l-1.5.6-.1-.2L1.8 9.1l.1-.2 1.3-.9a4.6 4.6 0 0 1 0-1.2L1.9 5.9l-.1-.2L3.3 3.1l.1-.2 1.5.6c.37-.28.77-.5 1.2-.7L6.5 2.5Z"
            fill="none"
            stroke="currentColor"
            stroke-width="1.2"
          />
          <circle cx="8" cy="8" r="1.8" fill="none" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </button>
    {/if}

    <button
      type="button"
      class="atic-titlebar-btn"
      aria-label="Minimizar"
      title="Minimizar"
      onclick={minimize}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M1.5 5h7" stroke="currentColor" stroke-width="1.3" />
      </svg>
    </button>
    <button
      type="button"
      class="atic-titlebar-btn"
      aria-label={maximized ? "Restaurar" : "Maximizar"}
      title={maximized ? "Restaurar" : "Maximizar"}
      onclick={toggleMaximize}
    >
      {#if maximized}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path
            d="M3 3.5h4.5V8H3V3.5Zm1.2-1.2h4.5V7"
            fill="none"
            stroke="currentColor"
            stroke-width="1.15"
          />
        </svg>
      {:else}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect
            x="1.6"
            y="1.6"
            width="6.8"
            height="6.8"
            fill="none"
            stroke="currentColor"
            stroke-width="1.15"
          />
        </svg>
      {/if}
    </button>
    <button
      type="button"
      class="atic-titlebar-btn atic-titlebar-btn-close"
      aria-label="Cerrar"
      title="Cerrar"
      onclick={close}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path
          d="M2.2 2.2l5.6 5.6M7.8 2.2L2.2 7.8"
          stroke="currentColor"
          stroke-width="1.25"
        />
      </svg>
    </button>
  </div>
</header>

<style>
  .atic-titlebar {
    display: flex;
    align-items: stretch;
    flex: 0 0 auto;
    height: 40px;
    background: color-mix(in srgb, var(--rb-surface) 92%, var(--rb-bg1));
    user-select: none;
    -webkit-user-select: none;
  }

  .atic-titlebar-lead {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0 0.7rem 0 0.75rem;
    min-width: 0;
  }

  .atic-mark {
    display: inline-flex;
    color: var(--rb-text);
    line-height: 0;
  }


  .atic-titlebar-copy {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    min-width: 0;
  }

  .atic-titlebar-copy strong {
    font-family: var(--rb-display);
    font-size: 0.8125rem;
    font-weight: 650;
    letter-spacing: -0.02em;
    color: var(--rb-text);
  }

  .atic-titlebar-copy span {
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .atic-titlebar-drag {
    flex: 1 1 auto;
    min-width: 1.5rem;
  }

  .atic-titlebar-actions {
    display: flex;
    flex: 0 0 auto;
    align-items: stretch;
  }

  .atic-titlebar-btn {
    display: inline-flex;
    width: 42px;
    align-items: center;
    justify-content: center;
    border: 0;
    margin: 0;
    padding: 0;
    color: var(--rb-faint);
    background: transparent;
    cursor: default;
  }

  .atic-titlebar-btn:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }

  .atic-titlebar-btn:focus-visible {
    outline: none;
    box-shadow: inset var(--rb-focus);
  }

  .atic-titlebar-btn-close:hover {
    color: #fff;
    background: #c42b1c;
  }

  @container atic-shell (max-width: 47.999rem) {
    .atic-titlebar-copy span {
      display: none;
    }

    .atic-titlebar-btn {
      width: 36px;
    }
  }
</style>
