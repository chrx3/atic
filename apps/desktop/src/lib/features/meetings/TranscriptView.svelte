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
  import type { Segment } from "$core/types";
  import EmptyState from "$ui/EmptyState.svelte";

  let { recordingId }: { recordingId: string } = $props();

  // Se pide al cambiar de grabación. El store cachea, así que volver a una ya
  // leída no vuelve a viajar a Rust.
  $effect(() => {
    void recordings.loadTranscript(recordingId).catch(toastError);
  });

  const transcript = $derived(recordings.transcripts[recordingId]);
  const loading = $derived(!(recordingId in recordings.transcripts));

  /** Segmentos consecutivos del mismo hablante, en un bloque. */
  const blocks = $derived.by(() => {
    const segments = transcript?.segments ?? [];
    const out: { speaker: string; startMs: number; text: string }[] = [];
    for (const segment of segments) {
      const who = speakerName(segment);
      const last = out.at(-1);
      if (last && last.speaker === who) {
        last.text += ` ${segment.text.trim()}`;
      } else {
        out.push({
          speaker: who,
          startMs: segment.start_ms,
          text: segment.text.trim(),
        });
      }
    }
    return out;
  });

  function speakerName(segment: Segment): string {
    return segment.speaker_name ?? (segment.speaker === "me" ? "Yo" : "Otros");
  }

  function stamp(ms: number): string {
    const total = Math.max(0, Math.floor(ms / 1000));
    const m = Math.floor(total / 60);
    const s = total % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  }
</script>

{#if loading}
  <p class="px-1 py-3 text-xs text-faint">Cargando transcripción…</p>
{:else if !transcript || blocks.length === 0}
  <EmptyState
    title="Sin transcripción"
    hint="Apretá Transcribir para generarla. Corre local, así que tarda un rato."
  />
{:else}
  <div class="flex flex-col gap-3">
    {#each blocks as block, i (i)}
      <div class="flex gap-2">
        <span
          class="w-10 shrink-0 pt-px text-right font-mono text-xs text-faint"
          data-numeric
        >
          {stamp(block.startMs)}
        </span>
        <div class="flex min-w-0 flex-col gap-0.5">
          <span class="text-micro text-faint uppercase">{block.speaker}</span>
          <p class="text-sm text-text">{block.text}</p>
        </div>
      </div>
    {/each}
  </div>
{/if}
