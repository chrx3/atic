<script lang="ts">
  /**
   * Textos guardados a mano, y el bloc.
   *
   * Son dos cosas distintas bajo una pestaña porque comparten el mismo origen:
   * lo que decides guardar, frente al historial que se llena solo.
   */
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
      {#if tab === "snippets"}
        <div class="min-w-0 flex-1">
          <Input
            type="search"
            bind:value={snippets.query}
            placeholder={t("page.snippets.searchPlaceholder")}
            aria-label={t("page.snippets.searchAria")}
          />
        </div>
      {/if}
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
              <ul class="flex flex-col">
                {#each snippets.visible as item (item.id)}
                  <li>
                    <button
                      type="button"
                      class="flex w-full flex-col gap-0.5 px-3 py-1.5
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
