<script lang="ts">
  import { onMount } from "svelte";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
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

<section class="snip-tool" aria-label="Fragmentos">
  <header class="snip-head">
    <p class="snip-kicker">Herramienta</p>
    <h2>{tool.label}</h2>
    <p class="snip-blurb">{tool.blurb}</p>
  </header>

  <div class="snip-shortcut">
    <p class="snip-shortcut-label">Atajo de fragmentos</p>
    <HotkeyCapture
      value={shortcut || "CmdOrCtrl+Shift+S"}
      defaultValue="CmdOrCtrl+Shift+S"
      ariaLabel="Cambiar atajo del panel de fragmentos"
      onChange={onShortcutChange}
    />
    <p class="snip-hint">
      Trae la pill al cursor y abre el panel. Clic en un fragmento para pegarlo en
      la app enfocada.
    </p>
  </div>

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
            <input type="text" bind:value={editing.name} required />
          </label>
          <label class="snip-field">
            <span>Alias (separados por coma)</span>
            <input type="text" bind:value={aliasesText} placeholder="firma, saludo…" />
          </label>
          <label class="snip-field snip-field-grow">
            <span>Contenido</span>
            <textarea bind:value={editing.body} rows="8"></textarea>
          </label>
          <div class="snip-editor-actions">
            <button type="button" class="snip-btn ghost" onclick={cancelEdit}>
              Cancelar
            </button>
            <button type="submit" class="snip-btn" disabled={saving}>
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
            class="snip-scratch-area"
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
</section>

<style>
  .snip-tool {
    display: flex;
    height: 100%;
    min-height: 0;
    flex-direction: column;
    gap: 1rem;
    padding: 1.1rem 1.15rem 1.25rem;
  }

  .snip-head {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .snip-kicker {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .snip-head h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 650;
  }

  .snip-blurb {
    margin: 0;
    max-width: 36rem;
    color: var(--rb-muted);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .snip-shortcut {
    display: flex;
    max-width: 28rem;
    flex-direction: column;
    gap: 0.45rem;
  }

  .snip-shortcut-label,
  .snip-hint,
  .snip-scratch-meta,
  .snip-empty {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .snip-shortcut-label {
    font-weight: 600;
  }

  .snip-tabs {
    display: flex;
    gap: 0.35rem;
  }

  .snip-tab {
    border: 1px solid var(--rb-border);
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
    border: 1px solid var(--rb-border);
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

  .snip-field input,
  .snip-field textarea,
  .snip-scratch-area {
    width: 100%;
    border: 1px solid var(--rb-border);
    border-radius: 0.45rem;
    padding: 0.45rem 0.55rem;
    background: var(--rb-bg0);
    color: var(--rb-text);
    font-size: 0.8125rem;
    font-family: inherit;
    resize: vertical;
  }

  .snip-field-grow textarea,
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

  .snip-btn {
    border: 0;
    border-radius: 0.45rem;
    padding: 0.4rem 0.8rem;
    background: var(--rb-accent);
    color: #fff;
    font-size: 0.8125rem;
    font-weight: 600;
    cursor: pointer;
  }

  .snip-btn.ghost {
    background: transparent;
    color: var(--rb-muted);
    border: 1px solid var(--rb-border);
  }

  .snip-scratch {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.35rem;
  }
</style>
