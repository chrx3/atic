<script lang="ts">
  /** Capturas recientes: mirarlas, copiarlas, leerles el texto. */
  import { formatListWhen, formatShortcut } from "$core/format";
  import { captures } from "$domain/captures.svelte";
  import { config } from "$domain/config.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import { openAnnotator } from "$ipc/annotate";
  import { captureSrc } from "$ipc/captures";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import Modal from "$ui/Modal.svelte";
  import { Copy, ScanText, Trash2 } from "$lib/icons";
  import type { CaptureItem } from "$core/types";

  const shortcut = $derived(config.current?.screenshot_shortcut ?? "");

  let preview = $state<CaptureItem | null>(null);

  async function run(action: () => Promise<unknown>, done?: string) {
    try {
      const result = await action();
      if (done) toasts.push(done);
      return result;
    } catch (error) {
      toastError(error);
    }
  }

  async function ocr(path: string) {
    const text = await run(() => captures.ocr(path));
    if (typeof text === "string") {
      toasts.push(text.trim() ? t("toast.textCopied") : t("toast.noText"));
    }
  }

  /**
   * El editor abre en su propia ventana flotante, y por eso la vista previa se
   * cierra: dejarla debajo mostraría dos copias de la misma imagen.
   */
  async function annotate(path: string) {
    await run(() => openAnnotator(path));
    preview = null;
  }
</script>

<ToolPage
  title={t("tools.captures.label")}
  icon="captures"
  kicker={t("tools.captures.short")}
  blurb={t("tools.captures.blurb")}
>
  {#snippet meta()}
    <Chip>{t("page.captures.count", { count: captures.items.length })}</Chip>
    {#if shortcut}
      <Kbd combo={formatShortcut(shortcut)} separator="+" />
    {/if}
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label={t("page.captures.actions")}>
      {#snippet end()}
        <Button
          variant="soft"
          size="sm"
          onclick={() =>
            void run(async () => {
              const removed = await captures.cleanup();
              toasts.push(
                removed > 0
                  ? t("settings.captures.cleaned", { count: removed })
                  : t("toast.nothingToClean"),
              );
            })}
        >
          {t("page.captures.cleanup")}
        </Button>
      {/snippet}
      <span class="text-xs text-muted">
        {t("page.captures.retention")}
      </span>
    </Toolbar>

    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      {#if captures.items.length === 0}
        <EmptyState
          compact
          icon="captures"
          title={t("page.captures.empty")}
          hint={shortcut
            ? t("page.captures.emptyHint", { shortcut: formatShortcut(shortcut) })
            : t("page.captures.emptyNoShortcut")}
        />
      {:else}
        <div
          class="@container/grid grid grid-cols-3 gap-2 @md/grid:grid-cols-4
                 @lg/grid:grid-cols-5"
        >
          {#each captures.items as item (item.id)}
            <figure
              class="group flex flex-col overflow-hidden rounded-sm border border-line
                     bg-surface"
            >
              <button
                type="button"
                class="block aspect-video w-full overflow-hidden bg-surface-2"
                title={t("page.captures.zoom")}
                onclick={() => (preview = item)}
              >
                <!-- `object-contain`: se ve la captura entera, no un recorte zoom. -->
                <img
                  src={captureSrc(item.path)}
                  alt={t("page.captures.alt", { label: item.label })}
                  loading="lazy"
                  class="size-full object-contain"
                />
              </button>

              <figcaption
                class="flex items-center gap-1 border-t border-line px-2 py-1"
              >
                <span
                  class="min-w-0 flex-1 truncate font-mono text-xs text-faint"
                  data-numeric
                >
                  {formatListWhen(Math.floor(item.createdAtMs / 1000))}
                </span>

                <div
                  class="flex shrink-0 items-center gap-0.5 opacity-0
                         transition-opacity duration-(--duration-quick)
                         group-hover:opacity-100 focus-within:opacity-100"
                >
                  <IconButton
                    label={t("page.captures.copyImage")}
                    size="sm"
                    onclick={() => void run(() => captures.copy(item.path), t("toast.copiedImage"))}
                  >
                    <Icon icon={Copy} size={12} />
                  </IconButton>
                  <IconButton
                    label={t("page.captures.copyOcr")}
                    size="sm"
                    onclick={() => void ocr(item.path)}
                  >
                    <Icon icon={ScanText} size={12} />
                  </IconButton>
                  <IconButton
                    label={t("page.common.delete")}
                    size="sm"
                    variant="danger"
                    onclick={() => void run(() => captures.remove(item.path))}
                  >
                    <Icon icon={Trash2} size={12} />
                  </IconButton>
                </div>
              </figcaption>
            </figure>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</ToolPage>

{#if preview}
  {@const item = preview}
  <Modal
    title={t("page.captures.preview")}
    subtitle={formatListWhen(Math.floor(item.createdAtMs / 1000))}
    size="lg"
    panelMax="min(90dvh, 880px)"
    onClose={() => (preview = null)}
  >
    {#snippet actions()}
      <Button
        variant="soft"
        size="sm"
        onclick={() => void run(() => captures.copy(item.path), t("toast.copiedImage"))}
      >
        {t("page.common.copy")}
      </Button>
      <Button variant="soft" size="sm" onclick={() => void annotate(item.path)}>
        {t("page.captures.draw")}
      </Button>
      <Button
        variant="soft"
        size="sm"
        onclick={() => void run(() => captures.open(item.path))}
      >
        {t("page.common.open")}
      </Button>
      <Button variant="primary" size="sm" onclick={() => (preview = null)}>
        {t("page.common.close")}
      </Button>
    {/snippet}

    <div
      class="flex max-h-[min(70dvh,720px)] items-center justify-center
             overflow-auto rounded-sm bg-surface-2 p-2"
    >
      <img
        src={captureSrc(item.path)}
        alt={t("page.captures.alt", { label: item.label })}
        class="max-h-[min(66dvh,680px)] max-w-full object-contain"
      />
    </div>
  </Modal>
{/if}
