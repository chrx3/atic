<script lang="ts">
  import { onMount } from "svelte";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import ToolPageShell from "$lib/ToolPageShell.svelte";
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

<ToolPageShell {tool} dataDir="clipboard">
  {#snippet prefs()}
    <div class="atic-shortcut-row">
      <div>
        <p class="atic-shortcut-label">Atajo del historial</p>
        <p class="atic-shortcut-hint">Trae la pill al cursor.</p>
      </div>
      <HotkeyCapture
        value={shortcut || "CmdOrCtrl+Shift+V"}
        defaultValue="CmdOrCtrl+Shift+V"
        ariaLabel="Cambiar atajo del historial de clipboard"
        onChange={onShortcutChange}
      />
    </div>
  {/snippet}

  <div class="clip-panel">
    <ClipboardHistoryList
      {items}
      {loading}
      onRefresh={refresh}
      onError={(message) => onToast?.(message)}
    />
  </div>
</ToolPageShell>

<style>
  .clip-panel {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    padding: 0.65rem 0.7rem;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  @container atic-main (max-width: 36.999rem) {
    .clip-panel {
      padding: 0.5rem 0.55rem;
    }
  }
</style>
