<script lang="ts">
  /** Historial del portapapeles: buscar, fijar, pegar. */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { formatListWhen } from "$core/format";
  import { clipboard } from "$domain/clipboard.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Chip from "$ui/Chip.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import { Pin, Trash2 } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";

  async function run(action: () => Promise<void>, done?: string) {
    try {
      await action();
      if (done) toasts.push(done);
    } catch (error) {
      toastError(error);
    }
  }
</script>

<ToolPage
  title={t("tools.clipboard.label")}
  icon="clipboard"
  kicker={t("tools.clipboard.short")}
  blurb={t("tools.clipboard.blurb")}
>
  {#snippet meta()}
    <Chip>{t("page.clipboard.count", { count: clipboard.items.length })}</Chip>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label={t("page.clipboard.search")}>
      <div class="w-full">
        <Input
          type="search"
          bind:value={clipboard.query}
          placeholder={t("page.clipboard.searchPlaceholder")}
          aria-label={t("page.clipboard.search")}
        />
      </div>
    </Toolbar>

    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if clipboard.visible.length === 0}
        <EmptyState
          compact
          icon={clipboard.query ? undefined : "clipboard"}
          title={clipboard.query ? t("page.common.nothing") : t("page.clipboard.empty")}
          hint={clipboard.query
            ? t("page.common.fewerWords")
            : t("page.clipboard.emptyHint")}
        />
      {:else}
        <ul class="flex flex-col">
          {#each clipboard.visible as item (item.id)}
            <li
              class="group flex items-start gap-2 border-b border-line px-3 py-1.5
                     transition-colors duration-(--duration-quick) hover:bg-surface-2"
            >
              <button
                type="button"
                class="flex min-w-0 flex-1 items-start gap-2.5 text-left"
                onclick={() => void run(() => clipboard.paste(item.id), t("toast.pasted"))}
                title={t("page.clipboard.pasteActive")}
              >
                <span
                  class="mt-0.5 grid size-10 shrink-0 place-items-center overflow-hidden
                         rounded-sm bg-surface-2"
                  aria-hidden="true"
                >
                  {#if item.kind === "image" && item.imagePath}
                    <img
                      src={convertFileSrc(item.imagePath)}
                      alt=""
                      class="size-full object-cover"
                      loading="lazy"
                      draggable="false"
                    />
                  {:else}
                    <span class="text-micro font-semibold text-muted">Aa</span>
                  {/if}
                </span>

                <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span class="line-clamp-2 text-sm text-text">
                    {#if item.kind === "image"}
                      {item.preview || t("page.clipboard.image")}
                    {:else}
                      {item.preview || t("page.clipboard.emptyPreview")}
                    {/if}
                  </span>
                  <span class="font-mono text-xs text-faint" data-numeric>
                    {item.kind === "image" ? t("page.clipboard.kindImage") : t("page.clipboard.kindText")}{formatListWhen(
                      Math.floor(item.createdAtMs / 1000),
                    )}
                  </span>
                </span>
              </button>

              <!-- Los controles solo aparecen al pasar por encima: en una lista
                   larga, tres iconos por fila compiten con el contenido. -->
              <div
                class="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity
                       duration-(--duration-quick)
                       group-hover:opacity-100 focus-within:opacity-100
                       {item.pinned ? 'opacity-100' : ''}"
              >
                <IconButton
                  label={item.pinned ? t("page.clipboard.unpin") : t("page.clipboard.pin")}
                  size="sm"
                  pressed={item.pinned}
                  onclick={() => void run(() => clipboard.pin(item.id, !item.pinned))}
                >
                  <Icon icon={Pin} size={12} />
                </IconButton>
                <IconButton
                  label={t("page.common.delete")}
                  size="sm"
                  variant="danger"
                  onclick={() => void run(() => clipboard.remove(item.id))}
                >
                  <Icon icon={Trash2} size={12} />
                </IconButton>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</ToolPage>
