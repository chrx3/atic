<script lang="ts">
  /**
   * La transcripción de una grabación.
   *
   * Se agrupa por hablante en vez de listar segmento por segmento: Whisper
   * corta cada pocas frases, y una lista de treinta líneas con la misma
   * etiqueta repetida se lee peor que cuatro bloques. La marca de tiempo es la
   * del primer segmento del bloque.
   */
  import { recordings } from "$domain/recordings.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import type { Segment, Speaker } from "$core/types";
  import { isJunkTranscriptText } from "$core/transcriptText";
  import { playback } from "$domain/playback.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import { t } from "$domain/i18n.svelte";

  let { recordingId }: { recordingId: string } = $props();

  // Se pide al cambiar de grabación. El store cachea, así que volver a una ya
  // leída no vuelve a viajar a Rust.
  $effect(() => {
    void recordings.loadTranscript(recordingId).catch(toastError);
  });

  const transcript = $derived(recordings.transcripts[recordingId]);
  const loading = $derived(!(recordingId in recordings.transcripts));

  const recording = $derived(
    recordings.items.find((item) => item.id === recordingId) ?? null,
  );

  /** Segmentos consecutivos del mismo hablante, en un bloque. */
  const blocks = $derived.by(() => {
    const segments = (transcript?.segments ?? []).filter(
      (segment) => !isJunkTranscriptText(segment.text),
    );
    const out: { speaker: Speaker; label: string; startMs: number; text: string }[] =
      [];
    for (const segment of segments) {
      const label = speakerName(segment);
      const last = out.at(-1);
      if (last && last.label === label) {
        last.text += ` ${segment.text.trim()}`;
      } else {
        out.push({
          speaker: segment.speaker,
          label,
          startMs: segment.start_ms,
          text: segment.text.trim(),
        });
      }
    }
    return out;
  });

  function speakerName(segment: Segment): string {
    return (
      segment.speaker_name ??
      (segment.speaker === "me" ? t("page.meetings.me") : t("page.meetings.others"))
    );
  }

  function stamp(ms: number): string {
    const total = Math.max(0, Math.floor(ms / 1000));
    const m = Math.floor(total / 60);
    const s = total % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  }
</script>

{#if loading}
  <p class="px-1 py-3 text-xs text-faint">{t("page.meetings.loadingTranscript")}</p>
{:else if !transcript || blocks.length === 0}
  <EmptyState
    title={t("page.meetings.noTranscript")}
    hint={t("page.meetings.noTranscriptHint")}
  />
{:else}
  <div class="flex flex-col gap-3">
    {#each blocks as block, i (i)}
      <button
        type="button"
        class="flex gap-2 rounded-xs px-1 py-0.5 text-left
               transition-colors duration-(--duration-quick) ease-calm
               hover:bg-surface-2"
        aria-label={t("page.meetings.listenFrom", { time: stamp(block.startMs) })}
        onclick={() => {
          if (!recording) return;
          void playback.playSpeaker(recording, block.speaker, block.startMs / 1000);
        }}
      >
        <span
          class="w-10 shrink-0 pt-px text-right font-mono text-xs text-faint"
          data-numeric
        >
          {stamp(block.startMs)}
        </span>
        <div class="flex min-w-0 flex-col gap-0.5">
          <span class="text-micro text-faint uppercase">{block.label}</span>
          <span class="text-sm text-text">{block.text}</span>
        </div>
      </button>
    {/each}
  </div>
{/if}
