<script lang="ts">
  /**
   * Textos guardados a mano, y el bloc.
   *
   * Son dos cosas distintas bajo una pestaña porque comparten el mismo origen:
   * lo que decides guardar, frente al historial que se llena solo.
   */
  import { nextIndex } from "$core/listNav";
  import type { Snippet as SnippetItem } from "$core/types";
  import { snippets } from "$domain/snippets.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
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
  let listEl = $state<HTMLDivElement | null>(null);

  const flatIndex = $derived(
    new Map(snippets.visible.map((item, index) => [item.id, index])),
  );
  const editingIndex = $derived(editing?.id ? (flatIndex.get(editing.id) ?? -1) : -1);

  /**
   * Elegir con el teclado abre el texto en el editor, igual que el clic: acá
   * «seleccionado» y «abierto» son lo mismo, no hay un paso intermedio.
   */
  function selectAt(index: number) {
    const item = snippets.visible[index];
    if (!item) return;
    editing = { ...item };
    const row = listEl?.querySelector<HTMLElement>(`[data-row="${index}"]`);
    row?.focus();
    row?.scrollIntoView({ block: "nearest" });
  }

  function onListKeydown(event: KeyboardEvent, item: SnippetItem) {
    const moved = nextIndex(event.key, editingIndex, snippets.visible.length);
    if (moved !== null) {
      event.preventDefault();
      selectAt(moved);
      return;
    }
    if (event.key === "Delete") {
      event.preventDefault();
      toDelete = item;
    }
  }

  function blank(): SnippetItem {
    return { id: "", name: "", body: "", aliases: [], updatedAtMs: Date.now() };
  }

  async function save() {
    const item = editing;
    if (!item || !item.name.trim()) return;
    saving = true;
    try {
      await snippets.save(item);
      toasts.push(t("toast.savedNamed", { name: item.name }));
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
  title={t("tools.snippets.label")}
  icon="snippets"
  kicker={t("page.snippets.kicker")}
  blurb={t("page.snippets.blurb")}
>
  {#snippet meta()}
    <Chip>{t("page.snippets.count", { count: snippets.items.length })}</Chip>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label={t("page.snippets.view")}>
      <SegmentedControl
        bind:value={tab}
        size="sm"
        label={t("page.snippets.what")}
        options={[
          { value: "snippets", label: t("page.snippets.texts") },
          { value: "scratchpad", label: t("page.snippets.pad") },
        ]}
      />
      {#snippet end()}
        {#if tab === "snippets"}
          <Button variant="primary" size="sm" onclick={() => (editing = blank())}>
            {t("page.snippets.new")}
          </Button>
        {/if}
      {/snippet}
    </Toolbar>

    {#if tab === "snippets"}
      <p class="shrink-0 border-b border-line px-3 py-1.5 text-xs text-muted">
        {t("page.snippets.intro")}
      </p>
    {/if}

    {#if tab === "scratchpad"}
      <div class="min-h-0 flex-1 overflow-y-auto p-3">
        <!-- Guarda sola con retardo; al salir de la vista se fuerza lo pendiente. -->
        <TextArea
          value={snippets.scratchpad?.body ?? ""}
          oninput={(e: Event) =>
            snippets.editScratchpad((e.currentTarget as HTMLTextAreaElement).value)}
          onblur={() => snippets.flushScratchpad()}
          rows={16}
          aria-label={t("page.snippets.padAria")}
          placeholder={t("page.snippets.padPlaceholder")}
        />
      </div>
    {:else}
      <div class="min-h-0 flex-1">
        <ListDetail
          hasSelection={editing !== null}
          listLabel={t("page.snippets.list")}
          listCount={snippets.visible.length}
        >
          {#snippet listHeader()}
            <Input
              type="search"
              bind:value={snippets.query}
              placeholder={t("page.snippets.searchPlaceholder")}
              aria-label={t("page.snippets.searchAria")}
            />
          {/snippet}

          {#snippet list()}
            {#if snippets.visible.length === 0}
              {#if snippets.query}
                <EmptyState
                  compact
                  title={t("page.common.nothing")}
                  hint={t("page.common.fewerWords")}
                />
              {:else}
                <EmptyState
                  compact
                  icon="snippets"
                  title={t("page.snippets.empty")}
                  hint={t("page.snippets.emptyHint")}
                >
                  {#snippet action()}
                    <Button
                      variant="primary"
                      size="sm"
                      onclick={() => (editing = blank())}
                    >
                      {t("page.snippets.newText")}
                    </Button>
                  {/snippet}
                </EmptyState>
              {/if}
            {:else}
              <div bind:this={listEl}>
                <ul class="flex flex-col">
                  {#each snippets.visible as item (item.id)}
                    {@const index = flatIndex.get(item.id) ?? 0}
                    <li>
                      <button
                        type="button"
                        data-row={index}
                        aria-current={editing?.id === item.id ? "true" : undefined}
                        onkeydown={(event) => onListKeydown(event, item)}
                        onclick={() => (editing = { ...item })}
                      >
                        <span class="truncate text-sm text-text">{item.name}</span>
                        <span class="line-clamp-1 text-xs text-faint">{item.body}</span>
                        {#if item.aliases.length > 0}
                          <!-- Los alias son con lo que se lo llama al pegarlo:
                               sin verlos hay que abrir el texto para saberlo. -->
                          <span class="truncate text-micro text-muted">
                            {item.aliases.join(" · ")}
                          </span>
                        {/if}
                      </button>
                    </li>
                  {/each}
                </ul>
              </div>
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
                    .then(() => toasts.push(t("toast.pasted")))
                    .catch(toastError)}
                onDelete={() => (toDelete = editing)}
                onClose={() => (editing = null)}
              />
            {/if}
          {/snippet}

          {#snippet empty()}
            <EmptyState
              compact
              icon="snippets"
              title={t("page.snippets.pick")}
              hint={t("page.snippets.pickHint")}
            >
              {#snippet action()}
                <Button variant="soft" size="sm" onclick={() => (editing = blank())}>
                  {t("page.snippets.newText")}
                </Button>
              {/snippet}
            </EmptyState>
          {/snippet}
        </ListDetail>
      </div>
    {/if}
  </div>
</ToolPage>

{#if toDelete}
  <ConfirmDialog
    title={t("page.snippets.deleteTitle", { name: toDelete.name })}
    body={t("page.snippets.deleteBody")}
    confirmLabel={t("page.common.delete")}
    tone="danger"
    onConfirm={() => void confirmDelete()}
    onCancel={() => (toDelete = null)}
  />
{/if}
