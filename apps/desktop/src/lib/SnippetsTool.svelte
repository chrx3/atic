<script lang="ts">
  import { onMount } from "svelte";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Trash2 } from "$lib/icons";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import ToolPageShell from "$lib/ToolPageShell.svelte";
  import {
    deleteNote,
    getScratchpad,
    listNotes,
    listSnippets,
    onSnippetsChanged,
    saveNote,
    setScratchpad,
    upsertSnippet,
  } from "$lib/api";
  import type { Note as TextNote, Snippet as TextSnippet } from "$lib/types";
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
  let notes = $state<TextNote[]>([]);
  /** Nota que se está editando en el bloc. `null` = borrador sin archivar. */
  let currentNoteId = $state<string | null>(null);
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
      onToast?.("Texto guardado");
    } catch (error) {
      onToast?.(String(error));
    } finally {
      saving = false;
    }
  }

  /* ─── Notas ───────────────────────────────────────────────────────────────
   *
   * El bloc es siempre "la nota actual". No hay un botón de guardar que puedas
   * olvidar: se autoguarda como antes, y cambiar de nota archiva la que estabas
   * escribiendo. Así nunca hay un estado en el que perder texto sea posible,
   * que era el problema del bloc único.
   */
  const currentTitle = $derived(
    notes.find((n) => n.id === currentNoteId)?.title ?? "Nota nueva",
  );

  function noteDate(ms: number): string {
    return new Date(ms).toLocaleDateString(undefined, {
      day: "numeric",
      month: "short",
    });
  }

  async function refreshNotes() {
    try {
      notes = await listNotes();
    } catch (error) {
      onToast?.(String(error));
    }
  }

  /** Archiva lo que haya en el bloc. Devuelve el id resultante (o el actual). */
  async function commitCurrent(): Promise<string | null> {
    if (!scratchBody.trim()) return currentNoteId;
    try {
      const saved = await saveNote(currentNoteId, scratchBody);
      await refreshNotes();
      return saved?.id ?? currentNoteId;
    } catch (error) {
      onToast?.(String(error));
      return currentNoteId;
    }
  }

  async function startNewNote() {
    await commitCurrent();
    currentNoteId = null;
    scratchBody = "";
    await persistScratchpad();
  }

  async function openNote(note: TextNote) {
    if (note.id === currentNoteId) return;
    await commitCurrent();
    currentNoteId = note.id;
    scratchBody = note.body;
    await persistScratchpad();
  }

  async function removeNote(note: TextNote) {
    try {
      await deleteNote(note.id);
      if (note.id === currentNoteId) {
        currentNoteId = null;
        scratchBody = "";
        await persistScratchpad();
      }
      await refreshNotes();
    } catch (error) {
      onToast?.(String(error));
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
      // El bloc guarda el borrador vivo; la nota archivada se actualiza en el
      // mismo paso para que cerrar la app no deje las dos versiones distintas.
      await setScratchpad(scratchBody);
      if (currentNoteId && scratchBody.trim()) {
        await saveNote(currentNoteId, scratchBody);
        await refreshNotes();
      }
    } catch (error) {
      onToast?.(String(error));
    } finally {
      scratchSaving = false;
    }
  }

  onMount(() => {
    void refresh();
    void loadScratchpad();
    void refreshNotes();
    const unlisten = onSnippetsChanged(() => void refresh());
    return () => {
      if (scratchTimer) {
        clearTimeout(scratchTimer);
        // Guardar lo pendiente, no descartarlo: el autoguardado espera 500 ms
        // tras la última tecla, y salir de la pestaña dentro de esa ventana
        // tiraba lo último escrito sin avisar.
        void persistScratchpad();
      }
      void unlisten.then((fn) => fn());
    };
  });
</script>

