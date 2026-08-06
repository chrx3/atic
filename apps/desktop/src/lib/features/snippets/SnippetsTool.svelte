<script lang="ts">
  /**
   * Textos guardados a mano, y el bloc.
   *
   * Son dos cosas distintas bajo una pestaña porque comparten el mismo origen:
   * lo que vos decidís guardar, frente al historial que se llena solo.
   */
  import type { Snippet as SnippetItem } from "$core/types";
  import { snippets } from "$domain/snippets.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import ListDetail from "$patterns/ListDetail.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Input from "$ui/Input.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import TextArea from "$ui/TextArea.svelte";
  import SnippetEditor from "./SnippetEditor.svelte";
  import { untrack } from "svelte";

  let { initialTab = "snippets" }: { initialTab?: "snippets" | "scratchpad" } =
    $props();

  // `untrack` porque acá el valor inicial es exactamente lo que se quiere: si
  // siguiera a la prop, entrar desde la búsqueda al bloc y volver a la pestaña
  // de textos a mano se desharía solo en el siguiente render.
  let tab = $state<"snippets" | "scratchpad">(untrack(() => initialTab));
  let editing = $state<SnippetItem | null>(null);
  let toDelete = $state<SnippetItem | null>(null);
  let saving = $state(false);

  function blank(): SnippetItem {
    return { id: "", name: "", body: "", aliases: [], updatedAtMs: Date.now() };
  }

  async function save() {
    const item = editing;
    if (!item || !item.name.trim()) return;
    saving = true;
    try {
      await snippets.save(item);
      toasts.push(`Guardado: ${item.name}`);
      editing = null;
    } catch (error) {
      toastError(error);
    } finally {
      saving = false;
    }
  }

  async function confirmDelete() {
    const target = toDelete;
    if (!target) return;
    try {
      await snippets.remove(target.id);
      if (editing?.id === target.id) editing = null;
      toDelete = null;
    } catch (error) {
      toastError(error);
    }
  }
</script>

<ToolPage
  title="Textos"
  icon="snippets"
  kicker="Guardados a mano"
  blurb="Los textos que escribís siempre, listos para pegar. Más un bloc para notas sueltas."
>
  {#snippet meta()}
    <Chip>{snippets.items.length} textos</Chip>
  {/snippet}

  <div class="flex h-full flex-col">
    <Toolbar label="Vista de textos">
      <SegmentedControl
        bind:value={tab}
        label="Qué mostrar"
        options={[
          { value: "snippets", label: "Textos" },
          { value: "scratchpad", label: "Bloc" },
        ]}
      />
      {#snippet end()}
        {#if tab === "snippets"}
          <Button variant="primary" size="sm" onclick={() => (editing = blank())}>
            Nuevo
          </Button>
        {/if}
      {/snippet}
    </Toolbar>

    {#if tab === "scratchpad"}
      <div class="min-h-0 flex-1 p-4">
        <!-- Guarda sola con retardo; al salir de la vista se fuerza lo pendiente. -->
        <TextArea
          value={snippets.scratchpad?.body ?? ""}
          oninput={(e: Event) =>
            snippets.editScratchpad((e.currentTarget as HTMLTextAreaElement).value)}
          onblur={() => snippets.flushScratchpad()}
          rows={16}
          aria-label="Bloc de notas"
          placeholder="Notas sueltas. Se guardan solas."
        />
      </div>
    {:else}
      <div class="px-4 pt-3">
        <Input
          type="search"
          bind:value={snippets.query}
          placeholder="Buscar…"
          aria-label="Buscar textos"
        />
      </div>

      <div class="min-h-0 flex-1 pt-3">
        <ListDetail
          hasSelection={editing !== null}
          listLabel="Textos guardados"
          listCount={snippets.visible.length}
        >
          {#snippet list()}
            {#if snippets.visible.length === 0}
              <EmptyState
                icon={snippets.query ? undefined : "snippets"}
                title={snippets.query ? "Nada coincide" : "Todavía no hay textos"}
                hint={snippets.query ? undefined : "Guardá el primero con «Nuevo»."}
              />
            {:else}
              <ul class="flex flex-col">
                {#each snippets.visible as item (item.id)}
                  <li>
                    <button
                      type="button"
                      class="flex w-full flex-col gap-0.5 px-3 py-2
                             text-left transition-colors duration-(--duration-quick)
                             hover:bg-surface-2
                             {editing?.id === item.id ? 'bg-surface-2' : ''}"
                      aria-current={editing?.id === item.id ? "true" : undefined}
                      onclick={() => (editing = { ...item })}
                    >
                      <span class="truncate text-sm text-text">{item.name}</span>
                      <span class="line-clamp-1 text-xs text-faint">{item.body}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          {/snippet}

          {#snippet detail()}
            {#if editing}
              <SnippetEditor
                bind:item={editing}
                {saving}
                onSave={() => void save()}
                onPaste={(id) =>
                  void snippets
                    .paste(id)
                    .then(() => toasts.push("Pegado"))
                    .catch(toastError)}
                onDelete={() => (toDelete = editing)}
                onClose={() => (editing = null)}
              />
            {/if}
          {/snippet}

          {#snippet empty()}
            <EmptyState
              icon="snippets"
              title="Elegí un texto"
              hint="O creá uno nuevo."
            />
          {/snippet}
        </ListDetail>
      </div>
    {/if}
  </div>
</ToolPage>

{#if toDelete}
  <ConfirmDialog
    title="Borrar «{toDelete.name}»"
    body="Se borra el texto guardado. No se puede deshacer."
    confirmLabel="Borrar"
    tone="danger"
    onConfirm={() => void confirmDelete()}
    onCancel={() => (toDelete = null)}
  />
{/if}
