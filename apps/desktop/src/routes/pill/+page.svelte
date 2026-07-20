<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { PhysicalPosition } from "@tauri-apps/api/dpi";
  import {
    currentMonitor,
    getCurrentWindow,
    LogicalSize,
  } from "@tauri-apps/api/window";
  import type { ClipboardItem, DictationPhase, Levels } from "$lib/types";
  import Waveform from "$lib/Waveform.svelte";
  import X from "reicon-svelte/icons/X.svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import {
    startRecording,
    stopRecording,
    isRecording,
    toggleDictation,
    dictationPhase,
    showMainWindow,
    listClipboardHistory,
    onLevels,
    onStatus,
    onCaptureWarn,
    onDictationStatus,
    onLiveTranscriptError,
    onLiveTranscriptFinal,
    prepareClipboardPill,
    restorePillPosition,
    onPillClipboardToggle,
    onPillClipboardClose,
    onPillReset,
    onClipboardHistoryChanged,
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

  let clipboardOpen = $state(false);
  let clipboardItems = $state<ClipboardItem[]>([]);
  let clipboardLoading = $state(false);
  let clipboardOpenedAt = 0;
  let pasting = $state(false);
  /** Clipboard siempre crece hacia arriba (barra abajo, lista arriba). */
  let expandUp = $state(false);

  let timer: ReturnType<typeof setInterval> | null = null;
  let startedAt = 0;
  let fitRaf = 0;
  let fitting = false;

  const COMPACT_H = 48;
  const IDLE_W = 112;
  const EXPANDED_W = 320;
  const EXPANDED_H = 380;

  const dictating = $derived(
    dictation === "listening" ||
      dictation === "transcribing" ||
      dictation === "pasted" ||
      dictation === "error",
  );

  const mode = $derived.by(() => {
    if (clipboardOpen) return "clipboard" as const;
    if (recording) return "recording" as const;
    if (dictating) return "dictation" as const;
    return "idle" as const;
  });

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

  function targetSize(): { w: number; h: number } {
    if (mode === "clipboard") return { w: EXPANDED_W, h: EXPANDED_H };
    if (mode === "recording") {
      return {
        w: liveError || btWarning || liveActive ? 248 : 210,
        h: COMPACT_H,
      };
    }
    if (mode === "dictation") {
      if (dictation === "listening") return { w: 210, h: COMPACT_H };
      if (dictation === "transcribing") return { w: 200, h: COMPACT_H };
      return { w: 220, h: COMPACT_H };
    }
    return { w: IDLE_W, h: COMPACT_H };
  }

  async function fitWindow() {
    if (fitting) return;
    fitting = true;
    const { w, h } = targetSize();
    const win = getCurrentWindow();
    try {
      await win.setMinSize(new LogicalSize(100, 40));
      await win.setMaxSize(new LogicalSize(360, 420));
      await win.setSize(new LogicalSize(w, h));
      if (mode !== "clipboard") expandUp = false;
    } catch (err) {
      console.warn("pill setSize", err);
    } finally {
      fitting = false;
    }
  }

  /** Expande el historial anclado al compacto actual; elige arriba/abajo según espacio. */
  async function fitClipboardExpanded() {
    if (fitting) return;
    fitting = true;
    const win = getCurrentWindow();
    try {
      const prevPos = await win.outerPosition();
      const prevSize = await win.outerSize();
      const monitor = await currentMonitor();

      await win.setMinSize(new LogicalSize(100, 40));
      await win.setMaxSize(new LogicalSize(360, 420));
      await win.setSize(new LogicalSize(EXPANDED_W, EXPANDED_H));
      const nextSize = await win.outerSize();
      const grow = nextSize.height - prevSize.height;

      const monTop = monitor ? monitor.position.y + 8 : 8;
      const monBottom = monitor
        ? monitor.position.y + monitor.size.height - 8
        : prevPos.y + nextSize.height;
      const spaceBelow = monBottom - (prevPos.y + prevSize.height);

      // Preferir abrir hacia abajo si cabe; si no, hacia arriba.
      if (spaceBelow >= grow) {
        expandUp = false;
        await win.setPosition(new PhysicalPosition(prevPos.x, prevPos.y));
      } else {
        expandUp = true;
        const nextY = Math.max(monTop, prevPos.y + prevSize.height - nextSize.height);
        // Si aún se sale por abajo, subir más.
        const bottom = nextY + nextSize.height;
        const clampedY =
          bottom > monBottom ? Math.max(monTop, monBottom - nextSize.height) : nextY;
        await win.setPosition(new PhysicalPosition(prevPos.x, clampedY));
      }
      // Evitar que quede cortado a la derecha/izquierda del monitor.
      if (monitor) {
        const pos = await win.outerPosition();
        const size = await win.outerSize();
        const minX = monitor.position.x + 8;
        const maxX = monitor.position.x + monitor.size.width - size.width - 8;
        const nextX = Math.min(Math.max(pos.x, minX), Math.max(minX, maxX));
        if (nextX !== pos.x) {
          await win.setPosition(new PhysicalPosition(nextX, pos.y));
        }
      }
    } catch (err) {
      console.warn("pill clipboard fit", err);
    } finally {
      fitting = false;
    }
  }

  function scheduleFit() {
    cancelAnimationFrame(fitRaf);
    fitRaf = requestAnimationFrame(() => {
      fitRaf = requestAnimationFrame(() => void fitWindow());
    });
  }

  async function refreshClipboard() {
    clipboardLoading = true;
    try {
      clipboardItems = await listClipboardHistory();
    } catch {
      clipboardItems = [];
    } finally {
      clipboardLoading = false;
    }
  }

  async function openClipboardPanel() {
    clipboardOpen = true;
    clipboardOpenedAt = Date.now();
    await fitClipboardExpanded();
    await refreshClipboard();
  }

  async function closeClipboardPanel() {
    if (!clipboardOpen) return;
    clipboardOpen = false;
    expandUp = false;
    try {
      await restorePillPosition();
    } catch (err) {
      console.warn("restore pill position", err);
    }
    scheduleFit();
  }

  /** Estado limpio compacto (tras traer pill o al reabrir clipboard). */
  async function resetPillChrome() {
    clipboardOpen = false;
    expandUp = false;
    pasting = false;
    const win = getCurrentWindow();
    try {
      await win.setSize(new LogicalSize(IDLE_W, COMPACT_H));
    } catch {
      // best-effort
    }
  }

  /**
   * Atajo clipboard: si ya está abierto, cierra y reabre en el cursor
   * (recalcula arriba/abajo). No deja el panel solo cerrado.
   */
  async function onClipboardHotkey() {
    if (clipboardOpen) {
      await resetPillChrome();
      // Un frame para que el DOM compacto aplique antes de medir/expandir.
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => resolve());
      });
    }
    try {
      await prepareClipboardPill();
    } catch (err) {
      console.warn("prepare clipboard pill", err);
    }
    await openClipboardPanel();
  }

  /** Arrastre de la ventana; no compite con ítems del historial. */
  function beginDrag(event: PointerEvent) {
    const target = event.target as HTMLElement | null;
    if (!target) return;
    if (
      target.closest(
        "button, a, input, textarea, [data-no-drag], .clip-item, .clip-items",
      )
    ) {
      return;
    }
    event.preventDefault();
    void getCurrentWindow().startDragging();
  }

  async function toggleRecord() {
    if (busy || dictating || clipboardOpen) return;
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
    if (busy || recording || clipboardOpen) return;
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

  async function openMain() {
    try {
      await showMainWindow();
    } catch {
      // best-effort
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
      onPillClipboardToggle(() => {
        void onClipboardHotkey();
      }),
      onPillClipboardClose(() => {
        pasting = false;
        void closeClipboardPanel();
      }),
      onPillReset(() => {
        void resetPillChrome();
      }),
      onClipboardHistoryChanged(() => {
        if (clipboardOpen) void refreshClipboard();
      }),
    );

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && clipboardOpen) {
        event.preventDefault();
        void closeClipboardPanel();
        return;
      }
      // Bloquea chrome del WebView (Imprimir, Buscar, DevTools, zoom…).
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (mod && ["p", "f", "g", "u", "j", "i", "r", "=", "+", "-", "0"].includes(key)) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (event.key === "F3" || event.key === "F5" || event.key === "F12") {
        event.preventDefault();
        event.stopPropagation();
      }
    };
    const onBlur = () => {
      if (pasting) return;
      if (clipboardOpen && Date.now() - clipboardOpenedAt > 400) {
        void closeClipboardPanel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onBlur);

    return () => {
      stopTimer();
      cancelAnimationFrame(fitRaf);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="pill-root"
  class:is-expanded={mode === "clipboard"}
  class:is-up={expandUp && mode === "clipboard"}
  onpointerdown={beginDrag}
>
  {#if mode === "clipboard" && expandUp}
    <div class="pill-panel" data-no-drag>
      <ClipboardHistoryList
        items={clipboardItems}
        loading={clipboardLoading}
        compact
        onRefresh={refreshClipboard}
        onPasteStart={() => {
          pasting = true;
        }}
        onPasted={() => {
          void closeClipboardPanel();
        }}
        onError={() => {
          pasting = false;
        }}
      />
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="pill-shell"
    class:is-expanded={mode === "clipboard"}
    class:is-up={expandUp && mode === "clipboard"}
    role="status"
    aria-label="Estado de Atic"
    title={mode === "idle" ? "Arrastra para mover · doble clic para abrir Atic" : undefined}
    ondblclick={() => {
      if (mode === "idle") void openMain();
    }}
  >
    {#if mode === "clipboard"}
      <span class="pill-mark" aria-hidden="true"></span>
      <span class="pill-label min-w-0 flex-1">Clipboard</span>
      <button
        type="button"
        class="pill-action pill-close"
        data-no-drag
        onclick={() => void closeClipboardPanel()}
        aria-label="Cerrar historial"
        title="Cerrar (Esc)"
      >
        <X size={14} />
      </button>
    {:else if mode === "recording"}
      <button
        type="button"
        class="pill-action pill-rec is-live"
        data-no-drag
        onclick={toggleRecord}
        disabled={busy}
        aria-label="Detener grabación"
        title={btWarning ?? "Detener grabación"}
      >
        <span class="pill-rec-square" aria-hidden="true"></span>
      </button>
      <span class="pill-timer" aria-live="off">{fmt(elapsed)}</span>
      {#if liveError}
        <span class="pill-live is-error" role="status">Error</span>
      {:else if btWarning}
        <span class="pill-live is-warn" role="status" title={btWarning}>BT</span>
      {:else if liveActive}
        <span class="pill-live" role="status">En vivo</span>
      {/if}
      <div class="pill-wave">
        <Waveform mic={levels.mic} system={levels.system} bars={10} variant="quiet" />
      </div>
    {:else if mode === "dictation"}
      <button
        type="button"
        class="pill-action pill-dict"
        class:is-live={dictation === "listening"}
        class:is-busy={dictation === "transcribing"}
        class:is-ok={dictation === "pasted"}
        class:is-error={dictation === "error"}
        data-no-drag
        onclick={toggleDictate}
        disabled={busy || dictation === "transcribing"}
        aria-label={dictation === "listening" ? "Detener dictado" : "Dictado"}
        title={dictation === "listening" ? "Detener dictado" : dictationLabel(dictation)}
      >
        <span class="pill-mic" aria-hidden="true"></span>
      </button>
      <span class="pill-label min-w-0 flex-1" aria-live="polite" title={dictationLabel(dictation)}>
        {dictationLabel(dictation)}
      </span>
      {#if dictation === "listening"}
        <div class="pill-wave">
          <Waveform mic={levels.mic} system={0} bars={8} variant="quiet" />
        </div>
      {/if}
    {:else}
      <span class="pill-mark" aria-hidden="true"></span>
      <span class="pill-label">Atic</span>
    {/if}
  </div>

  {#if mode === "clipboard" && !expandUp}
    <div class="pill-panel" data-no-drag>
      <ClipboardHistoryList
        items={clipboardItems}
        loading={clipboardLoading}
        compact
        onRefresh={refreshClipboard}
        onPasteStart={() => {
          pasting = true;
        }}
        onPasted={() => {
          void closeClipboardPanel();
        }}
        onError={() => {
          pasting = false;
        }}
      />
    </div>
  {/if}
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
    flex-direction: column;
    padding: 4px;
    overflow: hidden;
    cursor: grab;
  }
  .pill-root:active {
    cursor: grabbing;
  }

  .pill-shell {
    display: flex;
    width: 100%;
    height: 40px;
    min-width: 0;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    border-radius: 999px;
    padding: 0 12px 0 10px;
    background: color-mix(in srgb, var(--rb-surface) 97%, transparent);
    color: var(--rb-text);
    box-shadow: 0 5px 14px rgba(0, 0, 0, 0.3);
    overflow: hidden;
    white-space: nowrap;
  }
  .pill-shell.is-expanded {
    border-radius: 16px 16px 0 0;
    box-shadow: none;
    padding-right: 8px;
  }
  .pill-shell.is-expanded.is-up {
    border-radius: 0 0 16px 16px;
  }

  .pill-panel {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    border-radius: 0 0 16px 16px;
    padding: 0.45rem 0.5rem 0.55rem;
    background: color-mix(in srgb, var(--rb-surface) 97%, transparent);
    color: var(--rb-text);
    box-shadow: 0 8px 18px rgba(0, 0, 0, 0.28);
    overflow: hidden;
    cursor: default;
  }
  .pill-root.is-up .pill-panel {
    border-radius: 16px 16px 0 0;
  }

  @media (prefers-color-scheme: light) {
    .pill-shell,
    .pill-panel {
      background: color-mix(in srgb, #ffffff 97%, transparent);
      box-shadow: 0 5px 14px rgba(15, 23, 32, 0.12);
    }
    .pill-shell.is-expanded {
      box-shadow: none;
    }
  }

  .pill-mark {
    width: 8px;
    height: 8px;
    flex-shrink: 0;
    border-radius: 999px;
    background: var(--rb-accent);
  }

  .pill-label {
    min-width: 0;
    overflow: hidden;
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--rb-text);
    font-family: var(--rb-font);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .pill-action {
    flex-shrink: 0;
    border: 0;
    margin: 0;
    cursor: pointer;
  }

  .pill-close {
    display: grid;
    width: 1.5rem;
    height: 1.5rem;
    place-items: center;
    border-radius: 999px;
    padding: 0;
    background: color-mix(in srgb, var(--rb-text) 10%, transparent);
    color: var(--rb-text);
  }

  .pill-rec {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border-radius: 999px;
    background: var(--rb-record);
    color: #fff;
  }
  .pill-rec.is-live {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--rb-record) 22%, transparent);
  }
  .pill-rec-square {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: #fff;
  }

  .pill-dict {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border-radius: 999px;
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 10%, transparent);
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
    min-width: 2.4rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--rb-text);
    font-variant-numeric: tabular-nums;
    font-family: var(--rb-font);
  }

  .pill-live {
    overflow: hidden;
    max-width: 3.5rem;
    font-size: 0.6875rem;
    font-weight: 600;
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

  .pill-wave {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
  }

  .pill-root,
  .pill-root * {
    user-select: none !important;
    -webkit-user-select: none !important;
  }

  .pill-shell :global(button:hover:not(:disabled)) {
    filter: brightness(1.08);
  }

  .pill-shell :global(button:active:not(:disabled)) {
    transform: scale(0.94);
  }
</style>
