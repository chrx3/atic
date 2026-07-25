<script lang="ts">
  import { onMount } from "svelte";
  import {
    activateCapture,
    listRecentCaptures,
    ocrCaptureAndCopy,
    startCaptureSession,
  } from "$lib/api";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import ToolPageShell from "$lib/ToolPageShell.svelte";
  import type { CaptureItem } from "$lib/types";
  import { toolById } from "$lib/tools";
  import { convertFileSrc } from "@tauri-apps/api/core";

  let {
    shortcut,
    onToast,
    onShortcutChange,
    onOpenSettings,
  }: {
    shortcut: string;
    onToast: (message: string) => void;
    onShortcutChange: (shortcut: string) => void | Promise<void>;
    onOpenSettings: () => void;
  } = $props();

  const tool = toolById("captures");
  let items = $state<CaptureItem[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let ocrBusyPath = $state<string | null>(null);

  async function refresh() {
    try {
      items = await listRecentCaptures();
    } catch (error) {
      onToast(String(error));
    } finally {
      loading = false;
    }
  }

  async function startCapture() {
    if (busy) return;
    busy = true;
    try {
      await startCaptureSession();
    } catch (error) {
      onToast(String(error));
    } finally {
      busy = false;
    }
  }

  async function runOcr(path: string, event?: MouseEvent) {
    event?.stopPropagation();
    if (ocrBusyPath) return;
    ocrBusyPath = path;
    try {
      const text = await ocrCaptureAndCopy(path);
      const preview = text.length > 80 ? `${text.slice(0, 80)}…` : text;
      onToast(`Texto copiado: ${preview}`);
    } catch (error) {
      onToast(String(error));
    } finally {
      ocrBusyPath = null;
    }
  }

  function labelFor(item: CaptureItem): string {
    if (item.label?.trim()) return item.label;
    const name = item.path.split(/[/\\]/).pop() ?? item.path;
    return name.replace(/^Atic_/, "").replace(/\.png$/i, "");
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 4000);
    return () => clearInterval(timer);
  });
</script>

<ToolPageShell {tool} dataDir="captures">
  {#snippet actions()}
    <button
      type="button"
      class="rb-btn rb-btn-primary"
      disabled={busy}
      onclick={startCapture}
    >
      Nueva captura
    </button>
    <button type="button" class="rb-btn rb-btn-soft" onclick={onOpenSettings}>
      Ajustes
    </button>
  {/snippet}

  {#snippet prefs()}
    <div class="atic-shortcut-row">
      <div>
        <p class="atic-shortcut-label">Atajo de captura</p>
        <p class="atic-shortcut-hint">Selecciona ventana o región.</p>
      </div>
      <HotkeyCapture
        value={shortcut || "CmdOrCtrl+Shift+4"}
        defaultValue="CmdOrCtrl+Shift+4"
        ariaLabel="Cambiar atajo de captura"
        onChange={onShortcutChange}
      />
    </div>
  {/snippet}

  {#if loading}
    <p class="atic-empty">Cargando recientes…</p>
  {:else if items.length === 0}
    <p class="atic-empty">Captura una ventana o región para empezar.</p>
  {:else}
    <ul class="cap-grid">
      {#each items.slice(0, 12) as item (item.path)}
        <li>
          <div class="cap-thumb-wrap">
            <button
              type="button"
              class="cap-thumb"
              title={item.path}
              onclick={() =>
                void activateCapture(item.path).catch((e) => onToast(String(e)))}
            >
              <img src={convertFileSrc(item.path)} alt="" />
              <span>{labelFor(item)}</span>
            </button>
            <button
              type="button"
              class="cap-ocr-btn"
              disabled={ocrBusyPath === item.path}
              title="Extraer texto (OCR)"
              onclick={(e) => void runOcr(item.path, e)}
            >
              {ocrBusyPath === item.path ? "…" : "Texto"}
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</ToolPageShell>

<style>
  .atic-empty {
    margin: 0;
    padding: 1rem;
    border: 0;
    border-radius: var(--rb-radius);
    color: var(--rb-muted);
    background: color-mix(in srgb, var(--rb-text) 4%, transparent);
    font-size: 0.875rem;
  }

  .cap-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(7.5rem, 1fr));
    gap: 0.55rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .cap-thumb-wrap {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .cap-ocr-btn {
    border: 0;
    border-radius: var(--rb-radius-sm);
    padding: 0.2rem 0.35rem;
    background: var(--rb-bg1);
    color: var(--rb-muted);
    font-size: 0.625rem;
    font-weight: 650;
    cursor: pointer;
  }

  @container atic-main (max-width: 36.999rem) {
    .cap-grid {
      grid-template-columns: repeat(auto-fill, minmax(6.25rem, 1fr));
      gap: 0.45rem;
    }

    .cap-ocr-btn {
      min-height: 2rem;
      font-size: 0.6875rem;
    }
  }

  .cap-ocr-btn:hover:not(:disabled) {
    color: var(--rb-text);
    border-color: var(--rb-border-strong);
  }

  .cap-thumb {
    display: flex;
    width: 100%;
    flex-direction: column;
    gap: 0.35rem;
    border: 0;
    border-radius: var(--rb-radius-sm);
    padding: 0.35rem;
    color: var(--rb-muted);
    background: var(--rb-surface);
    text-align: left;
    cursor: pointer;
  }

  .cap-thumb:hover {
    border-color: var(--rb-border-strong);
    color: var(--rb-text);
  }

  .cap-thumb img {
    display: block;
    width: 100%;
    aspect-ratio: 3 / 2;
    border-radius: 0.3rem;
    object-fit: cover;
    background: var(--rb-bg1);
  }

  .cap-thumb span {
    overflow: hidden;
    font-size: 0.6875rem;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
