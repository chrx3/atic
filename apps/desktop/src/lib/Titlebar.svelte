<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let maximized = $state(false);

  onMount(() => {
    const win = getCurrentWindow();
    let cancelled = false;

    void (async () => {
      try {
        const value = await win.isMaximized();
        if (!cancelled) maximized = value;
      } catch {
        // Fuera de Tauri (preview web) no hay ventana nativa.
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
</script>

<header class="rb-titlebar" aria-label="Barra de ventana">
  <div
    class="rb-titlebar-drag"
    data-tauri-drag-region
    role="button"
    tabindex="0"
    aria-label="Maximizar o restaurar ventana"
    ondblclick={toggleMaximize}
    onkeydown={(event) => {
      if (event.key === "Enter") void toggleMaximize();
    }}
  >
    <span class="rb-titlebar-brand" data-tauri-drag-region>Atic</span>
  </div>

  <div class="rb-titlebar-controls">
    <button
      type="button"
      class="rb-titlebar-btn"
      aria-label="Minimizar"
      title="Minimizar"
      onclick={minimize}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M1 5h8" stroke="currentColor" stroke-width="1.2" fill="none" />
      </svg>
    </button>
    <button
      type="button"
      class="rb-titlebar-btn"
      aria-label={maximized ? "Restaurar" : "Maximizar"}
      title={maximized ? "Restaurar" : "Maximizar"}
      onclick={toggleMaximize}
    >
      {#if maximized}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect
            x="1.2"
            y="2.8"
            width="5.5"
            height="5.5"
            stroke="currentColor"
            stroke-width="1.1"
            fill="none"
          />
          <path
            d="M3.2 2.8V1.8h5.5v5.5H7.7"
            stroke="currentColor"
            stroke-width="1.1"
            fill="none"
          />
        </svg>
      {:else}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect
            x="1.5"
            y="1.5"
            width="7"
            height="7"
            stroke="currentColor"
            stroke-width="1.1"
            fill="none"
          />
        </svg>
      {/if}
    </button>
    <button
      type="button"
      class="rb-titlebar-btn rb-titlebar-btn-close"
      aria-label="Cerrar"
      title="Cerrar"
      onclick={close}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path
          d="M2 2l6 6M8 2L2 8"
          stroke="currentColor"
          stroke-width="1.2"
          fill="none"
        />
      </svg>
    </button>
  </div>
</header>

<style>
  .rb-titlebar {
    display: flex;
    align-items: stretch;
    flex: 0 0 auto;
    height: 34px;
    border-bottom: 0;
    background: color-mix(in srgb, var(--rb-bg1) 88%, var(--rb-surface));
    user-select: none;
    -webkit-user-select: none;
  }

  .rb-titlebar-drag {
    display: flex;
    min-width: 0;
    flex: 1 1 auto;
    align-items: center;
    padding: 0 0.75rem;
  }

  .rb-titlebar-brand {
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 550;
    letter-spacing: 0.01em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rb-titlebar-controls {
    display: flex;
    flex: 0 0 auto;
    align-items: stretch;
  }

  .rb-titlebar-btn {
    display: inline-flex;
    width: 46px;
    align-items: center;
    justify-content: center;
    border: 0;
    margin: 0;
    padding: 0;
    color: var(--rb-faint);
    background: transparent;
    cursor: default;
  }

  .rb-titlebar-btn:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .rb-titlebar-btn:focus-visible {
    outline: none;
    box-shadow: inset var(--rb-focus);
  }

  .rb-titlebar-btn-close:hover {
    color: #fff;
    background: #c42b1c;
  }
</style>
