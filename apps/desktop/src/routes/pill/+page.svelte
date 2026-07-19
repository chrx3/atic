<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import type { DictationPhase, Levels } from "$lib/types";
  import Waveform from "$lib/Waveform.svelte";
  import {
    startRecording,
    stopRecording,
    isRecording,
    toggleDictation,
    dictationPhase,
    onLevels,
    onStatus,
    onCaptureWarn,
    onDictationStatus,
    onLiveTranscriptError,
    onLiveTranscriptFinal,
  } from "$lib/api";

  let recording = $state(false);
  let elapsed = $state(0);
  let levels = $state<Levels>({ mic: 0, system: 0 });
  let busy = $state(false);
  let dictation = $state<DictationPhase>("idle");
  let dictationMessage = $state<string | null>(null);
  let liveActive = $state(false);
  let liveError = $state<string | null>(null);
  let btWarning = $state<string | null>(null);

  let timer: ReturnType<typeof setInterval> | null = null;
  let startedAt = 0;
  let fitRaf = 0;
  let lastWidth = 0;

  const dictating = $derived(
    dictation === "listening" ||
      dictation === "transcribing" ||
      dictation === "pasted" ||
      dictation === "error",
  );

  function startTimer() {
    startedAt = Date.now();
    elapsed = 0;
    timer = setInterval(
      () => (elapsed = Math.floor((Date.now() - startedAt) / 1000)),
      500,
    );
  }
  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  function targetWidth(): number {
    if (recording) {
      return liveError || btWarning || liveActive ? 246 : 204;
    }
    if (dictation === "listening") return 214;
    if (dictation === "transcribing") return 206;
    if (dictation === "pasted" || dictation === "error") return 224;
    return 146;
  }

  async function fitWindow() {
    const width = targetWidth();
    if (width === lastWidth) return;
    try {
      lastWidth = width;
      await getCurrentWindow().setSize(new LogicalSize(width, 48));
    } catch {
      // best-effort
    }
  }

  function scheduleFit() {
    cancelAnimationFrame(fitRaf);
    fitRaf = requestAnimationFrame(() => {
      fitRaf = requestAnimationFrame(() => void fitWindow());
    });
  }

  async function toggleRecord() {
    if (busy || dictating) return;
    busy = true;
    try {
      if (recording) await stopRecording();
      else await startRecording();
    } catch (e) {
      liveError = String(e);
    } finally {
      busy = false;
      scheduleFit();
    }
  }

  async function toggleDictate() {
    if (busy || recording) return;
    busy = true;
    try {
      await toggleDictation();
    } catch (e) {
      dictationMessage = String(e);
      dictation = "error";
    } finally {
      busy = false;
      scheduleFit();
    }
  }

  function fmt(secs: number): string {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  function dictationLabel(phase: DictationPhase): string {
    switch (phase) {
      case "listening":
        return "Dictando…";
      case "transcribing":
        return "Transcribiendo…";
      case "pasted":
        return dictationMessage ?? "Pegado";
      case "error":
        return dictationMessage ?? "Error";
      default:
        return "Dictar";
    }
  }

  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    (async () => {
      recording = await isRecording();
      if (recording) startTimer();
      try {
        dictation = await dictationPhase();
      } catch {
        dictation = "idle";
      }
      scheduleFit();
    })();

    unlisteners.push(
      onStatus((s) => {
        recording = s.active;
        if (s.active) {
          startTimer();
          liveActive = false;
          liveError = null;
        } else {
          stopTimer();
          liveActive = false;
          btWarning = null;
        }
        scheduleFit();
      }),
      onLevels((l) => (levels = l)),
      onCaptureWarn((message) => {
        btWarning = message;
        scheduleFit();
      }),
      onLiveTranscriptFinal(() => {
        liveActive = true;
        liveError = null;
        scheduleFit();
      }),
      onLiveTranscriptError((message) => {
        liveError = message;
        scheduleFit();
      }),
      onDictationStatus((s) => {
        dictation = s.phase;
        dictationMessage = s.message;
        scheduleFit();
      }),
    );

    return () => {
      stopTimer();
      cancelAnimationFrame(fitRaf);
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  });
</script>

