<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { PhysicalPosition } from "@tauri-apps/api/dpi";
  import {
    currentMonitor,
    getCurrentWindow,
    LogicalSize,
  } from "@tauri-apps/api/window";
  import type { ClipboardItem, DictationPhase, Levels, PasteQueueItem, Snippet as TextSnippet } from "$lib/types";
  import Waveform from "$lib/Waveform.svelte";
  import X from "reicon-svelte/icons/X.svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import {
    activateCapture,
    listPasteQueue,
    pasteQueueItemNow,
    dismissPasteQueueItem,
    onPasteQueueChanged,
    onPasteQueued,
    startRecording,
    stopRecording,
    isRecording,
    toggleDictation,
    dictationPhase,
    showMainWindow,
    listClipboardHistory,
    listSnippets,
    getScratchpad,
    setScratchpad,
    onLevels,
    onStatus,
    onCaptureWarn,
    onDictationStatus,
    onLiveTranscriptError,
    onLiveTranscriptFinal,
    prepareClipboardPill,
    prepareSnippetsPill,
    restorePillPosition,
    onPillClipboardToggle,
    onPillClipboardClose,
    onPillSnippetsToggle,
    onPillSnippetsClose,
    onPillReset,
    onClipboardHistoryChanged,
    onSnippetsChanged,
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

  let snippetsOpen = $state(false);
  let snippetsTab = $state<"list" | "scratchpad">("list");
  let snippetItems = $state<TextSnippet[]>([]);
  let snippetsLoading = $state(false);
  let snippetsOpenedAt = 0;
  let scratchBody = $state("");
  let scratchLoading = $state(false);
  let scratchSaving = $state(false);
  let scratchTimer: ReturnType<typeof setTimeout> | null = null;

  let pasting = $state(false);
  let pasteQueue = $state<PasteQueueItem[]>([]);
  let pasteQueueBusy = $state(false);
  /** Evita que startDragging() cierre el clipboard por blur. */
  let windowDragging = $state(false);
  /** Clipboard siempre crece hacia arriba (barra abajo, lista arriba). */
  let expandUp = $state(false);

  let timer: ReturnType<typeof setInterval> | null = null;
  let startedAt = 0;
  let fitRaf = 0;
  let fitting = false;

  const COMPACT_H = 48;
  const QUEUE_STRIP_H = 44;
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
    if (snippetsOpen) return "snippets" as const;
    if (recording) return "recording" as const;
    if (dictating) return "dictation" as const;
    return "idle" as const;
  });

  const panelExpanded = $derived(mode === "clipboard" || mode === "snippets");
  const queueVisible = $derived(pasteQueue.length > 0 && !panelExpanded);

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
    if (panelExpanded) return { w: EXPANDED_W, h: EXPANDED_H };
    let w = IDLE_W;
    let h = COMPACT_H;
    if (mode === "recording") {
      w = liveError || btWarning || liveActive ? 248 : 210;
    } else if (mode === "dictation") {
      if (dictation === "listening") w = 210;
      else if (dictation === "transcribing") w = 200;
      else w = 220;
    }
    if (queueVisible) h += QUEUE_STRIP_H;
    return { w, h };
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
      if (!panelExpanded) expandUp = false;
    } catch (err) {
      console.warn("pill setSize", err);
    } finally {
      fitting = false;
    }
  }

  /** Expande el panel anclado al compacto actual; elige arriba/abajo según espacio. */
  async function fitPanelExpanded() {
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
      console.warn("pill panel fit", err);
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

  async function refreshPasteQueue() {
    try {
      pasteQueue = await listPasteQueue();
    } catch {
      pasteQueue = [];
    }
  }

  async function pasteQueueFront() {
    const front = pasteQueue[0];
    if (!front || pasteQueueBusy) return;
    pasteQueueBusy = true;
    try {
      await pasteQueueItemNow(front.id);
      await refreshPasteQueue();
    } catch (err) {
      console.warn("paste queue", err);
    } finally {
      pasteQueueBusy = false;
      scheduleFit();
    }
  }

  async function dismissQueueFront() {
    const front = pasteQueue[0];
    if (!front || pasteQueueBusy) return;
    pasteQueueBusy = true;
    try {
      await dismissPasteQueueItem(front.id);
      await refreshPasteQueue();
    } catch (err) {
      console.warn("dismiss queue", err);
    } finally {
      pasteQueueBusy = false;
      scheduleFit();
    }
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
    snippetsOpen = false;
    clipboardOpen = true;
    clipboardOpenedAt = Date.now();
    try {
      await getCurrentWindow().setFocus();
    } catch {
      // best-effort
    }
    await fitPanelExpanded();
    await refreshClipboard();
  }

  async function closeClipboardPanel() {
    if (!clipboardOpen) return;
    clipboardOpen = false;
    expandUp = false;
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve());
    });
    try {
      await restorePillPosition();
    } catch (err) {
      console.warn("restore pill position", err);
    }
    scheduleFit();
  }

  async function refreshSnippets() {
    snippetsLoading = true;
    try {
      snippetItems = await listSnippets();
    } catch {
      snippetItems = [];
    } finally {
      snippetsLoading = false;
    }
  }

  async function loadScratchpad() {
    scratchLoading = true;
    try {
      const pad = await getScratchpad();
      scratchBody = pad.body;
    } catch {
      scratchBody = "";
    } finally {
      scratchLoading = false;
    }
  }

  function scheduleScratchSave() {
    if (scratchTimer) clearTimeout(scratchTimer);
    scratchTimer = setTimeout(() => {
      void persistScratchpad();
    }, 500);
  }

  async function persistScratchpad() {
    if (scratchSaving) return;
    scratchSaving = true;
    try {
      await setScratchpad(scratchBody);
    } catch (err) {
      console.warn("scratchpad save", err);
    } finally {
      scratchSaving = false;
    }
  }

  async function openSnippetsPanel() {
    clipboardOpen = false;
    snippetsOpen = true;
    snippetsOpenedAt = Date.now();
    try {
      await getCurrentWindow().setFocus();
    } catch {
      // best-effort
    }
    await fitPanelExpanded();
    await Promise.all([refreshSnippets(), loadScratchpad()]);
  }

  async function closeSnippetsPanel() {
    if (!snippetsOpen) return;
    snippetsOpen = false;
    expandUp = false;
    if (scratchTimer) {
      clearTimeout(scratchTimer);
      scratchTimer = null;
    }
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => resolve());
    });
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
    snippetsOpen = false;
    expandUp = false;
    pasting = false;
    if (scratchTimer) {
      clearTimeout(scratchTimer);
      scratchTimer = null;
    }
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

  async function onSnippetsHotkey() {
    if (snippetsOpen) {
      await resetPillChrome();
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => resolve());
      });
    }
    try {
      await prepareSnippetsPill();
    } catch (err) {
      console.warn("prepare snippets pill", err);
    }
    await openSnippetsPanel();
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
    // startDragging quita el foco → no cerrar el panel por ese blur.
    windowDragging = true;
    void getCurrentWindow().startDragging();
    // Si el foco no vuelve (Windows), no dejar el flag pegado.
    window.setTimeout(() => {
      windowDragging = false;
    }, 1500);
  }

  async function toggleRecord() {
    if (busy || dictating || clipboardOpen || snippetsOpen) return;
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
    if (busy || recording || clipboardOpen || snippetsOpen) return;
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
      await refreshPasteQueue();
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
      onPillSnippetsToggle(() => {
        void onSnippetsHotkey();
      }),
      onPillSnippetsClose(() => {
        pasting = false;
        void closeSnippetsPanel();
      }),
      onPillReset(() => {
        void resetPillChrome();
      }),
      onClipboardHistoryChanged(() => {
        if (clipboardOpen) void refreshClipboard();
      }),
      onSnippetsChanged(() => {
        if (snippetsOpen) void refreshSnippets();
      }),
      onPasteQueueChanged(() => {
        void refreshPasteQueue().then(() => scheduleFit());
      }),
      onPasteQueued(() => {
        void refreshPasteQueue().then(() => scheduleFit());
      }),
    );

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && clipboardOpen) {
        event.preventDefault();
        event.stopPropagation();
        void closeClipboardPanel();
        return;
      }
      if (event.key === "Escape" && snippetsOpen) {
        event.preventDefault();
        event.stopPropagation();
        void closeSnippetsPanel();
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
      if (pasting || windowDragging) return;
      if (clipboardOpen && Date.now() - clipboardOpenedAt > 400) {
        void closeClipboardPanel();
      }
      if (snippetsOpen && Date.now() - snippetsOpenedAt > 400) {
        void closeSnippetsPanel();
      }
    };
    const onFocus = () => {
      windowDragging = false;
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);

    return () => {
      stopTimer();
      cancelAnimationFrame(fitRaf);
      if (scratchTimer) clearTimeout(scratchTimer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="pill-root"
  class:is-expanded={panelExpanded}
  class:is-up={expandUp && panelExpanded}
  onpointerdown={beginDrag}
>
  {#if queueVisible}
    <div class="paste-queue-strip" data-no-drag>
      <span class="paste-queue-badge" title="Cola de pegado">{pasteQueue.length}</span>
      <span class="paste-queue-preview" title={pasteQueue[0]?.text}>
        {pasteQueue[0]?.text ?? ""}
      </span>
      <button
        type="button"
        class="paste-queue-btn"
        disabled={pasteQueueBusy}
        onclick={() => void pasteQueueFront()}
      >
        Pegar
      </button>
      <button
        type="button"
        class="paste-queue-btn is-muted"
        disabled={pasteQueueBusy}
        onclick={() => void dismissQueueFront()}
      >
        Descartar
      </button>
    </div>
  {/if}

  {#if panelExpanded && expandUp}
    <div class="pill-panel" data-no-drag>
      {#if mode === "clipboard"}
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
      {:else}
        <div class="snip-pill-tabs" role="tablist" aria-label="Vistas de fragmentos">
          <button
            type="button"
            role="tab"
            class="snip-pill-tab"
            class:active={snippetsTab === "list"}
            aria-selected={snippetsTab === "list"}
            onclick={() => (snippetsTab = "list")}
          >
            Lista
          </button>
          <button
            type="button"
            role="tab"
            class="snip-pill-tab"
            class:active={snippetsTab === "scratchpad"}
            aria-selected={snippetsTab === "scratchpad"}
            onclick={() => (snippetsTab = "scratchpad")}
          >
            Bloc
          </button>
        </div>
        {#if snippetsTab === "list"}
          <SnippetsList
            items={snippetItems}
            loading={snippetsLoading}
            compact
            onRefresh={refreshSnippets}
            onPasteStart={() => {
              pasting = true;
            }}
            onPasted={() => {
              void closeSnippetsPanel();
            }}
            onError={() => {
              pasting = false;
            }}
          />
        {:else if scratchLoading}
          <p class="snip-pill-empty">Cargando bloc…</p>
        {:else}
          <textarea
            class="snip-pill-scratch"
            bind:value={scratchBody}
            oninput={scheduleScratchSave}
            placeholder="Notas temporales…"
            aria-label="Bloc de notas"
            data-no-drag
          ></textarea>
        {/if}
      {/if}
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="pill-shell"
    class:is-expanded={panelExpanded}
    class:is-up={expandUp && panelExpanded}
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
    {:else if mode === "snippets"}
      <span class="pill-mark" aria-hidden="true"></span>
      <span class="pill-label min-w-0 flex-1">Fragmentos</span>
      <button
        type="button"
        class="pill-action pill-close"
        data-no-drag
        onclick={() => void closeSnippetsPanel()}
        aria-label="Cerrar fragmentos"
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

  {#if panelExpanded && !expandUp}
    <div class="pill-panel" data-no-drag>
      {#if mode === "clipboard"}
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
      {:else}
        <div class="snip-pill-tabs" role="tablist" aria-label="Vistas de fragmentos">
          <button
            type="button"
            role="tab"
            class="snip-pill-tab"
            class:active={snippetsTab === "list"}
            aria-selected={snippetsTab === "list"}
            onclick={() => (snippetsTab = "list")}
          >
            Lista
          </button>
          <button
            type="button"
            role="tab"
            class="snip-pill-tab"
            class:active={snippetsTab === "scratchpad"}
            aria-selected={snippetsTab === "scratchpad"}
            onclick={() => (snippetsTab = "scratchpad")}
          >
            Bloc
          </button>
        </div>
        {#if snippetsTab === "list"}
          <SnippetsList
            items={snippetItems}
            loading={snippetsLoading}
            compact
            onRefresh={refreshSnippets}
            onPasteStart={() => {
              pasting = true;
            }}
            onPasted={() => {
              void closeSnippetsPanel();
            }}
            onError={() => {
              pasting = false;
            }}
          />
        {:else if scratchLoading}
          <p class="snip-pill-empty">Cargando bloc…</p>
        {:else}
          <textarea
            class="snip-pill-scratch"
            bind:value={scratchBody}
            oninput={scheduleScratchSave}
            placeholder="Notas temporales…"
            aria-label="Bloc de notas"
            data-no-drag
          ></textarea>
        {/if}
      {/if}
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

  .paste-queue-strip {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.35rem;
    margin-bottom: 0.25rem;
    border-radius: 12px;
    padding: 0.3rem 0.45rem;
    background: color-mix(in srgb, var(--rb-accent) 12%, var(--rb-surface));
    color: var(--rb-text);
    cursor: default;
  }

  .paste-queue-badge {
    display: inline-flex;
    min-width: 1.1rem;
    height: 1.1rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    background: var(--rb-accent);
    color: #fff;
    font-size: 0.625rem;
    font-weight: 700;
  }

  .paste-queue-preview {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    font-size: 0.6875rem;
    white-space: nowrap;
    text-overflow: ellipsis;
    opacity: 0.9;
  }

  .paste-queue-btn {
    flex-shrink: 0;
    border: 0;
    border-radius: 999px;
    padding: 0.2rem 0.45rem;
    background: var(--rb-accent);
    color: #fff;
    font-size: 0.625rem;
    font-weight: 650;
    cursor: pointer;
  }

  .paste-queue-btn.is-muted {
    background: color-mix(in srgb, var(--rb-text) 12%, transparent);
    color: var(--rb-text);
  }

  .paste-queue-btn:disabled {
    opacity: 0.55;
    cursor: default;
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

  .snip-pill-tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.3rem;
    margin-bottom: 0.35rem;
  }

  .snip-pill-tab {
    border: 1px solid color-mix(in srgb, var(--rb-text) 12%, transparent);
    border-radius: 999px;
    padding: 0.2rem 0.55rem;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 600;
    cursor: pointer;
  }

  .snip-pill-tab.active {
    border-color: color-mix(in srgb, var(--rb-accent) 40%, transparent);
    background: color-mix(in srgb, var(--rb-accent) 12%, transparent);
    color: var(--rb-accent);
  }

  .snip-pill-scratch {
    width: 100%;
    min-height: 0;
    flex: 1;
    border: 1px solid color-mix(in srgb, var(--rb-text) 10%, transparent);
    border-radius: 0.45rem;
    padding: 0.4rem 0.5rem;
    background: color-mix(in srgb, var(--rb-bg0) 80%, transparent);
    color: var(--rb-text);
    font-size: 0.75rem;
    font-family: inherit;
    resize: none;
    outline: none;
  }

  .snip-pill-empty {
    margin: 0.35rem 0 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
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
