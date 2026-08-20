<script lang="ts">
  /**
   * La transcripción completa: buscarla, corregirla, escucharla y exportarla.
   *
   * Fragmento a fragmento y no agrupada por hablante como la vista de al lado,
   * porque acá cada fila es accionable: se hace clic para escuchar ese momento
   * exacto, y en modo edición cada una se corrige por separado.
   *
   * La edición trabaja sobre una copia. Whisper se equivoca seguido con quién
   * habla, así que corregir es lo normal —pero descartar a mitad de camino
   * también—, y sin copia cada tecla ya habría tocado el store.
   */
  import type { Recording, Segment, Speaker, Transcript } from "$core/types";
  import { isJunkTranscriptText } from "$core/transcriptText";
  import { playback } from "$domain/playback.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import { pickExportPath, safeFileName } from "$ipc/dialogs";
  import { exportRecording } from "$ipc/recordings";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import Modal from "$ui/Modal.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import TextArea from "$ui/TextArea.svelte";
  import { Trash2 } from "$lib/icons";
  import RecordingPlayer from "./RecordingPlayer.svelte";
  import TranscribeModelSelect from "./TranscribeModelSelect.svelte";
  import { models } from "$domain/models.svelte";

  let {
    recording,
    onClose,
    onRetranscribe,
    canTranscribe = true,
  }: {
    recording: Recording;
    onClose: () => void;
    onRetranscribe?: () => Promise<void>;
    /** `false` cuando falta descargar el modelo local. */
    canTranscribe?: boolean;
  } = $props();

  type Format = "docx" | "pdf" | "md";

  let query = $state("");
  let speakerFilter = $state<"all" | Speaker>("all");
  let draft = $state<Transcript | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let format = $state<Format>("docx");
  let exporting = $state(false);
  let exportStatus = $state<string | null>(null);
  let retranscribing = $state(false);

  $effect(() => {
    void recordings.loadTranscript(recording.id).catch(toastError);
  });

  const stored = $derived(recordings.transcripts[recording.id]);
  const loading = $derived(!(recording.id in recordings.transcripts));
  const editing = $derived(draft !== null);

  /**
   * Silencio y bucles de estática no se muestran.
   *
   * Whisper emite `[silence]` en los huecos, y sobre ruido plano inventa
   * la misma frase una y otra vez. En una reunión larga ensucian la lectura.
   */
  const segments = $derived(
    (draft?.segments ?? stored?.segments ?? []).filter(
      (segment) => !isJunkTranscriptText(segment.text),
    ),
  );

  const visible = $derived.by(() => {
    // En edición no se filtra: esconder una fila mientras se corrige es la
    // forma más simple de perder un cambio sin darse cuenta.
    if (editing) return segments;
    const needle = query.trim().toLocaleLowerCase();
    return segments.filter(
      (segment) =>
        (speakerFilter === "all" || segment.speaker === speakerFilter) &&
        (!needle || segment.text.toLocaleLowerCase().includes(needle)),
    );
  });

  /** Qué fragmento suena ahora, para resaltarlo mientras se escucha. */
  const playingStart = $derived.by(() => {
    if (playback.recordingId !== recording.id) return null;
    const ms = playback.currentTime * 1000;
    return segments.find((s) => ms >= s.start_ms && ms < s.end_ms)?.start_ms ?? null;
  });

  function stamp(ms: number): string {
    const total = Math.max(0, Math.floor(ms / 1000));
    return `${Math.floor(total / 60)}:${(total % 60).toString().padStart(2, "0")}`;
  }

  function speakerLabel(segment: Segment): string {
    return (
      segment.speaker_name?.trim() ||
      (segment.speaker === "me" ? t("page.meetings.me") : t("page.meetings.others"))
    );
  }

  async function copyVisible() {
    if (visible.length === 0) return;
    const text = visible
      .map((segment) => `${speakerLabel(segment)}: ${segment.text.trim()}`)
      .join("\n");
    await navigator.clipboard.writeText(text);
    toasts.push(t("toast.copiedFragments", { count: visible.length }));
  }

  function beginEditing() {
    if (!stored) return;
    // `$state.snapshot` primero: lo del store es un proxy y `structuredClone`
    // no sabe clonarlo.
    draft = structuredClone($state.snapshot(stored)) as Transcript;
    query = "";
    speakerFilter = "all";
    saveError = null;
  }

  async function saveEdits() {
    if (!draft || saving) return;
    saving = true;
    saveError = null;
    try {
      // «Yo» nunca lleva nombre: el nombre existe para distinguir a los demás
      // entre sí. Se normaliza acá y no al cambiar el selector para que el
      // invariante esté en un solo lugar.
      for (const segment of draft.segments) {
        if (segment.speaker === "me") segment.speaker_name = null;
      }
      await recordings.saveEditedTranscript(recording.id, draft);
      draft = null;
      toasts.push(t("toast.transcriptSaved"));
    } catch (error) {
      saveError = t("page.meetings.saveFail", { error: String(error) });
    } finally {
      saving = false;
    }
  }

  function removeSegment(target: Segment) {
    if (!draft) return;
    draft.segments = draft.segments.filter((segment) => segment !== target);
  }

  async function exportFile() {
    exporting = true;
    exportStatus = null;
    try {
      const destination = await pickExportPath(
        safeFileName(recording.title, t("page.meetings.exportSlug")),
        format,
      );
      if (!destination) return;
      await exportRecording(recording.id, format, destination);
      exportStatus = t("page.meetings.exportOk", { path: destination });
    } catch (error) {
      exportStatus = t("page.meetings.exportFail", { error: String(error) });
    } finally {
      exporting = false;
    }
  }

  async function retranscribe() {
    if (!onRetranscribe) return;
    retranscribing = true;
    try {
      await onRetranscribe();
    } finally {
      retranscribing = false;
    }
  }

  const speakerFilterOptions = $derived([
    { value: "all" as const, label: t("page.meetings.all") },
    { value: "me" as const, label: t("page.meetings.me") },
    { value: "others" as const, label: t("page.meetings.others") },
  ]);

  const speakerEditOptions = $derived([
    { value: "me", label: t("page.meetings.me") },
    { value: "others", label: t("page.meetings.participant") },
  ]);
