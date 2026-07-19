<script lang="ts">
  import { tick } from "svelte";
  import type { Recording } from "$lib/types";
  import { formatDate, formatDuration, statusLabel } from "$lib/format";
  import { playback, type AudioTrack } from "$lib/playback.svelte";
  import AudioPlayer from "$lib/AudioPlayer.svelte";

  let {
    recording,
    modelReady,
    modelName,
    progress,
    onRename,
    onTranscribe,
    onOpenTranscript,
    onOpenSummary,
    onDelete,
  }: {
    recording: Recording | null;
    modelReady: boolean;
    modelName?: string;
    progress?: number;
    onRename: (title: string) => void | Promise<void>;
    onTranscribe: () => void | Promise<void>;
    onOpenTranscript: () => void;
    onOpenSummary: () => void;
    onDelete: () => void;
  } = $props();

  let editing = $state(false);
  let editTitle = $state("");
  let renaming = $state(false);
  let titleInput = $state<HTMLInputElement>();

  const hasTranscript = $derived(
    recording?.status === "transcribed" ||
      recording?.status === "summarizing" ||
      recording?.status === "summarized",
  );
  const hasSummary = $derived(recording?.status === "summarized");
  const isTranscribing = $derived(
    recording?.status === "transcribing" || progress !== undefined,
  );
  const isSummarizing = $derived(recording?.status === "summarizing");

  async function beginEditing() {
    editTitle = recording?.title ?? "";
    editing = true;
    await tick();
    titleInput?.focus();
    titleInput?.select();
  }

  function captureTitleInput(node: HTMLInputElement) {
    titleInput = node;
    return () => {
      if (titleInput === node) titleInput = undefined;
    };
  }

  async function commitRename() {
    if (!recording || renaming) return;
    const next = editTitle.trim() || "Sin título";
    editing = false;
    if (next === recording.title) return;
    renaming = true;
    try {
      await onRename(next);
    } finally {
      renaming = false;
    }
  }

  async function toggleTrack(track: AudioTrack) {
    if (!recording) return;
    if (playback.isActive(recording.id, track)) {
      await playback.toggle();
    } else {
      await playback.play(recording, track);
    }
  }
</script>

