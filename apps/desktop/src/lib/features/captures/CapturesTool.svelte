<script lang="ts">
  /**
   * Capturas recientes: mirarlas, copiarlas, leerles el texto.
   *
   * Es una grilla y no una lista porque lo que identifica a una captura es la
   * imagen. De ahí sale el resto: llegan de a montones por un atajo, nadie las
   * nombra, y la única forma de encontrar una es reconocerla. Por eso se
   * agrupan por día —el «cuándo» es lo único que se recuerda de ellas— y por
   * eso se recorren con el teclado como cualquier carpeta de imágenes.
   */
  import { fuzzyMatch } from "$core/clipboardSearch";
  import { groupByDay } from "$core/dayGroups";
  import { formatListWhen, formatShortcut } from "$core/format";
  import { nextIndex } from "$core/listNav";
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
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import Modal from "$ui/Modal.svelte";
  import { ArrowLeft, ArrowRight, Copy, Folder, ScanText, Trash2 } from "$lib/icons";
  import type { CaptureItem } from "$core/types";

  const shortcut = $derived(config.current?.screenshot_shortcut ?? "");

  let query = $state("");
  let previewId = $state<string | null>(null);
  let focusIndex = $state(0);
  let grid = $state<HTMLDivElement | null>(null);

  /** Ids elegidos para una acción en lote. El orden no importa. */
  let selected = $state<string[]>([]);
  /** Desde dónde cuenta un clic con Shift. Fuera de `$state`: no se dibuja. */
  let rangeAnchor = -1;
  let confirmingBulk = $state(false);
  let deletingBulk = $state(false);

  /** El texto leído de la captura que se está mirando. `null` = no se pidió. */
  let ocrText = $state<string | null>(null);
  let ocrBusy = $state(false);

  const secondsOf = (item: CaptureItem) => Math.floor(item.createdAtMs / 1000);

  const visible = $derived(
    captures.items.filter((item) =>
      fuzzyMatch(`${item.label}\n${formatListWhen(secondsOf(item))}`, query),
    ),
  );
  const groups = $derived(groupByDay(visible, secondsOf));
  /** Índice plano de cada ítem: el teclado recorre la grilla entera, no un día. */
  const flatIndex = $derived(new Map(visible.map((item, index) => [item.id, index])));
  const chosen = $derived(new Set(selected));

  const previewIndex = $derived(
    previewId === null ? -1 : visible.findIndex((item) => item.id === previewId),
  );
  const previewItem = $derived(previewIndex < 0 ? null : visible[previewIndex]);

  async function run<T>(action: () => Promise<T>, done?: string) {
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
    previewId = null;
  }

  // --- Recorrer la grilla ---

  /**
   * Cuántas columnas hay ahora mismo.
   *
   * Se lee del estilo calculado en vez de repetir los breakpoints en JS: la
   * grilla es `auto-fill`, así que el número depende del ancho real del panel
   * y no de una tabla que habría que mantener a la par del CSS.
   */
  function columnCount(): number {
    if (!grid) return 1;
    const columns = getComputedStyle(grid).gridTemplateColumns;
    return Math.max(1, columns.split(" ").filter(Boolean).length);
  }

  function focusTile(index: number) {
    focusIndex = index;
    const tile = grid?.querySelector<HTMLElement>(`[data-tile="${index}"]`);
    tile?.focus();
    tile?.scrollIntoView({ block: "nearest" });
  }

  function onGridKeydown(event: KeyboardEvent) {
    const moved = nextIndex(event.key, focusIndex, visible.length, columnCount());
    if (moved !== null) {
      event.preventDefault();
      focusTile(moved);
      return;
    }

    if (event.key === "Escape" && selected.length > 0) {
      // Cortar la propagación no alcanza: el workspace mira `defaultPrevented`
      // para no cerrarse cuando el Esc ya sirvió para algo acá.
      event.preventDefault();
      selected = [];
      return;
    }

    if (event.key === "Delete" && visible.length > 0) {
      event.preventDefault();
      if (selected.length > 0) {
        confirmingBulk = true;
        return;
      }
      const item = visible[focusIndex];
      if (item) void run(() => captures.remove(item.path));
    }
  }

  // --- Elegir varias ---

  function toggle(id: string) {
    selected = chosen.has(id)
      ? selected.filter((value) => value !== id)
      : [...selected, id];
  }

  function selectThrough(index: number) {
    const from = rangeAnchor < 0 ? index : rangeAnchor;
    const [start, end] = from <= index ? [from, index] : [index, from];
    const ids = visible.slice(start, end + 1).map((item) => item.id);
    selected = [...new Set([...selected, ...ids])];
  }

  /**
   * Clic en una captura: abrir, o sumar a la selección.
   *
   * Ctrl y Shift hacen lo que hacen en cualquier explorador de archivos, y por
   * eso no hay un «modo selección» que haya que encender antes: el clic normal
   * sigue abriendo aunque ya haya cosas elegidas.
   */
  function onTileClick(event: MouseEvent, index: number, item: CaptureItem) {
    if (event.shiftKey) {
      event.preventDefault();
      selectThrough(index);
      return;
    }
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      rangeAnchor = index;
      toggle(item.id);
      return;
    }
    openPreview(item.id);
  }

  async function deleteSelected() {
    const targets = captures.items.filter((item) => chosen.has(item.id));
    deletingBulk = true;
    try {
      for (const item of targets) await captures.remove(item.path);
      selected = [];
      confirmingBulk = false;
    } catch (error) {
      toastError(error);
    } finally {
      deletingBulk = false;
    }
  }

  // --- Vista previa ---

  function openPreview(id: string) {
    previewId = id;
    ocrText = null;
    void loadCachedOcr(id);
  }

  /** Lo que Rust ya leyó de esta captura. Si no hay, se ofrece leerla. */
  async function loadCachedOcr(id: string) {
    const item = captures.items.find((entry) => entry.id === id);
    if (!item) return;
    try {
      const cached = await captures.ocrCached(item.path);
      if (previewId === id && typeof cached === "string") ocrText = cached;
    } catch {
      // Sin caché se lee a pedido; no es un fallo que valga interrumpir.
    }
  }

  async function readOcr(item: CaptureItem) {
    ocrBusy = true;
    try {
      const text = await captures.ocrText(item.path);
      if (previewId === item.id) ocrText = text;
    } catch (error) {
      toastError(error);
    } finally {
      ocrBusy = false;
    }
  }

  function stepPreview(delta: number) {
    if (previewIndex < 0 || visible.length === 0) return;
    const next = Math.max(0, Math.min(visible.length - 1, previewIndex + delta));
    if (next !== previewIndex) openPreview(visible[next].id);
  }

  function onPreviewKeydown(event: KeyboardEvent) {
    if (!previewItem) return;
    if (event.key === "ArrowRight") {
      event.preventDefault();
      stepPreview(1);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      stepPreview(-1);
    }
  }