</script>

<Modal
  title={t("page.meetings.transcriptTitle")}
  subtitle={recording.title}
  size="xl"
  {onClose}
  dismissible={!editing}
>
  {#if loading}
    <p class="py-12 text-center text-sm text-muted" role="status">
      {t("page.meetings.loadingTranscript")}
    </p>
  {:else if segments.length === 0}
    <EmptyState
      title={t("page.meetings.noTranscript")}
      hint={t("page.meetings.noTranscriptHint")}
    />
  {:else}
    <!-- Las herramientas quedan fijas: en una reunión larga la lista es de
         cientos de filas y el buscador tiene que seguir a mano. -->
    <div
      class="sticky -top-3 z-1 -mx-4 mb-2 flex flex-wrap items-center gap-2 border-b
             border-line bg-elevated px-4 py-2"
    >
      {#if editing}
        <p class="flex-1 text-xs text-muted">
          {t("page.meetings.editHint")}
        </p>
        <Button
          variant="ghost"
          size="sm"
          disabled={saving}
          onclick={() => (draft = null)}
        >
          {t("page.meetings.discard")}
        </Button>
        <Button
          variant="primary"
          size="sm"
          loading={saving}
          onclick={() => void saveEdits()}
        >
          {t("page.meetings.saveChanges")}
        </Button>
      {:else}
        <div class="min-w-48 flex-1">
          <Input
            type="search"
            bind:value={query}
            placeholder={t("page.meetings.searchTranscript")}
            aria-label={t("page.meetings.searchTranscriptAria")}
            autocomplete="off"
          />
        </div>

        <SegmentedControl
          bind:value={speakerFilter}
          options={speakerFilterOptions}
          size="sm"
          label={t("page.meetings.filterSpeaker")}
        />

        <Button
          variant="soft"
          size="sm"
          disabled={visible.length === 0}
          onclick={() => void copyVisible()}
        >
          {t("page.common.copy")}
        </Button>
        <Button variant="soft" size="sm" onclick={beginEditing}>{t("page.meetings.edit")}</Button>

        <span
          class="w-full font-mono text-micro text-faint"
          data-numeric
          aria-live="polite"
        >
          {t("page.meetings.fragmentsCount", {
            visible: visible.length,
            total: segments.length,
          })}
        </span>
      {/if}
    </div>

    {#if saveError}
      <div class="mb-3">
        <Banner tone="danger" title={saveError} />
      </div>
    {/if}

    {#if visible.length === 0}
      <EmptyState
        title={t("page.common.nothing")}
        hint={t("page.meetings.nothingFilterHint")}
      />
    {:else if editing}
      <ul class="flex flex-col gap-2">
        {#each visible as segment, index (`${segment.start_ms}-${index}`)}
          <li
            class="grid grid-cols-[4.5rem_7rem_minmax(0,1fr)_auto] items-start gap-2
                   border-b border-line pb-2 last:border-0"
          >
            <button
              type="button"
              class="h-8 rounded-xs font-mono text-xs text-muted
                     transition-colors duration-(--duration-quick) ease-calm
                     hover:bg-surface-2 hover:text-text"
              data-numeric
              onclick={() =>
                void playback.playSpeaker(
                  recording,
                  segment.speaker,
                  segment.start_ms / 1000,
                )}
              aria-label={t("page.meetings.listenFrom", {
                time: stamp(segment.start_ms),
              })}
            >
              ▶ {stamp(segment.start_ms)}
            </button>

            <Select
              value={segment.speaker}
              options={speakerEditOptions}
              aria-label={t("page.meetings.whoSpeaks")}
              onchange={(event: Event) => {
                segment.speaker = (event.currentTarget as HTMLSelectElement)
                  .value as Speaker;
                // «Yo» no lleva nombre: el nombre es para distinguir a los
                // demás entre sí.
                if (segment.speaker === "me") segment.speaker_name = null;
              }}
            />

            <div class="flex flex-col gap-1">
              <Input
                value={segment.speaker_name ?? ""}
                disabled={segment.speaker === "me"}
                maxlength={80}
                placeholder={segment.speaker === "me"
                  ? t("page.meetings.me")
                  : t("page.meetings.nameOptional")}
                aria-label={t("page.meetings.participantName")}
                oninput={(event: Event) => {
                  segment.speaker_name =
                    (event.currentTarget as HTMLInputElement).value || null;
                }}
              />
              <TextArea
                bind:value={segment.text}
                rows={2}
                maxlength={20000}
                aria-label={t("page.meetings.segmentText", {
                  time: stamp(segment.start_ms),
                })}
              />
            </div>

            <IconButton
              label={t("page.meetings.deleteFragment", {
                time: stamp(segment.start_ms),
              })}
              size="sm"
              onclick={() => removeSegment(segment)}
            >
              <Icon icon={Trash2} size={14} />
            </IconButton>
          </li>
        {/each}
      </ul>
    {:else}
      <ol class="flex flex-col">
        {#each visible as segment, index (`${segment.start_ms}-${index}`)}
          <li>
            <button
              type="button"
              class="grid w-full grid-cols-[3.5rem_2.75rem_minmax(0,1fr)] gap-3 rounded-xs
                     px-2 py-2 text-left
                     transition-colors duration-(--duration-quick) ease-calm
                     hover:bg-surface-2
                     {playingStart === segment.start_ms ? 'bg-surface-2' : ''}"
              aria-current={playingStart === segment.start_ms ? "true" : undefined}
              onclick={() =>
                void playback.playSpeaker(
                  recording,
                  segment.speaker,
                  segment.start_ms / 1000,
                )}
            >
              <span
                class="truncate pt-0.5 text-xs font-semibold
                       {segment.speaker === 'me' ? 'text-mic' : 'text-sys'}"
              >
                {speakerLabel(segment)}
              </span>
              <span class="pt-0.5 font-mono text-xs text-faint" data-numeric>
                {stamp(segment.start_ms)}
              </span>
              <span class="text-sm leading-relaxed text-text">{segment.text}</span>
            </button>
          </li>
        {/each}
      </ol>
    {/if}
  {/if}

  {#snippet actions()}
    <div class="flex w-full flex-col gap-3">
      <RecordingPlayer {recording} />

      <div class="flex flex-wrap items-center gap-2">
        <div class="w-40">
          <Select
            bind:value={format}
            options={[
              { value: "docx" as const, label: "Word (.docx)" },
              { value: "pdf" as const, label: "PDF (.pdf)" },
              { value: "md" as const, label: "Markdown (.md)" },
            ]}
            disabled={exporting || editing}
            aria-label={t("page.meetings.exportFormat")}
          />
        </div>
        <Button
          variant="soft"
          loading={exporting}
          disabled={editing}
          onclick={() => void exportFile()}
        >
          {t("page.meetings.export")}
        </Button>

        {#if onRetranscribe}
          <div class="min-w-52 max-w-72 flex-1">
            <TranscribeModelSelect disabled={retranscribing || editing} />
          </div>
          {#if !models.meetingCanTranscribe}
            <Button
              variant="soft"
              loading={models.downloading !== null}
              disabled={editing}
              onclick={() => {
                const id = models.meetingModel?.id;
                if (id) void models.download(id).catch(toastError);
              }}
            >
              {t("page.meetings.downloadModel")}
            </Button>
          {:else}
            <Button
              variant="ghost"
              loading={retranscribing}
              disabled={!canTranscribe || editing}
              onclick={() => void retranscribe()}
            >
              {t("page.meetings.retranscribe")}
            </Button>
          {/if}
        {/if}
      </div>

      {#if exportStatus}
        <p class="max-w-[70ch] text-xs break-words text-muted" role="status">
          {exportStatus}
        </p>
      {/if}
    </div>
  {/snippet}
</Modal>
