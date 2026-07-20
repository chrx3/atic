<script lang="ts">
  import { onMount } from "svelte";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import {
    listClipboardHistory,
    onClipboardHistoryChanged,
  } from "$lib/api";
  import type { ClipboardItem } from "$lib/types";
  import { toolById } from "$lib/tools";

  let {
    shortcut = "CmdOrCtrl+Shift+V",
    onShortcutChange,
    onToast,
  }: {
    shortcut?: string;
    onShortcutChange: (shortcut: string) => void | Promise<void>;
    onToast?: (message: string) => void;
  } = $props();

  const tool = toolById("clipboard");
  let items = $state<ClipboardItem[]>([]);
  let loading = $state(true);

  async function refresh() {
    try {
      items = await listClipboardHistory();
    } catch (error) {
      onToast?.(String(error));
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void refresh();
    const unlisten = onClipboardHistoryChanged(() => void refresh());
    return () => {
      void unlisten.then((fn) => fn());
    };
  });
</script>

<section class="clip-tool" aria-label="Clipboard">
  <header class="clip-head">
    <p class="clip-kicker">Herramienta</p>
    <h2>{tool.label}</h2>
    <p class="clip-blurb">{tool.blurb}</p>
  </header>

  <div class="clip-shortcut">
    <p class="clip-shortcut-label">Atajo del historial</p>
    <HotkeyCapture
      value={shortcut || "CmdOrCtrl+Shift+V"}
      defaultValue="CmdOrCtrl+Shift+V"
      ariaLabel="Cambiar atajo del historial de clipboard"
      onChange={onShortcutChange}
    />
    <p class="clip-hint">
      Trae la pill al cursor, la expande y muestra el historial. Clic en un ítem
      para pegarlo en la app enfocada.
    </p>
  </div>

  <div class="clip-panel">
    <ClipboardHistoryList
      {items}
      {loading}
      onRefresh={refresh}
      onError={(message) => onToast?.(message)}
    />
  </div>
</section>

<style>
  .clip-tool {
    display: flex;
    height: 100%;
    min-height: 0;
    flex-direction: column;
    gap: 1rem;
    padding: 1.1rem 1.15rem 1.25rem;
  }

  .clip-head {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .clip-kicker {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .clip-head h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 650;
  }

  .clip-blurb {
    margin: 0;
    max-width: 36rem;
    color: var(--rb-muted);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .clip-shortcut {
    display: flex;
    max-width: 28rem;
    flex-direction: column;
    gap: 0.45rem;
  }

  .clip-shortcut-label {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
    font-weight: 600;
  }

  .clip-hint {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .clip-panel {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    padding: 0.65rem 0.7rem;
    border: 1px solid var(--rb-border);
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }
</style>
