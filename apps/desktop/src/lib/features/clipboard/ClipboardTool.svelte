<script lang="ts">
  /**
   * Historial del portapapeles: buscar, mirar, copiar.
   *
   * Es una biblioteca, no un menú: cada elemento se abre en el detalle —el
   * texto completo, la imagen, la muestra de color— y las acciones viven ahí,
   * visibles y con nombre. La fila solo elige, como en cualquier lista del
   * sistema; antes cada fila era un botón de copiar con tres iconos que
   * aparecían al pasar el mouse, y nada se podía leer entero.
   *
   * En la ventana la acción principal es COPIAR: `Pegar` devuelve el foco a la
   * app anterior y le manda Ctrl+V, y desde acá esa app es cualquiera. Sigue
   * estando —es exactamente lo que se quiere cuando uno vino a la ventana para
   * pegar en otro lado— pero con su nombre completo.
   */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { groupByDay } from "$core/dayGroups";
  import { formatListWhen } from "$core/format";
  import { nextIndex } from "$core/listNav";
  import type { ClipboardItem, ClipboardKind } from "$core/types";
  import { clipboard } from "$domain/clipboard.svelte";
  import { t } from "$domain/i18n.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { parseCssColor, rgbToHex } from "$features/color/colorMath";
  import { Ellipsis, Pin, Trash2 } from "$lib/icons";
  import ListDetail from "$patterns/ListDetail.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import Input from "$ui/Input.svelte";
  import Menu from "$ui/Menu.svelte";
  import type { MenuItem } from "$ui/menu";
  import SegmentedControl from "$ui/SegmentedControl.svelte";

  let kind = $state<ClipboardKind | "all">("all");
  /** Lo elegido en la lista. `null` = la primera de arriba. */
  let selectedId = $state<string | null>(null);
  /** Elemento esperando confirmación de borrado. */
  let confirmingDeleteId = $state<string | null>(null);
  let listEl = $state<HTMLDivElement | null>(null);

  const confirmTarget = $derived(
    clipboard.items.find((item) => item.id === confirmingDeleteId) ?? null,
  );

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

  /**
   * Lo que muestra el detalle. Si no hay nada elegido todavía —o lo elegido
   * desapareció al filtrar o borrar— vale la primera: la ventana nunca queda
   * con un panel vacío teniendo algo que mirar.
   */
  const selected = $derived(
    ordered.find((item) => item.id === selectedId) ?? ordered[0] ?? null,
  );
  const selectedIndex = $derived(selected ? (flatIndex.get(selected.id) ?? -1) : -1);

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

  const when = (item: ClipboardItem) =>
    formatListWhen(Math.floor(item.createdAtMs / 1000));

  /** «texto · », «color · » o «imagen · »: el prefijo que ya usa la pill. */
  function kindLabel(item: ClipboardItem): string {
    if (item.kind === "image") return t("page.clipboard.kindImage");
    return swatchFor(item)
      ? t("page.clipboard.kindColor")
      : t("page.clipboard.kindText");
  }

  async function run(action: () => Promise<void>, done?: string) {
    try {
      await action();
      if (done) toasts.push(done);
    } catch (error) {
      toastError(error);
    }
  }

  async function copy(item: ClipboardItem) {
    await run(() => clipboard.copy(item.id), t("toast.copied"));
  }

  async function deleteConfirmed() {
    const item = confirmTarget;
    if (!item) return;
    confirmingDeleteId = null;
    await run(() => clipboard.remove(item.id));
  }

  // --- El menú del detalle ---

  type DetailPick = "pin" | "delete";

  const detailItems = $derived.by((): MenuItem<DetailPick>[] => {
    const item = selected;
    if (!item) return [];
    return [
      {
        id: "pin",
        label: item.pinned ? t("page.clipboard.unpin") : t("page.clipboard.pin"),
        icon: Pin,
      },
      { id: "delete", label: t("page.common.delete"), icon: Trash2, danger: true },
    ];
  });

  function onDetailPick(id: DetailPick) {
    const item = selected;
    if (!item) return;
    if (id === "pin") void run(() => clipboard.pin(item.id, !item.pinned));
    else confirmingDeleteId = item.id;
  }

  // --- Recorrer la lista ---

  function focusRow(index: number) {
    const row = listEl?.querySelector<HTMLElement>(`[data-row="${index}"]`);
    row?.focus();
    row?.scrollIntoView({ block: "nearest" });
  }

  /**
   * Las flechas recorren, Enter copia y Supr borra. El foco elige —a diferencia
   * del clic, que también elige—: lo que se mira es lo que se va a copiar.
   */
  function onRowKeydown(event: KeyboardEvent, item: ClipboardItem) {
    const moved = nextIndex(event.key, selectedIndex, ordered.length);
    if (moved !== null) {
      event.preventDefault();
      selectedId = ordered[moved]?.id ?? null;
      focusRow(moved);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      void copy(item);
      return;
    }
    if (event.key === "Delete") {
      event.preventDefault();
      confirmingDeleteId = item.id;
    }
  }
</script>