<section class="rb-detail" aria-label="Detalle de grabación">
  {#if !recording}
    <div class="rb-detail-empty">
      <p class="font-medium">Selecciona una grabación</p>
      <p>Sus pistas, texto y resumen aparecerán aquí.</p>
    </div>
  {:else}
    <header class="rb-detail-header">
      <div class="min-w-0 flex-1">
        {#if editing}
          <input
            {@attach captureTitleInput}
            class="rb-field rb-title-input"
            bind:value={editTitle}
            onblur={commitRename}
            onkeydown={(event) => {
              if (event.key === "Enter") commitRename();
              if (event.key === "Escape") editing = false;
            }}
            aria-label="Título de la grabación"
          />
        {:else}
          <button
            class="rb-detail-title"
            onclick={beginEditing}
            title="Renombrar"
          >
            {recording.title}
          </button>
        {/if}
        <p class="rb-detail-meta">
          {formatDate(recording.started_at)} · {formatDuration(recording.duration_secs)}
        </p>
      </div>

      <details class="rb-detail-menu">
        <summary aria-label="Más acciones">Más</summary>
        <div class="rb-detail-menu-popover">
          {#if hasTranscript}
            <button onclick={onTranscribe} disabled={!modelReady || isTranscribing}>
              Rehacer transcripción
            </button>
          {/if}
          <button onclick={beginEditing}>Renombrar</button>
          <button class="is-danger" onclick={onDelete}>Eliminar</button>
        </div>
      </details>
    </header>

    <div class="rb-pipeline" aria-label="Progreso">
      <span class="is-done">Audio</span>
      <span class:is-done={hasTranscript} class:is-active={isTranscribing}>
        {isTranscribing ? "Transcribiendo" : "Texto"}
      </span>
      <span class:is-done={hasSummary} class:is-active={isSummarizing}>
        {isSummarizing ? "Generando" : "Resumen"}
      </span>
    </div>

    <div class="rb-detail-section">
      <div class="rb-detail-section-heading">
        <h3>Audio</h3>
        <span>{statusLabel(recording.status)}</span>
      </div>

      <div class="rb-track-switcher" aria-label="Pista de audio">
        {#if recording.mic_path}
          <button
            class:is-active={playback.isActive(recording.id, "mic")}
            onclick={() => toggleTrack("mic")}
            aria-pressed={playback.isActive(recording.id, "mic")}
          >
            {playback.isActive(recording.id, "mic") && playback.playing
              ? "Pausar Yo"
              : "Yo"}
          </button>
        {/if}
        {#if recording.system_path}
          <button
            class:is-active={playback.isActive(recording.id, "system")}
            onclick={() => toggleTrack("system")}
            aria-pressed={playback.isActive(recording.id, "system")}
          >
            {playback.isActive(recording.id, "system") && playback.playing
              ? "Pausar Otros"
              : "Otros"}
          </button>
        {/if}
      </div>

      <div class="rb-detail-player">
        <AudioPlayer alwaysVisible dismissible={false} />
      </div>
    </div>

    <div class="rb-detail-actions">
      {#if isTranscribing}
        <div class="rb-progress-state" role="status">
          <span>
            Transcribiendo{modelName ? ` con ${modelName}` : ""} en este equipo…
          </span>
          <strong>{progress !== undefined ? `${Math.round(progress * 100)}%` : ""}</strong>
          <div>
            <i style="width: {progress !== undefined ? progress * 100 : 25}%"></i>
          </div>
        </div>
      {:else if hasTranscript}
        <button class="rb-btn rb-btn-primary" onclick={onOpenTranscript}>
          Ver transcripción
        </button>
        <button class="rb-btn rb-btn-soft" onclick={onOpenSummary}>
          {hasSummary ? "Ver resumen" : isSummarizing ? "Ver generación" : "Generar resumen"}
        </button>
      {:else}
        <button
          class="rb-btn rb-btn-primary"
          onclick={onTranscribe}
          disabled={!modelReady}
          title={modelReady ? "Transcribir localmente" : "Descarga primero el modelo"}
        >
          Transcribir audio
        </button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .rb-detail {
    min-height: 0;
    overflow: visible;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface-2);
  }
  .rb-detail-empty {
    display: flex;
    min-height: 18rem;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    padding: 1.5rem;
    color: var(--rb-muted);
    text-align: center;
    font-size: 0.75rem;
  }
  .rb-detail-header {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 1rem;
    border-bottom: 0;
  }
  .rb-detail-title {
    display: block;
    max-width: 100%;
    overflow: hidden;
    color: var(--rb-text);
    font-size: 1rem;
    font-weight: 600;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rb-detail-title:hover {
    color: color-mix(in srgb, var(--rb-text) 80%, var(--rb-accent));
  }
  .rb-detail-title:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-title-input {
    padding-block: 0.35rem;
  }
  .rb-detail-meta {
    margin-top: 0.25rem;
    color: var(--rb-faint);
    font-size: 0.6875rem;
  }
  .rb-detail-menu {
    position: relative;
  }
  .rb-detail-menu summary {
    display: flex;
    min-height: 2rem;
    align-items: center;
    padding: 0.35rem 0.65rem;
    border-radius: var(--rb-radius-sm);
    color: var(--rb-muted);
    cursor: pointer;
    font-size: 0.75rem;
    list-style: none;
  }
  .rb-detail-menu summary::-webkit-details-marker {
    display: none;
  }
  .rb-detail-menu summary:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }
  .rb-detail-menu summary:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-detail-menu-popover {
    position: absolute;
    z-index: 5;
    top: calc(100% + 0.3rem);
    right: 0;
    display: flex;
    width: 11rem;
    flex-direction: column;
    padding: 0.3rem;
    border: 0;
    border-radius: var(--rb-radius-sm);
    background: var(--rb-surface-2);
    box-shadow: 0 12px 24px rgba(0, 0, 0, 0.24);
  }
  .rb-detail-menu-popover button {
    padding: 0.5rem 0.6rem;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-text);
    text-align: left;
    font-size: 0.75rem;
  }
  .rb-detail-menu-popover button:hover {
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }
  .rb-detail-menu-popover button.is-danger {
    color: var(--rb-record);
  }
  .rb-pipeline {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.25rem;
    padding: 0 1rem;
    background: transparent;
  }
  .rb-pipeline span {
    padding: 0.5rem;
    color: var(--rb-faint);
    border-radius: var(--rb-radius-xs);
    background: color-mix(in srgb, var(--rb-text) 4%, transparent);
    font-size: 0.625rem;
    text-align: center;
  }
  .rb-pipeline span.is-done {
    color: var(--rb-accent);
  }
  .rb-pipeline span.is-active {
    color: var(--rb-warn);
  }
  .rb-detail-section {
    padding: 1rem;
  }
  .rb-detail-section-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.75rem;
  }
  .rb-detail-section-heading h3 {
    font-size: 0.75rem;
    font-weight: 600;
  }
  .rb-detail-section-heading span {
    color: var(--rb-faint);
    font-size: 0.625rem;
  }
  .rb-track-switcher {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--rb-radius-sm);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }
  .rb-track-switcher button {
    min-height: 1.75rem;
    padding: 0.3rem 0.7rem;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-muted);
    font-size: 0.6875rem;
  }
  .rb-track-switcher button:hover {
    color: var(--rb-text);
  }
  .rb-track-switcher button.is-active {
    color: var(--rb-text);
    background: var(--rb-surface-2);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  }
  .rb-track-switcher button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-detail-player {
    margin-top: 0.75rem;
    padding: 0.75rem;
    border-radius: var(--rb-radius-sm);
    background: color-mix(in srgb, var(--rb-bg0) 55%, var(--rb-surface));
  }
  .rb-detail-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0 1rem 1rem;
  }
  .rb-progress-state {
    display: grid;
    width: 100%;
    grid-template-columns: 1fr auto;
    gap: 0.4rem;
    color: var(--rb-muted);
    font-size: 0.6875rem;
  }
  .rb-progress-state strong {
    color: var(--rb-text);
    font-variant-numeric: tabular-nums;
  }
  .rb-progress-state div {
    grid-column: 1 / -1;
    height: 3px;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }
  .rb-progress-state i {
    display: block;
    height: 100%;
    min-width: 12%;
    border-radius: inherit;
    background: var(--rb-accent);
  }

  @media (max-width: 760px) {
    .rb-detail {
      border-radius: var(--rb-radius);
    }
  }

  @media (max-width: 36rem) {
    .rb-detail-header,
    .rb-detail-section {
      padding: 0.875rem;
    }

    .rb-detail-title {
      font-size: 1rem;
    }

    .rb-detail-menu summary {
      min-height: 2.75rem;
    }

    .rb-detail-meta,
    .rb-detail-section-heading span,
    .rb-pipeline span {
      font-size: 0.75rem;
    }

    .rb-detail-actions {
      padding: 0 0.875rem 0.875rem;
    }

    .rb-detail-actions .rb-btn {
      width: 100%;
    }

    .rb-track-switcher {
      display: flex;
    }

    .rb-track-switcher button {
      min-height: 2.75rem;
      flex: 1;
      font-size: 0.8125rem;
    }
  }
</style>