<ToolPageShell {tool} dataDir="snippets">
  {#snippet prefs()}
    <div class="atic-shortcut-row">
      <div>
        <p class="atic-shortcut-label">Atajo de textos</p>
        <p class="atic-shortcut-hint">Trae la pill al cursor.</p>
      </div>
      <HotkeyCapture
        value={shortcut || "CmdOrCtrl+Shift+S"}
        defaultValue="CmdOrCtrl+Shift+S"
        ariaLabel="Cambiar atajo del panel de textos"
        onChange={onShortcutChange}
      />
    </div>
  {/snippet}

  <!-- Dos cosas distintas conviven acá: textos que guardás para reusar y un
       bloc de notas libre. Las pestañas son lo único que las separa, así que
       tienen que nombrar el CONTENIDO, no la vista. -->
  <div class="snip-tabs" role="tablist" aria-label="Textos y notas">
    <button
      type="button"
      role="tab"
      class="snip-tab"
      class:active={tab === "snippets"}
      aria-selected={tab === "snippets"}
      onclick={() => (tab = "snippets")}
    >
      Textos
    </button>
    <button
      type="button"
      role="tab"
      class="snip-tab"
      class:active={tab === "scratchpad"}
      aria-selected={tab === "scratchpad"}
      onclick={() => (tab = "scratchpad")}
    >
      Notas
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
            <!-- Decía "Alias", que promete expansión automática: escribís
                 "firma" en cualquier app y se reemplaza sola. Eso NO existe;
                 estas palabras solo sirven para encontrar el fragmento en el
                 buscador de la lista. El nombre ahora dice lo que hace. -->
            <span>Palabras para buscarlo</span>
            <input
              class="rb-field"
              type="text"
              bind:value={aliasesText}
              placeholder="firma, saludo, despedida"
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
          <div class="snip-note-bar">
            <span class="snip-note-title">
              {currentNoteId ? currentTitle : "Nota nueva"}
            </span>
            <button
              type="button"
              class="rb-btn rb-btn-ghost snip-note-new"
              onclick={() => void startNewNote()}
              disabled={!scratchBody.trim()}
            >
              Guardar y empezar otra
            </button>
          </div>
          <textarea
            class="snip-scratch-area rb-field"
            bind:value={scratchBody}
            oninput={scheduleScratchSave}
            placeholder="Escribí lo que sea. Se guarda solo, acá en tu equipo."
            aria-label="Nota"
          ></textarea>
          <p class="snip-scratch-meta">
            {scratchSaving ? "Guardando…" : "Guardado localmente"}
          </p>

          <!-- La lista vive debajo del área de escritura, no en otra pestaña:
               consultar una nota vieja mientras escribís es el caso normal. -->
          {#if notes.length > 0}
            <ul class="snip-notes" role="list">
              {#each notes as note (note.id)}
                <li class:is-current={note.id === currentNoteId}>
                  <button
                    type="button"
                    class="snip-note"
                    onclick={() => void openNote(note)}
                  >
                    <span class="snip-note-name">{note.title}</span>
                    <span class="snip-note-date">{noteDate(note.updatedAtMs)}</span>
                  </button>
                  <button
                    type="button"
                    class="snip-icon-btn is-danger"
                    aria-label="Eliminar {note.title}"
                    onclick={() => void removeNote(note)}
                  >
                    <Icon icon={Trash2} size={14} />
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
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
  .snip-note-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .snip-note-title {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.75rem;
    font-weight: 600;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .snip-note-new {
    flex-shrink: 0;
    font-size: 0.75rem;
  }

  .snip-notes {
    display: flex;
    max-height: 12rem;
    flex-direction: column;
    gap: 0.3rem;
    margin: 0.25rem 0 0;
    padding: 0;
    list-style: none;
    overflow: auto;
  }

  .snip-notes li {
    display: flex;
    align-items: stretch;
    gap: 0.35rem;
  }

  .snip-note {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    border: 0;
    border-radius: 0.5rem;
    padding: 0.4rem 0.55rem;
    background: var(--rb-bg0);
    color: var(--rb-text);
    text-align: left;
    cursor: pointer;
  }

  .snip-notes li.is-current .snip-note {
    background: var(--rb-accent-soft);
    color: var(--rb-accent);
  }

  .snip-note-name {
    min-width: 0;
    overflow: hidden;
    font-size: 0.8125rem;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .snip-note-date {
    flex-shrink: 0;
    color: var(--rb-muted);
    font-size: 0.6875rem;
  }

  .snip-icon-btn {
    border: 0;
    border-radius: 0.4rem;
    padding: 0.25rem 0.35rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .snip-icon-btn.is-danger:hover {
    color: var(--rb-record);
  }

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
