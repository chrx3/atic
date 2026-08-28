<script lang="ts">
  /**
   * Reuniones: grabar, ver lo grabado, transcribir.
   *
   * Es la primera pantalla que conecta los stores de dominio con las
   * primitivas, y por eso no tiene ni un `onMount` con suscripciones ni una
   * variable de estado propia que venga de Rust: todo eso vive en `domain/` y
   * acá solo se lee. Lo único local es lo que no sale de la ventana — qué
   * confirmación está abierta, qué se está filtrando, qué se está renombrando.
   */
  import { fuzzyMatch } from "$core/clipboardSearch";
  import { groupByDay } from "$core/dayGroups";
  import { formatDate, formatDuration } from "$core/format";
  import { nextIndex } from "$core/listNav";
  import type { Recording, RecordingStatus } from "$core/types";
  import { capture } from "$domain/capture.svelte";
  import { models } from "$domain/models.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t, whisperModelLabel } from "$domain/i18n.svelte";
  import { pickAudioFiles } from "$ipc/dialogs";
  import { openRecordingDir } from "$ipc/recordings";
  import { Pencil } from "$lib/icons";
  import ListDetail from "$patterns/ListDetail.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Button from "$ui/Button.svelte";
  import Banner from "$ui/Banner.svelte";
  import Chip from "$ui/Chip.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import Meter from "$ui/Meter.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import Select from "$ui/Select.svelte";
  import LiveTranscript from "./LiveTranscript.svelte";
  import RecordingPlayer from "./RecordingPlayer.svelte";
  import SummaryPanel from "./SummaryPanel.svelte";
  import TranscribeModelSelect from "./TranscribeModelSelect.svelte";
  import TranscriptPanel from "./TranscriptPanel.svelte";
  import TranscriptView from "./TranscriptView.svelte";
  import { summaries } from "$domain/summaries.svelte";

  let { onOpenSettings }: { onOpenSettings?: () => void } = $props();

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
</script>

<ToolPage
  title={t("tools.meetings.label")}
  icon="meetings"
  blurb={t("tools.meetings.blurb")}
  kicker={t("tools.meetings.short")}
>
  {#snippet meta()}
    {#if capture.active}
      <Chip tone="rec">
        {t("page.meetings.recordingChip", { elapsed: formatDuration(capture.elapsed) })}
      </Chip>
    {:else}
      <Chip>{t("page.meetings.count", { count: recordings.items.length })}</Chip>
    {/if}
    {#if capture.meeting?.active}
      <Chip tone="info">
        {t("page.meetings.meetingDetected")}{capture.meeting.provider
          ? ` · ${capture.meeting.provider}`
          : ""}
      </Chip>
    {/if}
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label={t("page.meetings.actions")}>
      <Button
        variant={capture.active ? "danger-solid" : "primary"}
        size="sm"
        loading={capture.busy}
        onclick={toggle}
      >
        {capture.active ? t("tools.meetings.stop") : t("tools.meetings.record")}
      </Button>

      <Button
        variant="soft"
        size="sm"
        loading={importing}
        disabled={capture.active}
        onclick={() => void importAudio()}
      >
        {t("page.meetings.import")}
      </Button>

      <div class="w-40">
        <Select
          bind:value={statusFilter}
          options={statusOptions}
          aria-label={t("page.meetings.statusFilter")}
        />
      </div>

      {#snippet end()}
        {#if capture.active}
          <!-- Los niveles solo tienen sentido mientras entra audio. -->
          <div class="flex w-40 flex-col gap-0.5">
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
        {/if}
      {/snippet}
    </Toolbar>

    {#if models.missing.length > 0}
      <div class="px-4 pt-3">
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
      <div class="px-4 pt-3">
        <ProgressBar
          value={models.downloading.downloaded / Math.max(models.downloading.total, 1)}
          label={t("page.meetings.downloading")}
        />
      </div>
    {/if}

    {#if capture.note}
      <div class="px-4 pt-3">
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
          <Input
            type="search"
            bind:value={query}
            placeholder={t("page.meetings.searchPlaceholder")}
            aria-label={t("page.meetings.searchAria")}
          />
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
            <!-- Sin padding propio: el panel de ListDetail ya lo trae. -->
            <div class="flex flex-col gap-3">
              <div class="flex items-start gap-2">
                <div class="flex min-w-0 flex-1 flex-col gap-1">
                  {#if draftTitle !== null}
                    <div class="flex items-center gap-1.5">
                      <Input
                        bind:value={draftTitle}
                        aria-label={t("page.meetings.renameAria")}
                        onkeydown={(event: KeyboardEvent) =>
                          onTitleKeydown(event, item)}
                      />
                      <Button
                        variant="primary"
                        size="sm"
                        loading={renaming}
                        onclick={() => void saveTitle(item)}
                      >
                        {t("page.common.save")}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onclick={() => (draftTitle = null)}
                      >
                        {t("chrome.cancel")}
                      </Button>
                    </div>
                  {:else}
                    <div class="flex min-w-0 items-center gap-1">
                      <h3 class="min-w-0 truncate text-md font-semibold text-text">
                        {item.title}
                      </h3>
                      <IconButton
                        label={t("page.common.rename")}
                        size="sm"
                        onclick={() => (draftTitle = item.title)}
                      >
                        <Icon icon={Pencil} size={12} />
                      </IconButton>
                    </div>
                  {/if}
                  <p class="font-mono text-xs text-faint" data-numeric>
                    {formatDuration(item.duration_secs)} · {formatDate(item.started_at)}
                  </p>
                </div>
                <Chip tone={TONE[item.status]}
                  >{t(`page.meetings.status.${item.status}`)}</Chip
                >
              </div>

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

              <!--
                Dos filas y no una de siete botones iguales. Arriba lo que se
                hace con la reunión —resumirla, corregirla, sacarla de en
                medio—; abajo el motor de transcripción, que es una decisión
                aparte y arrastra su propio selector.
              -->
              <div class="flex flex-wrap items-center gap-1.5">
                <Button variant="primary" size="sm" onclick={() => (summaryFor = item)}>
                  {summaries.byId[item.id]
                    ? t("page.meetings.summary")
                    : t("page.meetings.summarize")}
                </Button>
                <Button variant="soft" size="sm" onclick={() => (transcriptFor = item)}>
                  {t("page.meetings.viewFix")}
                </Button>
                <div class="ml-auto flex items-center gap-1.5">
                  <Button
                    variant="ghost"
                    size="sm"
                    loading={openingFolder}
                    onclick={() => void openThisRecording(item.id)}
                  >
                    {t("page.meetings.folder")}
                  </Button>
                  <Button variant="danger" size="sm" onclick={() => (toDelete = item)}>
                    {t("page.common.delete")}
                  </Button>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-micro text-muted uppercase">
                  {t("page.meetings.transcriptionRow")}
                </span>
                <div class="min-w-52 max-w-72 flex-1">
                  <TranscribeModelSelect
                    disabled={recordings.progress[item.id] !== undefined}
                  />
                </div>
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
                    {t("page.common.download")}
                  </Button>
                {:else}
                  <Button
                    variant="soft"
                    size="sm"
                    disabled={recordings.progress[item.id] !== undefined}
                    onclick={() => void transcribe(item.id)}
                  >
                    {item.status === "recorded" || item.status === "error"
                      ? t("page.meetings.transcribe")
                      : t("page.meetings.retranscribe")}
                  </Button>
                {/if}
              </div>

              <div class="border-t border-line pt-3">
                <TranscriptView recordingId={item.id} />
              </div>
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
    {onOpenSettings}
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
</style>
