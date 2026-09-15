<script lang="ts">
  /**
   * Reuniones: grabar, ver lo grabado, transcribir y resumir.
   *
   * Dos capas y no cinco. Una sola barra: la grabadora, que es además la única
   * pieza con color y movimiento de la pantalla (medidores y cronómetro solo
   * mientras entra audio). Y el par lista/detalle, sin rótulos que repitan lo
   * que el contenido ya dice.
   *
   * El detalle muestra el contenido primero —reproductor y transcripción— y
   * deja lo secundario en el menú `⋯`; el motor de transcripción es un ajuste y
   * vive en Ajustes, así que acá solo queda una línea que dice con cuál corre.
   *
   * Igual que antes: acá no hay estado de dominio. Lo único local es lo que no
   * sale de la ventana — qué confirmación está abierta, qué se filtra, qué se
   * renombra.
   */
  import { fuzzyMatch } from "$core/clipboardSearch";
  import { groupByDay } from "$core/dayGroups";
  import { formatDate, formatDuration } from "$core/format";
  import { nextIndex } from "$core/listNav";
  import type { Recording, RecordingStatus } from "$core/types";
  import { capture } from "$domain/capture.svelte";
  import { models } from "$domain/models.svelte";
  import { defaultTrack, playback } from "$domain/playback.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { summaries } from "$domain/summaries.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t, whisperModelLabel } from "$domain/i18n.svelte";
  import type { SettingsSectionId } from "$features/settings/settingsSections";
  import { pickAudioFiles } from "$ipc/dialogs";
  import { openRecordingDir } from "$ipc/recordings";
  import { Ellipsis, Folder, Pencil, SlidersHorizontal, Trash2 } from "$lib/icons";
  import ListDetail from "$patterns/ListDetail.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import Menu from "$ui/Menu.svelte";
  import type { MenuItem } from "$ui/menu";
  import Meter from "$ui/Meter.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import LiveTranscript from "./LiveTranscript.svelte";
  import RecordingPlayer from "./RecordingPlayer.svelte";
  import SummaryPanel from "./SummaryPanel.svelte";
  import TranscriptPanel from "./TranscriptPanel.svelte";
  import TranscriptView from "./TranscriptView.svelte";

  let { onOpenSettings }: { onOpenSettings?: (section?: SettingsSectionId) => void } =
    $props();

  let toDelete = $state<Recording | null>(null);
  let deleting = $state(false);
  let importing = $state(false);
  let transcriptFor = $state<Recording | null>(null);
  let summaryFor = $state<Recording | null>(null);
  let openingFolder = $state(false);

  let query = $state("");
  let statusFilter = $state<RecordingStatus | "all">("all");
  let listEl = $state<HTMLDivElement | null>(null);

  /** El título en edición. `null` cuando no se está renombrando. */
  let draftTitle = $state<string | null>(null);
  let renaming = $state(false);

  const TONE = {
    recorded: "neutral",
    transcribing: "info",
    transcribed: "ok",
    summarizing: "info",
    summarized: "ok",
    error: "danger",
  } as const;

  const STATUSES: RecordingStatus[] = [
    "recorded",
    "transcribing",
    "transcribed",
    "summarizing",
    "summarized",
    "error",
  ];

  const statusOptions = $derived([
    { value: "all" as const, label: t("page.meetings.statusAll") },
    ...STATUSES.map((status) => ({
      value: status,
      label: t(`page.meetings.status.${status}`),
    })),
  ]);

  const filterItems = $derived.by((): MenuItem<RecordingStatus | "all">[] =>
    statusOptions.map((option) => ({
      id: option.value,
      label: option.label,
      checked: option.value === statusFilter,
    })),
  );

  const visible = $derived(
    recordings.items.filter(
      (item) =>
        (statusFilter === "all" || item.status === statusFilter) &&
        fuzzyMatch(`${item.title}\n${formatDate(item.started_at)}`, query),
    ),
  );

  const groups = $derived(
    groupByDay(visible, (item) => Math.floor(Date.parse(item.started_at) / 1000)),
  );
  const flatIndex = $derived(new Map(visible.map((item, index) => [item.id, index])));
  const selectedIndex = $derived(
    recordings.selectedId === null ? -1 : (flatIndex.get(recordings.selectedId) ?? -1),
  );

  /**
   * Con qué corre la transcripción. Es la única huella que queda del motor en
   * esta pantalla: el ajuste se cambia en Ajustes › Reuniones.
   */
  const engine = $derived(
    `${models.meetingUsesGroq ? t("settings.meetings.groq") : t("settings.meetings.local")} · ${models.meetingProgressLabel}`,
  );

  /**
   * Si hay transcripción que mostrar. Se mira el estado de la grabación y no
   * la caché porque el texto recién se pide al montar `TranscriptView`: si
   * dependiera de la caché, el detalle nunca lo montaría.
   */
  const hasTranscript = $derived(
    recordings.selected !== null &&
      ["transcribed", "summarized", "summarizing"].includes(recordings.selected.status),
  );

  async function toggle() {
    try {
      await capture.toggle();
    } catch (error) {
      toastError(error);
    }
  }

  async function transcribe(id: string) {
    try {
      await recordings.transcribe(id);
    } catch (error) {
      toastError(error);
    }
  }

  async function openThisRecording(id: string) {
    if (openingFolder) return;
    openingFolder = true;
    try {
      await openRecordingDir(id);
    } catch (error) {
      toastError(error);
    } finally {
      openingFolder = false;
    }
  }

  /**
   * Traer audio que ya existe.
   *
   * No se puede importar mientras se graba: Rust tiene un solo pipeline de
   * captura y meterle archivos en el medio le cambiaría la lista debajo.
   */
  async function importAudio() {
    if (capture.active) return;
    importing = true;
    try {
      const paths = await pickAudioFiles();
      if (paths.length === 0) return;
      const imported = await recordings.importFiles(paths);
      if (imported[0]) recordings.select(imported[0].id);
      toasts.push(
        imported.length === 1
          ? t("toast.importedOne", { title: imported[0].title })
          : t("toast.importedMany", { count: imported.length }),
      );
    } catch (error) {
      toastError(error);
    } finally {
      importing = false;
    }
  }

  async function confirmDelete() {
    const target = toDelete;
    if (!target) return;
    deleting = true;
    try {
      await recordings.remove(target.id);
      toasts.push(t("toast.deleted", { title: target.title }));
      toDelete = null;
    } catch (error) {
      toastError(error);
    } finally {
      deleting = false;
    }
  }

  // --- Las acciones del detalle que no merecen un botón propio ---

  type DetailPick = "folder" | "delete";

  const detailItems = $derived.by((): MenuItem<DetailPick>[] => [
    {
      id: "folder",
      label: t("page.meetings.openFolder"),
      icon: Folder,
      disabled: openingFolder,
    },
    { id: "delete", label: t("page.common.delete"), icon: Trash2, danger: true },
  ]);

  function onDetailPick(id: DetailPick) {
    const item = recordings.selected;
    if (!item) return;
    if (id === "folder") void openThisRecording(item.id);
    else toDelete = item;
  }

  // --- Renombrar ---

  /**
   * Los títulos los pone Atic sola a partir de la hora, así que la única forma
   * de volver a encontrar una reunión meses después es poder llamarla por su
   * nombre. `renameRecording` existía en Rust desde el principio; lo que
   * faltaba era desde dónde llamarlo.
   */
  async function saveTitle(item: Recording) {
    const title = (draftTitle ?? "").trim();
    if (!title || title === item.title) {
      draftTitle = null;
      return;
    }
    renaming = true;
    try {
      await recordings.rename(item.id, title);
      toasts.push(t("toast.renamed", { title }));
      draftTitle = null;
    } catch (error) {
      toastError(error);
    } finally {
      renaming = false;
    }
  }

  function onTitleKeydown(event: KeyboardEvent, item: Recording) {
    if (event.key === "Enter") {
      event.preventDefault();
      void saveTitle(item);
    } else if (event.key === "Escape") {
      // El workspace mira `defaultPrevented`: sin esto, cancelar el nombre
      // cerraría además la herramienta entera.
      event.preventDefault();
      draftTitle = null;
    }
  }

  // --- Recorrer la lista ---

  function selectAt(index: number) {
    const item = visible[index];
    if (!item) return;
    recordings.select(item.id);
    draftTitle = null;
    const row = listEl?.querySelector<HTMLElement>(`[data-row="${index}"]`);
    row?.focus();
    row?.scrollIntoView({ block: "nearest" });
  }

  function onListKeydown(event: KeyboardEvent) {
    const moved = nextIndex(event.key, selectedIndex, visible.length);
    if (moved !== null) {
      event.preventDefault();
      selectAt(moved);
      return;
    }
    if (event.key === "Delete" && recordings.selected) {
      event.preventDefault();
      toDelete = recordings.selected;
    }
  }

  /**
   * Espacio reproduce o pausa, como en cualquier app de audio del sistema.
   * Se ignora si hay un diálogo abierto, si se está escribiendo o si el foco
   * está en un control (ahí el espacio hace lo que ese control diga).
   */
  function onKeydown(event: KeyboardEvent) {
    if (event.key !== " ") return;
    if (draftTitle !== null || transcriptFor || summaryFor || toDelete) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("input, textarea, select, button, a, [contenteditable]"))
      return;
    const item = recordings.selected;
    if (!item) return;
    event.preventDefault();
    if (playback.label) void playback.toggle();
    else void playback.play(item, defaultTrack(item));
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet filterTrigger()}
  <Icon icon={SlidersHorizontal} size={14} />
{/snippet}

