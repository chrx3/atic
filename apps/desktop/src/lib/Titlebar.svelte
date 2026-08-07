<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import AticMark from "$lib/AticMark.svelte";
  import type { ToolDef } from "$lib/tools";
  import {
    themeLabel,
    type UiTheme,
  } from "$lib/theme";
  import Icon from "$ui/Icon.svelte";
  import {
    Minus,
    Monitor,
    Moon,
    Settings,
    Square,
    Sun,
    X,
  } from "$lib/icons";

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
        <Icon
          icon={theme === "dark" ? Moon : theme === "light" ? Sun : Monitor}
          size={14}
        />
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
        <Icon icon={Settings} size={14} />
      </button>
    {/if}

    <button
      type="button"
      class="atic-titlebar-btn"
      aria-label="Minimizar"
      title="Minimizar"
      onclick={minimize}
    >
      <Icon icon={Minus} size={10} />
    </button>
    <button
      type="button"
      class="atic-titlebar-btn"
      aria-label={maximized ? "Restaurar" : "Maximizar"}
      title={maximized ? "Restaurar" : "Maximizar"}
      onclick={toggleMaximize}
    >
      <Icon icon={Square} size={10} />
    </button>
    <button
      type="button"
      class="atic-titlebar-btn atic-titlebar-btn-close"
      aria-label="Cerrar"
      title="Cerrar"
      onclick={close}
    >
      <Icon icon={X} size={10} />
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