<div class="pill-root">
  <div
    data-tauri-drag-region
    class="pill-shell group"
    role="toolbar"
    aria-label="Controles rápidos de Atic"
  >
    <button
      class="pill-rec flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full text-white transition disabled:opacity-50"
      class:is-live={recording}
      onclick={toggleRecord}
      disabled={busy || dictating}
      aria-label={recording ? "Detener grabación" : "Grabar reunión"}
      title={btWarning ?? (recording ? "Detener grabación" : "Grabar reunión")}
    >
      {#if recording}
        <span class="h-2 w-2 rounded-[2px] bg-white" aria-hidden="true"></span>
      {:else}
        <span class="h-2 w-2 rounded-full bg-white" aria-hidden="true"></span>
      {/if}
    </button>

    <button
      class="pill-dict flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-full transition disabled:opacity-50"
      class:is-live={dictation === "listening"}
      class:is-busy={dictation === "transcribing"}
      class:is-ok={dictation === "pasted"}
      class:is-error={dictation === "error"}
      onclick={toggleDictate}
      disabled={busy || recording || dictation === "transcribing"}
      aria-label={dictation === "listening" ? "Detener dictado" : "Dictar"}
      title="Dictar (toggle). El atajo global puede ser push-to-talk."
    >
      <span class="pill-mic" aria-hidden="true"></span>
    </button>

    {#if recording}
      <span data-tauri-drag-region class="pill-timer shrink-0 tabular-nums" aria-live="off">
        {fmt(elapsed)}
      </span>
      {#if liveError}
        <span data-tauri-drag-region class="pill-live is-error shrink-0" role="status">
          Error live
        </span>
      {:else if btWarning}
        <span
          data-tauri-drag-region
          class="pill-live is-warn shrink-0"
          role="status"
          title={btWarning}
        >
          BT audio
        </span>
      {:else if liveActive}
        <span data-tauri-drag-region class="pill-live shrink-0" role="status">
          En vivo
        </span>
      {/if}
      <Waveform
        mic={levels.mic}
        system={levels.system}
        bars={10}
        variant="quiet"
      />
    {:else if dictating}
      <span
        data-tauri-drag-region
        class="pill-idle min-w-0 flex-1"
        role="status"
        aria-live="polite"
        title={dictationLabel(dictation)}
      >
        {dictationLabel(dictation)}
      </span>
      {#if dictation === "listening"}
        <Waveform mic={levels.mic} system={0} bars={8} variant="quiet" />
      {/if}
    {:else}
      <span data-tauri-drag-region class="pill-idle min-w-0 flex-1">Listo</span>
    {/if}
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    overflow: hidden;
    background: transparent;
    margin: 0;
    width: 100%;
    height: 100%;
  }

  .pill-root {
    display: flex;
    width: 100vw;
    height: 100vh;
    box-sizing: border-box;
    padding: 4px;
    overflow: hidden;
  }

  .pill-shell {
    display: flex;
    width: 100%;
    height: 40px;
    min-width: 0;
    align-items: center;
    gap: 7px;
    border-radius: 999px;
    padding: 0 9px 0 6px;
    cursor: grab;
    background: color-mix(in srgb, var(--rb-surface) 97%, transparent);
    color: var(--rb-text);
    box-shadow: 0 5px 14px rgba(0, 0, 0, 0.3);
    overflow: hidden;
    white-space: nowrap;
  }
  .pill-shell:active {
    cursor: grabbing;
  }

  @media (prefers-color-scheme: light) {
    .pill-shell {
      background: color-mix(in srgb, #ffffff 97%, transparent);
      box-shadow: 0 5px 14px rgba(15, 23, 32, 0.12);
    }
  }

  .pill-rec {
    width: 28px;
    height: 28px;
    background: var(--rb-record);
    transition:
      filter 0.16s ease,
      opacity 0.16s ease,
      transform 0.12s ease;
  }

  .pill-rec.is-live {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--rb-record) 22%, transparent);
  }

  .pill-dict {
    width: 28px;
    height: 28px;
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 10%, transparent);
    transition:
      color 0.16s ease,
      background 0.16s ease,
      opacity 0.16s ease,
      transform 0.12s ease;
  }
  .pill-dict.is-live {
    color: #fbfbf8;
    background: var(--rb-accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--rb-accent) 22%, transparent);
  }
  .pill-dict.is-busy {
    color: var(--rb-warn);
    background: var(--rb-warn-soft);
  }
  .pill-dict.is-ok {
    color: var(--rb-ok);
    background: var(--rb-ok-soft);
  }
  .pill-dict.is-error {
    color: var(--rb-record);
    background: var(--rb-record-soft);
  }

  .pill-mic {
    width: 10px;
    height: 10px;
    border-radius: 999px 999px 4px 4px;
    background: currentColor;
    box-shadow: 0 3px 0 -1px currentColor;
  }

  .pill-timer {
    min-width: 2.5rem;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0;
    color: var(--rb-text);
    font-variant-numeric: tabular-nums;
    font-family: var(--rb-font);
  }

  .pill-live {
    overflow: hidden;
    max-width: 4.5rem;
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0;
    color: var(--rb-accent);
    font-family: var(--rb-font);
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .pill-live.is-error {
    color: var(--rb-record);
  }
  .pill-live.is-warn {
    color: var(--rb-warn);
  }

  .pill-idle {
    min-width: 0;
    overflow: hidden;
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--rb-muted);
    padding-right: 2px;
    font-family: var(--rb-font);
    white-space: nowrap;
    flex: 1 1 auto;
    text-overflow: ellipsis;
  }

  .pill-shell,
  .pill-shell * {
    user-select: none !important;
    -webkit-user-select: none !important;
  }

  .pill-shell :global(button) {
    -webkit-app-region: no-drag;
  }

  .pill-shell :global(button:hover:not(:disabled)) {
    filter: brightness(1.08);
  }

  .pill-shell :global(button:active:not(:disabled)) {
    transform: scale(0.94);
  }

  .pill-shell :global(button:focus-visible) {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  @media (prefers-reduced-motion: reduce) {
    .pill-rec,
    .pill-dict {
      transition: none;
    }
  }
</style>