{#snippet moreTrigger()}
  <Icon icon={Ellipsis} size={14} />
{/snippet}

<ToolPage
  title={t("tools.meetings.label")}
  icon="meetings"
  blurb={t("tools.meetings.blurb")}
  kicker={t("tools.meetings.short")}
>
  <div class="flex h-full min-h-0 flex-col">
    <!--
      La barra es la grabadora: el botón que graba, el que importa y —solo
      mientras entra audio— el cronómetro con los dos medidores. El estado de
      grabación vive acá y no en un chip aparte.
    -->
    <Toolbar label={t("page.meetings.actions")}>
      <Button
        variant={capture.active ? "danger-solid" : "primary"}
        size="md"
        loading={capture.busy}
        onclick={toggle}
      >
        {capture.active ? t("tools.meetings.stop") : t("tools.meetings.record")}
      </Button>

      <Button
        variant="soft"
        size="md"
        loading={importing}
        disabled={capture.active}
        onclick={() => void importAudio()}
      >
        {t("page.meetings.import")}
      </Button>

      {#snippet end()}
        {#if capture.meeting?.active}
          <Chip tone="info">
            {t("page.meetings.meetingDetected")}{capture.meeting.provider
              ? ` · ${capture.meeting.provider}`
              : ""}
          </Chip>
        {/if}
        {#if capture.active}
          <span class="flex items-center gap-2.5">
            <span class="flex items-center gap-1.5">
              <span class="rec-dot" aria-hidden="true"></span>
              <span class="font-mono text-xs text-rec" data-numeric>
                {formatDuration(capture.elapsed)}
              </span>
            </span>
            <!-- Los niveles solo tienen sentido mientras entra audio. -->
            <div class="flex w-32 flex-col gap-0.5">
              <Meter
                value={capture.levels.mic}
                tone="mic"
                label={t("page.meetings.me")}
              />
              <Meter
                value={capture.levels.system}
                tone="sys"
                label={t("page.meetings.others")}
              />
            </div>
          </span>
        {/if}
      {/snippet}
    </Toolbar>

    {#if models.missing.length > 0}
      <div class="px-3 pt-3">
        <Banner
          tone="warn"
          title={models.missing.length === 1
            ? t("page.meetings.missingOne", {
                name: whisperModelLabel(models.missing[0].id),
              })
            : t("page.meetings.missingMany", { count: models.missing.length })}
        >
          {#snippet action()}
            <Button
              variant="soft"
              size="sm"
              loading={models.downloading !== null}
              onclick={() => void models.download(models.missing[0].id)}
            >
              {t("page.common.download")}
            </Button>
          {/snippet}
          {t("page.meetings.missingBody")}
        </Banner>
      </div>
    {/if}

    {#if models.downloading}
      <div class="px-3 pt-3">
        <ProgressBar
          value={models.downloading.downloaded / Math.max(models.downloading.total, 1)}
          label={t("page.meetings.downloading")}
        />
      </div>
    {/if}

    {#if capture.note}
      <div class="px-3 pt-3">
        <Banner tone="warn" title={capture.note} />
      </div>
    {/if}

    <LiveTranscript />

    <div class="min-h-0 flex-1">
      <ListDetail
        hasSelection={recordings.selected !== null}
        listLabel={t("page.meetings.list")}
        listCount={visible.length}
      >
        {#snippet listHeader()}
          <div class="flex items-center gap-1.5">
            <div class="min-w-0 flex-1">
              <Input
                type="search"
                bind:value={query}
                placeholder={t("page.meetings.searchPlaceholder")}
                aria-label={t("page.meetings.searchAria")}
              />
            </div>
            <!-- Filtrar es del listado, no de la herramienta: vive acá. Abre
                 alineado a la derecha para no salir de la columna. -->
            <Menu
              items={filterItems}
              label={t("page.meetings.statusFilter")}
              triggerLabel={t("page.meetings.statusFilter")}
              align="end"
              onpick={(value) => (statusFilter = value)}
              pressed={statusFilter !== "all"}
              triggerClass="grid size-8 shrink-0 place-items-center rounded-sm text-muted
                            transition-colors duration-(--duration-quick) ease-calm
                            hover:bg-surface-2 hover:text-text
                            aria-pressed:bg-surface-2 aria-pressed:text-text
                            focus-visible:[outline:2px_solid_var(--accent)]"
              trigger={filterTrigger}
            />
          </div>
        {/snippet}

        {#snippet list()}
          {#if visible.length === 0}
            <EmptyState
              compact
              icon={query || statusFilter !== "all" ? undefined : "meetings"}
              title={query || statusFilter !== "all"
                ? t("page.common.nothing")
                : t("page.meetings.empty")}
              hint={query || statusFilter !== "all"
                ? t("page.common.fewerWords")
                : t("page.meetings.emptyHint")}
            />
          {:else}
            <div bind:this={listEl}>
              {#each groups as group (group.key)}
                <p class="day">{group.label}</p>
                <ul class="flex flex-col">
                  {#each group.items as item (item.id)}
                    {@const index = flatIndex.get(item.id) ?? 0}
                    <li>
                      <button
                        type="button"
                        data-row={index}
                        aria-current={recordings.selectedId === item.id
                          ? "true"
                          : undefined}
                        onkeydown={onListKeydown}
                        onclick={() => {
                          recordings.select(item.id);
                          draftTitle = null;
                        }}
                      >
                        <span class="flex w-full items-center gap-1.5">
                          <span
                            class="dot"
                            data-tone={TONE[item.status]}
                            title={t(`page.meetings.status.${item.status}`)}
                          ></span>
                          <span class="min-w-0 flex-1 truncate text-sm text-text">
                            {item.title}
                          </span>
                        </span>
                        <span class="font-mono text-xs text-faint" data-numeric>
                          {formatDuration(item.duration_secs)}
                        </span>
                        {#if recordings.progress[item.id] !== undefined}
                          <span class="mt-1 block w-full">
                            <ProgressBar value={recordings.progress[item.id]} />
                          </span>
                        {/if}
                      </button>
                    </li>
                  {/each}
                </ul>
              {/each}
            </div>
          {/if}
        {/snippet}

        {#snippet detail()}
          {@const item = recordings.selected}
          {#if item}
            <div class="flex flex-col gap-3">
              {#if draftTitle !== null}
                <div class="flex items-center gap-1.5">
                  <Input
                    bind:value={draftTitle}
                    aria-label={t("page.meetings.renameAria")}
                    onkeydown={(event: KeyboardEvent) => onTitleKeydown(event, item)}
                  />
                  <Button
                    variant="primary"
                    size="sm"
                    loading={renaming}
                    onclick={() => void saveTitle(item)}
                  >
                    {t("page.common.save")}
                  </Button>
                  <Button variant="ghost" size="sm" onclick={() => (draftTitle = null)}>
                    {t("chrome.cancel")}
                  </Button>
                </div>
              {:else}
                <!-- El estado va al lado del título: describe a la grabación,
                     no al panel. Lo secundario, al menú. -->
                <div class="flex items-start gap-2">
                  <div class="flex min-w-0 flex-1 flex-col gap-1">
                    <div class="flex min-w-0 items-center gap-2">
                      <h3 class="min-w-0 truncate text-md font-semibold text-text">
                        {item.title}
                      </h3>
                      <Chip tone={TONE[item.status]}>
                        {t(`page.meetings.status.${item.status}`)}
                      </Chip>
                    </div>
                    <p class="font-mono text-xs text-faint" data-numeric>
                      {formatDuration(item.duration_secs)} · {formatDate(
                        item.started_at,
                      )}
                    </p>
                  </div>
                  <div class="flex shrink-0 items-center gap-0.5">
                    <IconButton
                      label={t("page.common.rename")}
                      size="sm"
                      onclick={() => (draftTitle = item.title)}
                    >
                      <Icon icon={Pencil} size={13} />
                    </IconButton>
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
                </div>
              {/if}

              {#if item.mic_path || item.system_path}
                <RecordingPlayer recording={item} />
              {/if}

              {#if recordings.progress[item.id] !== undefined}
                <ProgressBar
                  value={recordings.progress[item.id]}
                  label={t("page.meetings.transcribing", {
                    label: models.meetingProgressLabel,
                  })}
                  tone="ok"
                />
              {/if}

              {#if hasTranscript}
                <section class="flex flex-col gap-2">
                  <div class="flex flex-wrap items-center gap-1.5">
                    <Button
                      variant="soft"
                      size="sm"
                      onclick={() => (transcriptFor = item)}
                    >
                      {t("page.meetings.edit")}
                    </Button>
                    <Button
                      variant="soft"
                      size="sm"
                      onclick={() => (summaryFor = item)}
                    >
                      {summaries.byId[item.id]
                        ? t("page.meetings.viewSummary")
                        : t("page.meetings.summarize")}
                    </Button>
                  </div>
                  <TranscriptView recordingId={item.id} />
                </section>
              {:else}
                <div class="border-t border-line pt-3">
                  <EmptyState
                    title={t("page.meetings.noTranscript")}
                    hint={t("page.meetings.engineHint", { engine })}
                  >
                    {#snippet action()}
                      <div class="flex flex-wrap items-center justify-center gap-1.5">
                        {#if !models.meetingCanTranscribe}
                          <Button
                            variant="soft"
                            size="sm"
                            loading={models.downloading !== null}
                            onclick={() => {
                              const id = models.meetingModel?.id;
                              if (id) void models.download(id).catch(toastError);
                            }}
                          >
                            {t("page.meetings.downloadModel")}
                          </Button>
                        {:else}
                          <Button
                            variant="soft"
                            size="sm"
                            disabled={recordings.progress[item.id] !== undefined}
                            onclick={() => void transcribe(item.id)}
                          >
                            {t("page.meetings.transcribeAudio")}
                          </Button>
                        {/if}
                        <Button
                          variant="ghost"
                          size="sm"
                          onclick={() => onOpenSettings?.("meetings")}
                        >
                          {t("page.meetings.changeEngine")}
                        </Button>
                      </div>
                    {/snippet}
                  </EmptyState>
                </div>
              {/if}
            </div>
          {/if}
        {/snippet}

        {#snippet empty()}
          <EmptyState
            compact
            icon="meetings"
            title={t("page.common.pickOne")}
            hint={t("page.common.pickOneHint")}
          />
        {/snippet}
      </ListDetail>
    </div>
  </div>
</ToolPage>

{#if transcriptFor}
  <!-- Se fija en una constante para que el cierre de `onRetranscribe` no
       dependa de una variable que puede volverse nula. -->
  {@const item = transcriptFor}
  <TranscriptPanel
    recording={item}
    canTranscribe={models.meetingCanTranscribe}
    onRetranscribe={() => transcribe(item.id)}
    onClose={() => (transcriptFor = null)}
  />
{/if}

{#if summaryFor}
  <SummaryPanel
    recording={summaryFor}
    onOpenSettings={() => onOpenSettings?.("summary")}
    onClose={() => (summaryFor = null)}
  />
{/if}

{#if toDelete}
  <ConfirmDialog
    title={t("page.meetings.deleteTitle", { title: toDelete.title })}
    body={t("page.meetings.deleteBody")}
    confirmLabel={t("page.common.delete")}
    tone="danger"
    busy={deleting}
    onConfirm={() => void confirmDelete()}
    onCancel={() => (toDelete = null)}
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

  /*
   * Un punto y no un chip por fila: en una lista de cien, seis etiquetas de
   * colores compiten con los títulos, que es lo que se está leyendo.
   */
  .dot {
    display: block;
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 999px;
    background: var(--muted);
  }

  .dot[data-tone="ok"] {
    background: var(--ok);
  }

  .dot[data-tone="info"] {
    background: var(--info);
  }

  .dot[data-tone="danger"] {
    background: var(--danger);
  }

  /* El único punto rojo de la pantalla: dice que hay audio entrando. */
  .rec-dot {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: var(--rec);
  }
</style>
