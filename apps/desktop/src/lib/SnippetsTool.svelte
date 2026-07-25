<script lang="ts">
  import { onMount } from "svelte";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import ToolPageShell from "$lib/ToolPageShell.svelte";
  import {
    getScratchpad,
    listSnippets,
    onSnippetsChanged,
    setScratchpad,
    upsertSnippet,
  } from "$lib/api";
  import type { Snippet as TextSnippet } from "$lib/types";
  import { toolById } from "$lib/tools";

  let {
    shortcut = "CmdOrCtrl+Shift+S",
    initialTab = "snippets",
    onShortcutChange,
    onToast,
  }: {
    shortcut?: string;
    initialTab?: "snippets" | "scratchpad";
    onShortcutChange: (shortcut: string) => void | Promise<void>;
    onToast?: (message: string) => void;
  } = $props();

  const tool = toolById("snippets");
  let tab = $state<"snippets" | "scratchpad">("snippets");
  let items = $state<TextSnippet[]>([]);
  let loading = $state(true);
  let scratchBody = $state("");
  let scratchLoading = $state(true);
  let scratchSaving = $state(false);
  let editing = $state<TextSnippet | null>(null);
  let aliasesText = $state("");
  let saving = $state(false);

  let scratchTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    tab = initialTab;
  });

  async function refresh() {
    try {
      items = await listSnippets();
    } catch (error) {
      onToast?.(String(error));
    } finally {
      loading = false;
    }
  }

  async function loadScratchpad() {
    scratchLoading = true;
    try {
      const pad = await getScratchpad();
      scratchBody = pad.body;
    } catch (error) {
      onToast?.(String(error));
    } finally {
      scratchLoading = false;
    }
  }

  function startEdit(snippet: TextSnippet) {
    editing = { ...snippet };
    aliasesText = snippet.aliases.join(", ");
    tab = "snippets";
  }

  function cancelEdit() {
    editing = null;
    aliasesText = "";
  }

  async function saveEdit() {
    if (!editing || saving) return;
    saving = true;
    try {
      const aliases = aliasesText
        .split(",")
        .map((a) => a.trim())
        .filter(Boolean);
      await upsertSnippet({ ...editing, aliases });
      editing = null;
      aliasesText = "";
      await refresh();
      onToast?.("Fragmento guardado");
    } catch (error) {
      onToast?.(String(error));
    } finally {
      saving = false;
    }
  }

  function scheduleScratchSave() {
    if (scratchTimer) clearTimeout(scratchTimer);
    scratchTimer = setTimeout(() => {
      void persistScratchpad();
    }, 500);
  }

  async function persistScratchpad() {
    if (scratchSaving) return;
    scratchSaving = true;
    try {
      await setScratchpad(scratchBody);
    } catch (error) {
      onToast?.(String(error));
    } finally {
      scratchSaving = false;
    }
  }

  onMount(() => {
    void refresh();
    void loadScratchpad();
    const unlisten = onSnippetsChanged(() => void refresh());
    return () => {
      if (scratchTimer) clearTimeout(scratchTimer);
      void unlisten.then((fn) => fn());
    };
  });
</script>

<ToolPageShell {tool} dataDir="snippets">
  {#snippet prefs()}
    <div class="atic-shortcut-row">
      <div>
        <p class="atic-shortcut-label">Atajo de fragmentos</p>
        <p class="atic-shortcut-hint">Trae la pill al cursor.</p>
      </div>
      <HotkeyCapture
        value={shortcut || "CmdOrCtrl+Shift+S"}
        defaultValue="CmdOrCtrl+Shift+S"
        ariaLabel="Cambiar atajo del panel de fragmentos"
        onChange={onShortcutChange}
      />
    </div>
  {/snippet}

  <div class="snip-tabs" role="tablist" aria-label="Vistas de fragmentos">
    <button
      type="button"
      role="tab"
      class="snip-tab"
      class:active={tab === "snippets"}
      aria-selected={tab === "snippets"}
      onclick={() => (tab = "snippets")}
    >
      Fragmentos
    </button>
    <button
      type="button"
      role="tab"
      class="snip-tab"
      class:active={tab === "scratchpad"}
      aria-selected={tab === "scratchpad"}
      onclick={() => (tab = "scratchpad")}
    >
      Bloc de notas
    </button>
  </div>

  <div class="snip-panel">
    {#if tab === "snippets"}
      {#if editing}
        <form
          class="snip-editor"
          onsubmit={(event) => {
            event.preventDefault();
            void saveEdit();
          }}
        >
          <label class="snip-field">
            <span>Nombre</span>
            <input class="rb-field" type="text" bind:value={editing.name} required />
          </label>
          <label class="snip-field">
            <span>Alias (separados por coma)</span>
            <input
              class="rb-field"
              type="text"
              bind:value={aliasesText}
              placeholder="firma, saludo…"
            />
          </label>
          <label class="snip-field snip-field-grow">
            <span>Contenido</span>
            <textarea class="rb-field" bind:value={editing.body} rows="8"></textarea>
          </label>
          <div class="snip-editor-actions">
            <button type="button" class="rb-btn rb-btn-ghost" onclick={cancelEdit}>
              Cancelar
            </button>
            <button type="submit" class="rb-btn rb-btn-primary" disabled={saving}>
              {saving ? "Guardando…" : "Guardar"}
            </button>
          </div>
        </form>
      {:else}
        <SnippetsList
          {items}
          {loading}
          onRefresh={refresh}
          onEdit={startEdit}
          onError={(message) => onToast?.(message)}
        />
      {/if}
    {:else}
      <div class="snip-scratch">
        {#if scratchLoading}
          <p class="snip-empty">Cargando bloc…</p>
        {:else}
          <textarea
            class="snip-scratch-area rb-field"
            bind:value={scratchBody}
            oninput={scheduleScratchSave}
            placeholder="Notas temporales… se guardan automáticamente."
            aria-label="Bloc de notas"
          ></textarea>
          <p class="snip-scratch-meta">
            {scratchSaving ? "Guardando…" : "Guardado localmente"}
          </p>
        {/if}
      </div>
    {/if}
  </div>
</ToolPageShell>

<style>
  .snip-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin-bottom: 0;
  }

  .snip-tab {
    border: 0;
    border-radius: 999px;
    padding: 0.3rem 0.75rem;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
  }

  .snip-tab.active {
    border-color: color-mix(in srgb, var(--rb-accent) 40%, var(--rb-border));
    background: var(--rb-accent-soft);
    color: var(--rb-accent);
  }

  .snip-panel {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    padding: 0.65rem 0.7rem;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  .snip-editor {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.65rem;
  }

  .snip-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.75rem;
    color: var(--rb-muted);
  }

  .snip-field-grow {
    min-height: 0;
    flex: 1;
  }

  .snip-field :global(.rb-field),
  .snip-scratch-area {
    width: 100%;
  }

  .snip-field-grow :global(textarea),
  .snip-scratch-area {
    min-height: 10rem;
    flex: 1;
    resize: none;
  }

  .snip-editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.45rem;
  }

  .snip-scratch {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.35rem;
  }

  .snip-scratch-meta,
  .snip-empty {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  @container atic-main (max-width: 36.999rem) {
    .snip-panel {
      padding: 0.5rem 0.55rem;
    }

    .snip-tab {
      min-height: 2.25rem;
      padding-inline: 0.85rem;
    }

    .snip-editor-actions {
      flex-direction: column-reverse;
    }

    .snip-editor-actions :global(.rb-btn) {
      width: 100%;
    }
  }
</style>
