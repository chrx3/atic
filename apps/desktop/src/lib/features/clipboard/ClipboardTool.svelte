<script lang="ts">
  /**
   * Historial del portapapeles: buscar, fijar, copiar.
   *
   * La acción principal acá es COPIAR, no pegar, y esa es la única diferencia
   * de fondo con el float. Pegar devuelve el foco a la app anterior y le manda
   * Ctrl+V: desde la pill eso es exactamente lo que se quiere, porque flota
   * sobre el documento en el que se estaba escribiendo. Desde la ventana
   * principal la app anterior es cualquiera, y el texto terminaba en un sitio
   * que nadie miró. Pegar sigue estando, pero como acción secundaria y dicho
   * con todas las letras.
   */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { groupByDay } from "$core/dayGroups";
  import { formatListWhen } from "$core/format";
  import { nextIndex } from "$core/listNav";
  import type { ClipboardItem, ClipboardKind } from "$core/types";
  import { clipboard } from "$domain/clipboard.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Chip from "$ui/Chip.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import { Pin, SquareArrowOutUpRight, Trash2 } from "$lib/icons";
  import { parseCssColor, rgbToHex } from "$features/color/colorMath";
  import { t } from "$domain/i18n.svelte";

  let kind = $state<ClipboardKind | "all">("all");
  let focusIndex = $state(0);
  let listEl = $state<HTMLDivElement | null>(null);

  const kindOptions = $derived([
    { value: "all" as const, label: t("page.clipboard.kindAll") },
    { value: "text" as const, label: t("page.clipboard.kindTextOnly") },
    { value: "image" as const, label: t("page.clipboard.kindImageOnly") },
  ]);

  const matching = $derived(
    clipboard.visible.filter((item) => kind === "all" || item.kind === kind),
  );
  /**
   * Los fijados van aparte y no dentro de su día: se fijaron justamente para
   * no tener que buscarlos por fecha.
   */
  const pinned = $derived(matching.filter((item) => item.pinned));
  const rest = $derived(matching.filter((item) => !item.pinned));
  const groups = $derived(
    groupByDay(rest, (item) => Math.floor(item.createdAtMs / 1000)),
  );

  /** Orden plano —fijados primero— para que el teclado recorra todo. */
  const ordered = $derived([...pinned, ...rest]);
  const flatIndex = $derived(new Map(ordered.map((item, index) => [item.id, index])));

  async function run(action: () => Promise<void>, done?: string) {
    try {
      await action();
      if (done) toasts.push(done);
    } catch (error) {
      toastError(error);
    }
  }

  /**
   * Aro interior de la muestra de color, en dos tonos.
   *
   * Uno solo no alcanza: el blanco desaparece contra el tema claro y el negro
   * contra el oscuro, y son justo los dos colores que más se copian.
   */
  const SWATCH_RING =
    "inset 0 0 0 1px rgb(255 255 255 / 22%), inset 0 0 0 1px rgb(0 0 0 / 18%)";

  /** El color de la entrada, o `null` si no es un color. */
  function swatchFor(item: ClipboardItem): string | null {
    if (item.kind === "image") return null;
    const rgb = parseCssColor(item.text || item.preview || "");
    return rgb ? rgbToHex(rgb) : null;
  }

  function focusRow(index: number) {
    focusIndex = index;
    const row = listEl?.querySelector<HTMLElement>(`[data-row="${index}"]`);
    row?.focus();
    row?.scrollIntoView({ block: "nearest" });
  }

  function onRowKeydown(event: KeyboardEvent, item: ClipboardItem) {
    const moved = nextIndex(event.key, focusIndex, ordered.length);
    if (moved !== null) {
      event.preventDefault();
      focusRow(moved);
      return;
    }
    if (event.key === "Delete") {
      event.preventDefault();
      void run(() => clipboard.remove(item.id));
    }
  }
</script>

{#snippet row(item: ClipboardItem)}
  {@const index = flatIndex.get(item.id) ?? 0}
  {@const swatch = swatchFor(item)}
  <li
    class="group flex items-start gap-2 border-b border-line px-3 py-1.5
           transition-colors duration-(--duration-quick) hover:bg-surface-2"
  >
    <button
      type="button"
      class="flex min-w-0 flex-1 items-start gap-2.5 text-left"
      data-row={index}
      title={t("page.common.copy")}
      onfocus={() => (focusIndex = index)}
      onkeydown={(event) => onRowKeydown(event, item)}
      onclick={() => void run(() => clipboard.copy(item.id), t("toast.copied"))}
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
        {:else if swatch}
          <span class="size-full" style:background={swatch} style:box-shadow={SWATCH_RING}
          ></span>
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
          {item.kind === "image"
            ? t("page.clipboard.kindImage")
            : t("page.clipboard.kindText")}{formatListWhen(
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
        label={t("page.clipboard.pasteActive")}
        size="sm"
        onclick={() => void run(() => clipboard.paste(item.id), t("toast.pasted"))}
      >
        <Icon icon={SquareArrowOutUpRight} size={12} />
      </IconButton>
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
{/snippet}

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
      <div class="w-full max-w-72">
        <Input
          type="search"
          bind:value={clipboard.query}
          placeholder={t("page.clipboard.searchPlaceholder")}
          aria-label={t("page.clipboard.search")}
        />
      </div>
      <SegmentedControl
        bind:value={kind}
        options={kindOptions}
        size="sm"
        label={t("page.clipboard.kindFilter")}
      />
    </Toolbar>

    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if ordered.length === 0}
        <EmptyState
          compact
          icon={clipboard.query || kind !== "all" ? undefined : "clipboard"}
          title={clipboard.query || kind !== "all"
            ? t("page.common.nothing")
            : t("page.clipboard.empty")}
          hint={clipboard.query || kind !== "all"
            ? t("page.common.fewerWords")
            : t("page.clipboard.emptyHint")}
        />
      {:else}
        <div bind:this={listEl}>
          {#if pinned.length > 0}
            <p class="day">{t("page.clipboard.pinnedGroup")}</p>
            <ul class="flex flex-col">
              {#each pinned as item (item.id)}
                {@render row(item)}
              {/each}
            </ul>
          {/if}

          {#each groups as group (group.key)}
            <p class="day">{group.label}</p>
            <ul class="flex flex-col">
              {#each group.items as item (item.id)}
                {@render row(item)}
              {/each}
            </ul>
          {/each}
        </div>

        <p class="py-2 text-center text-micro text-faint">
          {t("page.common.keyboardHint")}
        </p>
      {/if}
    </div>
  </div>
</ToolPage>

<style>
  .day {
    position: sticky;
    top: 0;
    z-index: 1;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
    padding: 0.25rem 0.75rem;
    font-size: var(--text-micro);
    letter-spacing: var(--text-micro--letter-spacing);
    color: var(--muted);
    text-transform: uppercase;
  }
</style>
