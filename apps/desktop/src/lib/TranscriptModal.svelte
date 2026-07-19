<script lang="ts">
  import { onMount } from "svelte";
  import type { Recording, Speaker, Transcript } from "$lib/types";
  import { exportRecording, getTranscript, saveTranscript } from "$lib/api";
  import { playback } from "$lib/playback.svelte";
  import AudioPlayer from "$lib/AudioPlayer.svelte";
  import ModalShell from "$lib/ModalShell.svelte";

  let {
    recording,
    onClose,
    modelReady = true,
    onRetranscribe,
  }: {
    recording: Recording;
    onClose: () => void;
    modelReady?: boolean;
    onRetranscribe?: () => void | Promise<void>;
  } = $props();

  let transcript = $state<Transcript | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");
  let speakerFilter = $state<"all" | Speaker>("all");
  let copied = $state(false);
  let retranscribing = $state(false);
  let editing = $state(false);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let editSnapshot = $state<Transcript | null>(null);
  let exportFormat = $state<"md" | "docx" | "pdf">("docx");
  let exporting = $state(false);
  let exportStatus = $state<string | null>(null);

  function isSilenceMarker(text: string): boolean {
    const compact = text.trim().replace(/\s+/g, "").toLowerCase();
    return (
      !compact ||
      compact === "[silence]" ||
      compact === "(silence)" ||
      compact === "silence" ||
      compact.startsWith("[silence") ||
      compact.startsWith("(silence")
    );
  }

  function formatTimestamp(milliseconds: number): string {
    const seconds = Math.floor(milliseconds / 1000);
    const minutes = Math.floor(seconds / 60);
    return `${minutes}:${(seconds % 60).toString().padStart(2, "0")}`;
  }

  const baseSegments = $derived(
    (transcript?.segments ?? []).filter((segment) => !isSilenceMarker(segment.text)),
  );
  const visibleSegments = $derived(
    baseSegments.filter((segment) => {
      const matchesSpeaker =
        speakerFilter === "all" || segment.speaker === speakerFilter;
      const matchesQuery =
        !query.trim() ||
        segment.text.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
      return matchesSpeaker && matchesQuery;
    }),
  );
  const activeStart = $derived.by(() => {
    if (playback.recordingId !== recording.id) return null;
    const milliseconds = playback.currentTime * 1000;
    return (
      baseSegments.find(
        (segment) =>
          milliseconds >= segment.start_ms && milliseconds < segment.end_ms,
      )?.start_ms ?? null
    );
  });

  async function playSegment(startMs: number, speaker: Speaker) {
    await playback.playSpeaker(recording, speaker, startMs / 1000);
  }

  async function copyVisible() {
    if (visibleSegments.length === 0) return;
    const text = visibleSegments
      .map(
        (segment) =>
          `${speakerLabel(segment)}: ${segment.text.trim()}`,
      )
      .join("\n");
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function speakerLabel(segment: Transcript["segments"][number]): string {
    return segment.speaker_name?.trim() ||
      (segment.speaker === "me" ? "Yo" : "Otros");
  }

  function beginEditing() {
    if (!transcript) return;
    editSnapshot = structuredClone(transcript);
    query = "";
    speakerFilter = "all";
    saveError = null;
    editing = true;
  }

  function cancelEditing() {
    if (editSnapshot) transcript = structuredClone(editSnapshot);
    editSnapshot = null;
    saveError = null;
    editing = false;
  }

  function removeSegment(segment: Transcript["segments"][number]) {
    if (!transcript) return;
    transcript.segments = transcript.segments.filter((item) => item !== segment);
  }

  async function saveEdits() {
    if (!transcript || saving) return;
    saving = true;
    saveError = null;
    try {
      await saveTranscript(recording.id, transcript);
      transcript = await getTranscript(recording.id);
      editSnapshot = null;
      editing = false;
    } catch (saveFailure) {
      saveError = `No se pudieron guardar los cambios: ${String(saveFailure)}`;
    } finally {
      saving = false;
    }
  }

  async function exportFile() {
    if (exporting) return;
    exporting = true;
    exportStatus = null;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const extension = exportFormat;
      const safeTitle = recording.title
        .replace(/[<>:"/\\|?*]+/g, "-")
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 100) || "transcripcion";
      const destination = await save({
        title: "Exportar reunión",
        defaultPath: `${safeTitle}.${extension}`,
        filters: [
          {
            name:
              exportFormat === "md"
                ? "Markdown"
                : exportFormat === "docx"
                  ? "Documento de Word"
                  : "PDF",
            extensions: [extension],
          },
        ],
      });
      if (!destination) return;
      await exportRecording(recording.id, exportFormat, destination);
      exportStatus = `Exportado en ${destination}`;
    } catch (exportFailure) {
      exportStatus = `No se pudo exportar: ${String(exportFailure)}`;
    } finally {
      exporting = false;
    }
  }

  async function retranscribe() {
    if (!onRetranscribe || retranscribing) return;
    retranscribing = true;
    try {
      await onRetranscribe();
    } finally {
      retranscribing = false;
    }
  }

  onMount(async () => {
    try {
      transcript = await getTranscript(recording.id);
    } catch (loadError) {
      error = `No se pudo abrir la transcripción: ${String(loadError)}`;
    } finally {
      loading = false;
    }
  });
</script>

<ModalShell
  title="Transcripción"
  subtitle={recording.title}
  size="xl"
  onClose={onClose}
>
  {#if loading}
    <div class="rb-transcript-state" role="status">Cargando transcripción…</div>
  {:else if error}
    <div class="rb-transcript-state is-error" role="alert">{error}</div>
  {:else if !transcript || baseSegments.length === 0}
    <div class="rb-transcript-state">No hay texto disponible.</div>
  {:else}
    <div class="rb-transcript-tools">
      <label class="rb-search">
        <span class="sr-only">Buscar en la transcripción</span>
        <input
          class="rb-field"
          type="search"
          name="transcript-search"
          bind:value={query}
          placeholder="Buscar en la transcripción…"
          autocomplete="off"
          disabled={editing}
        />
      </label>

      <div class="rb-filter" aria-label="Filtrar hablante">
        <button
          type="button"
          class:is-active={speakerFilter === "all"}
          onclick={() => (speakerFilter = "all")}
          aria-pressed={speakerFilter === "all"}
          disabled={editing}
        >
          Todos
        </button>
        <button
          type="button"
          class:is-active={speakerFilter === "me"}
          onclick={() => (speakerFilter = "me")}
          aria-pressed={speakerFilter === "me"}
          disabled={editing}
        >
          Yo
        </button>
        <button
          type="button"
          class:is-active={speakerFilter === "others"}
          onclick={() => (speakerFilter = "others")}
          aria-pressed={speakerFilter === "others"}
          disabled={editing}
        >
          Otros
        </button>
      </div>

      <button
        type="button"
        class="rb-btn rb-btn-soft"
        onclick={copyVisible}
        disabled={visibleSegments.length === 0}
      >
        {copied ? "Copiado" : "Copiar"}
      </button>
      <button
        type="button"
        class="rb-btn {editing ? 'rb-btn-primary' : 'rb-btn-soft'}"
        onclick={editing ? saveEdits : beginEditing}
        disabled={saving}
      >
        {saving ? "Guardando…" : editing ? "Guardar cambios" : "Editar"}
      </button>
      <span class="rb-result-count" aria-live="polite">
        {visibleSegments.length} fragmentos
      </span>
    </div>

    {#if visibleSegments.length === 0}
      <div class="rb-transcript-state">
        No hay resultados para esta búsqueda.
      </div>
    {:else}
      <ol class="rb-transcript-list rb-select-text">
        {#each visibleSegments as segment, index (`${segment.speaker}-${segment.start_ms}-${index}`)}
          <li>
            {#if editing}
              <div class="rb-transcript-editor-row">
                <button
                  type="button"
                  class="rb-editor-play"
                  onclick={() => playSegment(segment.start_ms, segment.speaker)}
                  aria-label={`Reproducir desde ${formatTimestamp(segment.start_ms)}`}
                >
                  ▶ {formatTimestamp(segment.start_ms)}
                </button>
                <select
                  class="rb-field rb-editor-speaker"
                  value={segment.speaker}
                  aria-label="Origen del fragmento"
                  onchange={(event) => {
                    segment.speaker = event.currentTarget.value as Speaker;
                    if (segment.speaker === "me") segment.speaker_name = null;
                  }}
                >
                  <option value="me">Yo</option>
                  <option value="others">Participante</option>
                </select>
                <input
                  class="rb-field rb-editor-name"
                  value={segment.speaker_name ?? ""}
                  disabled={segment.speaker === "me"}
                  maxlength="80"
                  placeholder={segment.speaker === "me" ? "Yo" : "Nombre opcional"}
                  aria-label="Nombre del participante"
                  oninput={(event) => {
                    segment.speaker_name = event.currentTarget.value || null;
                  }}
                />
                <textarea
                  class="rb-field rb-editor-text"
                  bind:value={segment.text}
                  maxlength="20000"
                  rows="2"
                  aria-label={`Texto en ${formatTimestamp(segment.start_ms)}`}
                ></textarea>
                <button
                  type="button"
                  class="rb-btn rb-btn-danger rb-editor-remove"
                  onclick={() => removeSegment(segment)}
                  aria-label={`Eliminar fragmento de ${formatTimestamp(segment.start_ms)}`}
                >
                  Eliminar
                </button>
              </div>
            {:else}
              <button
                type="button"
                class="rb-transcript-row"
                class:is-active={activeStart === segment.start_ms}
                onclick={() => playSegment(segment.start_ms, segment.speaker)}
                aria-current={activeStart === segment.start_ms ? "true" : undefined}
              >
                <span
                  class="rb-speaker"
                  class:is-me={segment.speaker === "me"}
                >
                  {speakerLabel(segment)}
                </span>
                <span class="rb-timestamp">{formatTimestamp(segment.start_ms)}</span>
                <span class="rb-segment-text">{segment.text}</span>
              </button>
            {/if}
          </li>
        {/each}
      </ol>
      {#if editing}
        <div class="rb-edit-note" role="status">
          <p>
            Guardar elimina fragmentos vacíos y marca cualquier resumen anterior
            como pendiente para evitar contenido desactualizado.
          </p>
          <button type="button" class="rb-btn rb-btn-ghost" onclick={cancelEditing} disabled={saving}>
            Descartar cambios
          </button>
        </div>
      {/if}
      {#if saveError}
        <div class="rb-banner rb-banner-warn mt-3" role="alert">{saveError}</div>
      {/if}
    {/if}
  {/if}

  {#snippet actions()}
    <div class="rb-transcript-actions">
      <div class="rb-export-actions">
        <label class="rb-label">
          Formato de exportación
          <select class="rb-field" bind:value={exportFormat} disabled={exporting || editing}>
            <option value="docx">Word (.docx)</option>
            <option value="pdf">PDF (.pdf)</option>
            <option value="md">Markdown (.md)</option>
          </select>
        </label>
        <button
          type="button"
          class="rb-btn rb-btn-soft"
          onclick={exportFile}
          disabled={exporting || editing}
        >
          {exporting ? "Exportando…" : "Exportar"}
        </button>
      </div>
      {#if exportStatus}
        <p class="rb-export-status" role="status">{exportStatus}</p>
      {/if}
      {#if onRetranscribe}
        <button
          class="rb-btn rb-btn-soft"
          onclick={retranscribe}
          disabled={!modelReady || retranscribing}
        >
          {retranscribing
            ? "Re-transcribiendo…"
            : "Re-transcribir (local, alta calidad)"}
        </button>
      {/if}
      <div class="w-full">
        <AudioPlayer
          alwaysVisible
          dismissible={false}
          placeholder="Selecciona un fragmento para escucharlo"
        />
      </div>
    </div>
  {/snippet}
</ModalShell>

<style>
  .rb-transcript-actions {
    display: flex;
    width: 100%;
    flex-direction: column;
    gap: 0.75rem;
  }
  .rb-transcript-tools {
    position: sticky;
    z-index: 2;
    top: -0.5rem;
    display: grid;
    grid-template-columns: minmax(12rem, 1fr) auto auto auto;
    align-items: center;
    gap: 0.5rem;
    padding-block: 0.5rem 0.75rem;
    background: var(--rb-surface);
  }
  .rb-result-count {
    grid-column: 1 / -1;
    color: var(--rb-faint);
    font-size: 0.625rem;
  }
  .rb-filter {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--rb-radius-sm);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }
  .rb-filter button {
    min-height: 1.75rem;
    padding: 0.3rem 0.6rem;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-muted);
    font-size: 0.6875rem;
  }
  .rb-filter button:hover {
    color: var(--rb-text);
  }
  .rb-filter button.is-active {
    color: var(--rb-text);
    background: var(--rb-surface-2);
  }
  .rb-filter button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-transcript-list {
    display: flex;
    flex-direction: column;
  }
  .rb-transcript-list li {
    content-visibility: auto;
    contain-intrinsic-size: auto 64px;
  }
  .rb-transcript-list li + li {
    border-top: 0;
  }
  .rb-transcript-row {
    display: grid;
    width: 100%;
    grid-template-columns: 3.5rem 2.75rem minmax(0, 1fr);
    gap: 0.75rem;
    padding: 0.75rem 0.5rem;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-text);
    text-align: left;
    transition: background 0.14s ease;
  }
  .rb-transcript-row:hover {
    background: color-mix(in srgb, var(--rb-text) 4%, transparent);
  }
  .rb-transcript-row.is-active {
    background: color-mix(in srgb, var(--rb-accent) 9%, transparent);
  }
  .rb-transcript-row:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--rb-accent) 60%, transparent);
  }
  .rb-speaker {
    padding-top: 0.1rem;
    color: var(--rb-sys);
    font-size: 0.6875rem;
    font-weight: 600;
  }
  .rb-speaker.is-me {
    color: var(--rb-mic);
  }
  .rb-timestamp {
    padding-top: 0.1rem;
    color: var(--rb-faint);
    font-size: 0.6875rem;
    font-variant-numeric: tabular-nums;
  }
  .rb-segment-text {
    font-size: 0.875rem;
    line-height: 1.6;
  }
  .rb-export-actions {
    display: flex;
    align-items: end;
    gap: 0.5rem;
  }
  .rb-export-actions .rb-label {
    width: min(100%, 13rem);
  }
  .rb-export-status {
    max-width: 70ch;
    overflow-wrap: anywhere;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    line-height: 1.4;
  }
  .rb-transcript-editor-row {
    display: grid;
    grid-template-columns: 5rem 7rem minmax(8rem, 0.6fr) minmax(14rem, 1.4fr) auto;
    gap: 0.5rem;
    align-items: start;
    padding: 0.65rem 0.25rem;
  }
  .rb-editor-play {
    min-height: 2.25rem;
    padding: 0.45rem;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-faint);
    font-size: 0.6875rem;
    font-variant-numeric: tabular-nums;
  }
  .rb-editor-play:hover,
  .rb-editor-play:focus-visible {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }
  .rb-editor-speaker,
  .rb-editor-name,
  .rb-editor-text {
    min-width: 0;
    font-size: 0.75rem;
  }
  .rb-editor-text {
    min-height: 3.75rem;
    resize: vertical;
    line-height: 1.45;
  }
  .rb-editor-remove {
    min-height: 2.25rem;
  }
  .rb-edit-note {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-top: 0.75rem;
    padding: 0.75rem;
    border-radius: var(--rb-radius-sm);
    background: var(--rb-info-soft);
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.45;
  }
  .rb-transcript-state {
    display: flex;
    min-height: 18rem;
    align-items: center;
    justify-content: center;
    color: var(--rb-muted);
    text-align: center;
    font-size: 0.8125rem;
  }
  .rb-transcript-state.is-error {
    color: var(--rb-record);
  }
  @media (max-width: 640px) {
    .rb-transcript-tools {
      grid-template-columns: 1fr auto;
    }
    .rb-search {
      grid-column: 1 / -1;
    }
    .rb-transcript-row {
      grid-template-columns: 3rem 2.5rem minmax(0, 1fr);
      gap: 0.5rem;
    }
    .rb-transcript-editor-row {
      grid-template-columns: 5rem minmax(0, 1fr);
    }
    .rb-editor-name,
    .rb-editor-text {
      grid-column: 1 / -1;
    }
    .rb-editor-remove {
      justify-self: end;
      grid-column: 1 / -1;
    }
    .rb-edit-note {
      align-items: flex-start;
      flex-direction: column;
    }

    .rb-export-actions {
      align-items: stretch;
      flex-direction: column;
    }

    .rb-export-actions .rb-label,
    .rb-export-actions .rb-btn {
      width: 100%;
    }

    .rb-transcript-tools {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .rb-filter {
      grid-column: 1 / -1;
    }

    .rb-filter button,
    .rb-editor-play {
      min-height: 2.75rem;
      font-size: 0.8125rem;
    }

    .rb-filter button {
      flex: 1;
    }

    .rb-transcript-tools > .rb-btn {
      width: 100%;
    }

    .rb-transcript-row {
      grid-template-columns: 3.25rem minmax(0, 1fr);
      min-height: 3.5rem;
      padding-inline: 0;
    }

    .rb-segment-text {
      grid-column: 1 / -1;
      font-size: 0.9375rem;
    }

    .rb-speaker,
    .rb-timestamp,
    .rb-result-count,
    .rb-export-status {
      font-size: 0.75rem;
    }
  }
</style>