{#snippet moreTrigger()}
  <Icon icon={Ellipsis} size={14} />
{/snippet}

{#snippet row(item: ClipboardItem)}
  {@const index = flatIndex.get(item.id) ?? 0}
  {@const swatch = swatchFor(item)}
  <li>
    <button
      type="button"
      data-row={index}
      aria-current={selected?.id === item.id ? "true" : undefined}
      onfocus={() => (selectedId = item.id)}
      onkeydown={(event) => onRowKeydown(event, item)}
      onclick={() => (selectedId = item.id)}
    >
      <span class="flex w-full items-start gap-2.5">
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
            <span
              class="size-full"
              style:background={swatch}
              style:box-shadow={SWATCH_RING}
            ></span>
          {:else}
            <span class="text-micro font-semibold text-muted">Aa</span>
          {/if}
        </span>

        <span class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="line-clamp-2 text-sm text-text">
            {item.preview || t("page.clipboard.emptyPreview")}
          </span>
          <span class="font-mono text-xs text-faint" data-numeric>
            {kindLabel(item)}{when(item)}
          </span>
        </span>
      </span>
    </button>
  </li>
{/snippet}

<ToolPage
  title={t("tools.clipboard.label")}
  icon="clipboard"
  kicker={t("tools.clipboard.short")}
  blurb={t("tools.clipboard.blurb")}
>
  <div class="flex h-full min-h-0 flex-col">
    <ListDetail
      hasSelection={selected !== null}
      listLabel={t("page.clipboard.list")}
      listCount={ordered.length}
    >
      {#snippet listHeader()}
        <div class="flex flex-col gap-1.5">
          <Input
            type="search"
            bind:value={clipboard.query}
            placeholder={t("page.clipboard.searchPlaceholder")}
            aria-label={t("page.clipboard.search")}
          />
          <!-- El tipo filtra la lista que está justo abajo, así que vive con
               ella y no en una barra aparte. -->
          <SegmentedControl
            bind:value={kind}
            options={kindOptions}
            size="sm"
            full
            label={t("page.clipboard.kindFilter")}
          />
        </div>
      {/snippet}

      {#snippet list()}
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
            {t("page.clipboard.keyboardHint")}
          </p>
        {/if}
      {/snippet}

      {#snippet detail()}
        {#if selected}
          {@const item = selected}
          {@const swatch = swatchFor(item)}
          <div class="flex flex-col gap-3">
            <div class="flex items-start gap-2">
              <div class="flex min-w-0 flex-1 flex-wrap items-center gap-2">
                {#if item.pinned}
                  <Chip tone="info">{t("page.clipboard.pinned")}</Chip>
                {/if}
                <span class="font-mono text-xs text-faint" data-numeric>
                  {kindLabel(item)}{when(item)}
                </span>
              </div>
              <Menu
                items={detailItems}
                label={t("page.common.more")}
                triggerLabel={t("page.common.more")}
                align="end"
                onpick={onDetailPick}
                triggerClass="grid size-6 place-items-center rounded-sm text-muted
                              transition-colors duration-(--duration-quick) ease-calm
                              hover:bg-surface-2 hover:text-text
                              focus-visible:[outline:2px_solid_var(--accent)]"
                trigger={moreTrigger}
              />
            </div>

            {#if item.kind === "image" && item.imagePath}
              <div class="flex justify-center rounded-md bg-surface-2 p-2">
                <img
                  src={convertFileSrc(item.imagePath)}
                  alt={item.preview || t("page.clipboard.image")}
                  class="max-h-72 w-full rounded-sm object-contain"
                  draggable="false"
                />
              </div>
            {:else if swatch}
              <div class="flex items-center gap-3 rounded-md bg-surface-2 p-3">
                <span
                  class="size-12 shrink-0 rounded-sm"
                  style:background={swatch}
                  style:box-shadow={SWATCH_RING}
                ></span>
                <span class="font-mono text-sm text-text select-text">{swatch}</span>
              </div>
            {:else}
              <div class="max-h-72 overflow-y-auto rounded-md bg-surface-2 p-3">
                <p
                  class="text-sm break-words whitespace-pre-wrap text-text select-text"
                >
                  {item.text || item.preview || t("page.clipboard.emptyPreview")}
                </p>
              </div>
            {/if}

            <div class="flex flex-wrap items-center gap-1.5">
              <Button variant="primary" size="sm" onclick={() => void copy(item)}>
                {t("page.common.copy")}
              </Button>
              <Button
                variant="soft"
                size="sm"
                onclick={() =>
                  void run(() => clipboard.paste(item.id), t("toast.pasted"))}
              >
                {t("page.clipboard.pasteActive")}
              </Button>
              {#if item.kind !== "image"}
                <span class="ml-auto font-mono text-xs text-faint" data-numeric>
                  {t("page.clipboard.chars", {
                    count: (item.text || item.preview || "").length,
                  })}
                </span>
              {/if}
            </div>
          </div>
        {/if}
      {/snippet}

      {#snippet empty()}
        <EmptyState
          compact
          icon="clipboard"
          title={t("page.clipboard.pickOne")}
          hint={t("page.common.pickOneHint")}
        />
      {/snippet}
    </ListDetail>
  </div>
</ToolPage>

{#if confirmTarget}
  <ConfirmDialog
    title={t("page.clipboard.deleteTitle")}
    body={t("page.clipboard.deleteBody")}
    confirmLabel={t("page.common.delete")}
    tone="danger"
    onConfirm={() => void deleteConfirmed()}
    onCancel={() => (confirmingDeleteId = null)}
  />
{/if}

<style>
  /* El día corta la lista; no es una fila más, así que no se puede elegir. */
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