</script>

<svelte:window onkeydown={onPreviewKeydown} />

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
      <div class="w-full max-w-72">
        <Input
          type="search"
          bind:value={query}
          placeholder={t("page.captures.searchPlaceholder")}
          aria-label={t("page.captures.searchAria")}
        />
      </div>

      {#snippet end()}
        <span class="text-xs text-muted">
          {t("page.captures.retention")}
        </span>
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
    </Toolbar>

    <!-- La barra de selección solo existe cuando hay algo elegido: un lugar
         vacío reservado para ella confundiría más de lo que anticipa. -->
    {#if selected.length > 0}
      <div class="pick-bar">
        <span class="text-xs font-medium text-text">
          {t("page.captures.selectedCount", { count: selected.length })}
        </span>
        <div class="ml-auto flex items-center gap-1.5">
          <Button variant="ghost" size="sm" onclick={() => (selected = [])}>
            {t("page.captures.clearSelection")}
          </Button>
          <Button variant="danger" size="sm" onclick={() => (confirmingBulk = true)}>
            {t("page.captures.deleteSelected")}
          </Button>
        </div>
      </div>
    {/if}

    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      {#if visible.length === 0}
        <EmptyState
          compact
          icon={query ? undefined : "captures"}
          title={query ? t("page.common.nothing") : t("page.captures.empty")}
          hint={query
            ? t("page.common.fewerWords")
            : shortcut
              ? t("page.captures.emptyHint", { shortcut: formatShortcut(shortcut) })
              : t("page.captures.emptyNoShortcut")}
        />
      {:else}
        <div
          bind:this={grid}
          class="shots"
          role="group"
          aria-label={t("page.captures.grid")}
        >
          {#each groups as group (group.key)}
            <h3 class="day">{group.label}</h3>

            {#each group.items as item (item.id)}
              {@const index = flatIndex.get(item.id) ?? 0}
              <figure class="tile" class:tile--on={chosen.has(item.id)}>
                <button
                  type="button"
                  class="shot"
                  data-tile={index}
                  tabindex={index === focusIndex ? 0 : -1}
                  title={t("page.captures.zoom")}
                  aria-label={t("page.captures.alt", { label: item.label })}
                  onfocus={() => (focusIndex = index)}
                  onkeydown={onGridKeydown}
                  onclick={(event) => onTileClick(event, index, item)}
                >
                  <!-- `object-contain`: se ve la captura entera, no un recorte. -->
                  <img
                    src={captureSrc(item.path)}
                    alt=""
                    loading="lazy"
                    class="size-full object-contain"
                  />
                </button>

                <label class="pick">
                  <input
                    type="checkbox"
                    checked={chosen.has(item.id)}
                    aria-label={t("page.captures.select")}
                    onchange={() => {
                      rangeAnchor = index;
                      toggle(item.id);
                    }}
                  />
                </label>

                <figcaption class="cap">
                  <span
                    class="min-w-0 flex-1 truncate font-mono text-xs text-faint"
                    data-numeric
                  >
                    {formatListWhen(secondsOf(item))}
                  </span>

                  <div class="acts">
                    <IconButton
                      label={t("page.captures.copyImage")}
                      size="sm"
                      onclick={() =>
                        void run(
                          () => captures.copy(item.path),
                          t("toast.copiedImage"),
                        )}
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
                      label={t("page.captures.reveal")}
                      size="sm"
                      onclick={() => void run(() => captures.reveal(item.path))}
                    >
                      <Icon icon={Folder} size={12} />
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
          {/each}
        </div>

        <p class="mt-3 text-center text-micro text-faint">
          {t("page.common.keyboardHint")}
        </p>
      {/if}
    </div>
  </div>
</ToolPage>

{#if previewItem}
  {@const item = previewItem}
  <Modal
    title={t("page.captures.preview")}
    subtitle={formatListWhen(secondsOf(item))}
    size="xl"
    panelMax="min(92dvh, 940px)"
    onClose={() => (previewId = null)}
  >
    {#snippet actions()}
      <IconButton
        label={t("page.captures.prev")}
        size="sm"
        disabled={previewIndex <= 0}
        onclick={() => stepPreview(-1)}
      >
        <Icon icon={ArrowLeft} size={14} />
      </IconButton>
      <span class="px-1 font-mono text-xs text-faint" data-numeric>
        {t("page.captures.position", {
          index: previewIndex + 1,
          total: visible.length,
        })}
      </span>
      <IconButton
        label={t("page.captures.next")}
        size="sm"
        disabled={previewIndex >= visible.length - 1}
        onclick={() => stepPreview(1)}
      >
        <Icon icon={ArrowRight} size={14} />
      </IconButton>
    {/snippet}

    <div class="flex min-h-0 flex-col gap-2">
      <div class="stage">
        <img
          src={captureSrc(item.path)}
          alt={t("page.captures.alt", { label: item.label })}
          class="max-h-full max-w-full object-contain"
        />
      </div>

      <div class="flex flex-wrap items-center gap-1.5">
        <Button
          variant="primary"
          size="sm"
          onclick={() =>
            void run(() => captures.copy(item.path), t("toast.copiedImage"))}
        >
          {t("page.common.copy")}
        </Button>
        <Button variant="soft" size="sm" onclick={() => void annotate(item.path)}>
          {t("page.captures.draw")}
        </Button>
        <Button
          variant="soft"
          size="sm"
          loading={ocrBusy}
          onclick={() => void readOcr(item)}
        >
          {t("page.captures.ocrRead")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onclick={() => void run(() => captures.open(item.path))}
        >
          {t("page.common.open")}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onclick={() => void run(() => captures.reveal(item.path))}
        >
          {t("page.captures.reveal")}
        </Button>
        <Button
          variant="danger"
          size="sm"
          onclick={() =>
            void run(async () => {
              const at = previewIndex;
              await captures.remove(item.path);
              // Seguir mirando: se cae a la que ocupó su lugar, o a la
              // anterior. Se mira `visible` y no la lista entera porque el
              // índice era de la lista filtrada: con una búsqueda puesta,
              // saltaría a una captura que no está en pantalla.
              previewId = visible[at]?.id ?? visible[at - 1]?.id ?? null;
            })}
        >
          {t("page.common.delete")}
        </Button>
      </div>

      {#if ocrText !== null}
        <section class="ocr">
          <h4 class="text-micro text-muted uppercase">
            {t("page.captures.ocrPanel")}
          </h4>
          {#if ocrText.trim()}
            <p class="ocr-body">{ocrText}</p>
            <div>
              <Button variant="soft" size="sm" onclick={() => void ocr(item.path)}>
                {t("page.captures.ocrCopy")}
              </Button>
            </div>
          {:else}
            <p class="text-xs text-faint">{t("page.captures.ocrNone")}</p>
          {/if}
        </section>
      {/if}
    </div>
  </Modal>
{/if}

{#if confirmingBulk}
  <ConfirmDialog
    title={t("page.captures.deleteSelectedTitle", { count: selected.length })}
    body={t("page.captures.deleteSelectedBody")}
    confirmLabel={t("page.common.delete")}
    tone="danger"
    busy={deletingBulk}
    onConfirm={() => void deleteSelected()}
    onCancel={() => (confirmingBulk = false)}
  />
{/if}

<style>
  .pick-bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.5rem;
    border-bottom: 1px solid var(--line);
    background: var(--surface-2);
    padding: 0.375rem 0.75rem;
  }

  /*
   * `auto-fill` y no columnas fijas: con la ventana maximizada el panel pasa
   * de 800 a 1900 px, y una tabla de breakpoints se desincroniza del conteo
   * de columnas que usa el teclado. Acá el CSS es la única fuente.
   */
  .shots {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(196px, 1fr));
    gap: 0.5rem;
    align-content: start;
  }

  /* El día ocupa la fila entera: es un corte, no una celda más. */
  .day {
    grid-column: 1 / -1;
    margin-top: 0.375rem;
    font-size: var(--text-micro);
    letter-spacing: var(--text-micro--letter-spacing);
    color: var(--muted);
    text-transform: uppercase;
  }

  .day:first-child {
    margin-top: 0;
  }

  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--surface);
    transition: border-color var(--duration-quick) var(--ease-calm);
  }

  .tile--on {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .shot {
    display: block;
    aspect-ratio: 16 / 9;
    width: 100%;
    overflow: hidden;
    border: 0;
    padding: 0;
    background: var(--surface-2);
    cursor: pointer;
  }

  .shot:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  /* La casilla solo aparece al apuntar la captura o cuando ya está elegida:
     una por celda, siempre visible, compite con la imagen. */
  .pick {
    position: absolute;
    top: 0.25rem;
    left: 0.25rem;
    display: grid;
    place-items: center;
    border-radius: var(--radius-xs);
    background: color-mix(in sRGB, var(--bg) 70%, transparent);
    padding: 0.125rem;
    opacity: 0;
    transition: opacity var(--duration-quick) var(--ease-calm);
  }

  .tile:hover .pick,
  .tile:focus-within .pick,
  .tile--on .pick {
    opacity: 1;
  }

  .pick input {
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .cap {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    border-top: 1px solid var(--line);
    padding: 0.25rem 0.5rem;
  }

  .acts {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.125rem;
    opacity: 0;
    transition: opacity var(--duration-quick) var(--ease-calm);
  }

  .tile:hover .acts,
  .tile:focus-within .acts {
    opacity: 1;
  }

  .stage {
    display: flex;
    max-height: min(64dvh, 640px);
    min-height: 0;
    flex: 1;
    align-items: center;
    justify-content: center;
    overflow: auto;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    padding: 0.5rem;
  }

  .ocr {
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    border-top: 1px solid var(--line);
    padding-top: 0.5rem;
  }

  .ocr-body {
    max-height: 9rem;
    overflow-y: auto;
    font-size: 0.75rem;
    line-height: 1.5;
    color: var(--text);
    white-space: pre-wrap;
  }
</style>
