<script lang="ts">
  import type { DictationPhase, Levels } from "$lib/types";
  import { formatDuration } from "$lib/format";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import Waveform from "$lib/Waveform.svelte";

  let {
    recording,
    elapsed,
    levels,
    busy,
    importing = false,
    dictation,
    dictationMessage,
    liveActive = false,
    liveError = null,
    recordingNote = null,
    onToggle,
    onToggleDictation,
    onImport,
  }: {
    recording: boolean;
    elapsed: number;
    levels: Levels;
    busy: boolean;
    importing?: boolean;
    dictation: DictationPhase;
    dictationMessage: string | null;
    liveActive?: boolean;
    liveError?: string | null;
    recordingNote?: string | null;
    onToggle: () => void | Promise<void>;
    onToggleDictation: () => void | Promise<void>;
    onImport: () => void | Promise<void>;
  } = $props();

  const dictating = $derived(
    dictation === "listening" ||
      dictation === "transcribing" ||
      dictation === "pasted" ||
      dictation === "error",
  );

  const dictationLabel = $derived.by(() => {
    switch (dictation) {
      case "listening":
        return "Terminar dictado";
      case "transcribing":
        return "Transcribiendo…";
      case "pasted":
        return dictationMessage ?? "Texto pegado";
      case "error":
        return dictationMessage ?? "Reintentar dictado";
      default:
        return "Dictar texto";
    }
  });
</script>

<section class="rb-recorder" aria-label="Controles de audio">
  <div class="rb-recorder-primary">
    <button
      type="button"
      class="rb-rec-btn"
      class:is-live={recording}
      onclick={onToggle}
      disabled={busy || dictating || importing}
      aria-label={recording ? "Detener grabación" : "Grabar reunión"}
    >
      {#if recording}
        <span class="rb-rec-glyph is-stop" aria-hidden="true"></span>
      {:else}
        <span class="rb-rec-glyph" aria-hidden="true"></span>
      {/if}
    </button>

    <div class="min-w-0">
      <p class="rb-recorder-title">{recording ? "Grabando reunión" : "Grabar reunión"}</p>
      <p class="rb-recorder-copy">
        {recording
          ? liveError
            ? `${formatDuration(elapsed)} · error en vivo`
            : liveActive
              ? `${formatDuration(elapsed)} · transcribiendo en vivo`
              : `${formatDuration(elapsed)} · captura local en curso`
          : importing
            ? "Importando audio…"
            : "Micrófono y audio del sistema, solo en este equipo"}
      </p>
      {#if recording && recordingNote}
        <p class="rb-recorder-note" role="status">{recordingNote}</p>
      {/if}
    </div>
  </div>

  <div class="rb-recorder-meter" aria-live="polite">
    {#if recording}
      <div class="rb-recorder-level">
        <span>Yo</span>
        <Waveform level={levels.mic} color="mic" bars={16} variant="quiet" />
      </div>
      <div class="rb-recorder-level">
        <span>Otros</span>
        <Waveform level={levels.system} color="sys" bars={16} variant="quiet" />
      </div>
    {:else if dictation === "listening"}
      <div class="rb-recorder-level">
        <span>Escuchando</span>
        <Waveform level={levels.mic} color="mic" bars={16} variant="quiet" />
      </div>
    {:else}
      <p>Elige una acción.</p>
    {/if}
  </div>

  <div class="rb-recorder-actions">
    <button
      type="button"
      class="rb-btn rb-btn-soft"
      onclick={onImport}
      disabled={busy || recording || dictating || importing}
    >
      {importing ? "Importando…" : "Importar audio…"}
    </button>
    <button
      type="button"
      class="rb-btn rb-btn-soft rb-dictate-btn"
      class:is-active={dictation === "listening"}
      onclick={onToggleDictation}
      disabled={busy || recording || importing || dictation === "transcribing"}
    >
      <ToolIcon id="dictation" size={15} strokeWidth={1.4} />
      {dictationLabel}
    </button>
  </div>
</section>

<style>
  .rb-recorder {
    display: grid;
    grid-template-columns: 1fr;
    align-items: center;
    gap: 0.875rem;
    padding: 0.875rem;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  .rb-rec-glyph {
    width: 0.7rem;
    height: 0.7rem;
    border-radius: 999px;
    background: currentColor;
    transition: border-radius 0.15s ease;
  }

  .rb-rec-glyph.is-stop {
    border-radius: 1px;
  }

  @container atic-main (min-width: 30rem) {
    .rb-recorder {
      grid-template-columns: 1fr auto;
      gap: 1rem;
      padding: 1rem;
    }

    .rb-recorder-meter {
      grid-column: 1 / -1;
      padding-top: 0.65rem;
    }
  }

  @container atic-main (min-width: 37rem) {
    .rb-recorder {
      grid-template-columns: minmax(14rem, 1fr) minmax(12rem, 0.9fr) auto;
    }

    .rb-recorder-meter {
      grid-column: auto;
      padding-top: 0;
    }
  }
  .rb-recorder-primary {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.85rem;
  }
  .rb-recorder-title {
    color: var(--rb-text);
    font-size: 0.875rem;
    font-weight: 650;
  }
  .rb-recorder-copy,
  .rb-recorder-meter > p {
    margin-top: 0.15rem;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    line-height: 1.4;
  }
  .rb-recorder-note {
    margin-top: 0.35rem;
    color: var(--rb-warn);
    font-size: 0.6875rem;
    line-height: 1.4;
  }
  .rb-recorder-meter {
    display: flex;
    min-height: 2.5rem;
    flex-direction: column;
    justify-content: center;
    gap: 0.35rem;
  }
  .rb-recorder-level {
    display: grid;
    grid-template-columns: 4.25rem minmax(0, 1fr);
    align-items: center;
    gap: 0.65rem;
    color: var(--rb-faint);
    font-size: 0.5625rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .rb-recorder-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .rb-dictate-btn {
    min-width: 9.75rem;
  }
  .rb-dictate-btn.is-active {
    color: var(--rb-on-accent, #fbfbf8);
    background: var(--rb-accent);
    box-shadow: none;
  }
  @container atic-main (max-width: 29.999rem) {
    .rb-recorder-actions {
      width: 100%;
      flex-direction: column;
      align-items: stretch;
    }
    .rb-dictate-btn {
      width: 100%;
    }

    .rb-recorder-actions .rb-btn {
      width: 100%;
    }
  }
</style>
