<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type {
    AppConfig,
    DictationPhase,
    Levels,
    ModelStatus,
    Recording,
    Segment,
    MeetingDetectionPayload,
  } from "$lib/types";
  import AppShell from "$lib/AppShell.svelte";
  import AgentsTool from "$lib/AgentsTool.svelte";
  import CapturesTool from "$lib/CapturesTool.svelte";
  import ClipboardTool from "$lib/ClipboardTool.svelte";
  import SnippetsTool from "$lib/SnippetsTool.svelte";
  import ConfirmModal from "$lib/ConfirmModal.svelte";
  import DictationTool from "$lib/DictationTool.svelte";
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import OnboardingModal from "$lib/OnboardingModal.svelte";
  import RecordingDetail from "$lib/RecordingDetail.svelte";
  import RecordingList from "$lib/RecordingList.svelte";
  import RecordingToolbar from "$lib/RecordingToolbar.svelte";
  import SettingsModal from "$lib/SettingsModal.svelte";
  import SummaryModal from "$lib/SummaryModal.svelte";
  import TranscriptModal from "$lib/TranscriptModal.svelte";
  import SearchModal from "$lib/SearchModal.svelte";
  import ToolPageShell from "$lib/ToolPageShell.svelte";
  import type { SearchHit } from "$lib/types";
  import { toolById, type ToolId } from "$lib/tools";
  import {
    applyTheme,
    cycleTheme,
    normalizeTheme,
    type UiTheme,
  } from "$lib/theme";
  import { formatMegabytes } from "$lib/format";
  import { playback } from "$lib/playback.svelte";
  import {
    currentModelReady,
    deleteRecording,
    dictationPhase,
    downloadModelAndWait,
    getConfig,
    importAudio,
    isRecording,
    listModels,
    listRecordings,
    ollamaAvailable,
    onCaptureError,
    onCaptureWarn,
    onDictationStatus,
    onLevels,
    onLiveTranscriptError,
    onLiveTranscriptFinal,
    onLiveTranscriptPartial,
    onModelDownloadDone,
    onModelDownloadError,
    onModelDownloadProgress,
    onRecordingsChanged,
    onStatus,
    onSummarizeError,
    onSummaryReady,
    onTranscribeError,
    onTranscribeProgress,
    onTranscriptReady,
    renameRecording,
    secretsStatus,
    setConfig,
    startRecording,
    stopRecording,
    toggleDictation,
    transcribeRecording,
    checkAppUpdate,
    audioPreflight,
    onMeetingDetection,
    activateCapture,
    pasteClipboardItem,
    pasteSnippet,
    onPasteQueued,
    failedShortcuts,
    onShortcutsFailed,
  } from "$lib/api";

  let recordings = $state<Recording[]>([]);
  let config = $state<AppConfig | null>(null);
  let recording = $state(false);
  let elapsed = $state(0);
  let levels = $state<Levels>({ mic: 0, system: 0 });
  let busy = $state(false);
  let importing = $state(false);
  let toast = $state<string | null>(null);
  let dictation = $state<DictationPhase>("idle");
  let dictationMessage = $state<string | null>(null);
  let liveSegments = $state<Segment[]>([]);
  let livePartial = $state<Segment | null>(null);
  let liveError = $state<string | null>(null);
  let recordingNote = $state<string | null>(null);
  let detectedMeeting = $state<MeetingDetectionPayload | null>(null);
  let liveListEl = $state<HTMLElement | undefined>(undefined);

  let modelReady = $state(false);
  let models = $state<ModelStatus[]>([]);
  let downloading = $state<{ downloaded: number; total: number } | null>(null);
  let transcribeProgress = $state<Record<string, number>>({});
  let summarySetupNeeded = $state(false);

  let selectedId = $state<string | null>(null);
  let transcriptFor = $state<Recording | null>(null);
  let summaryFor = $state<Recording | null>(null);
  let deleteTarget = $state<Recording | null>(null);
  let settingsOpen = $state(false);
  let bluetoothConfirmMessage = $state<string | null>(null);
  let activeTool = $state<ToolId>("meetings");
  let shellView = $state<"hub" | "tool">("hub");
  let snippetsInitialTab = $state<"snippets" | "scratchpad">("snippets");
  let searchOpen = $state(false);
  let uiTheme = $state<UiTheme>("system");
  /** Atajos que otra app ya tenía tomados: sin esto el fallo era invisible. */
  let shortcutConflicts = $state<string[]>([]);

  const theme = $derived(normalizeTheme(config?.ui_theme ?? uiTheme));

  let timer: ReturnType<typeof setInterval> | null = null;
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let startedAt = 0;

  const selectedRecording = $derived(
    recordings.find((item) => item.id === selectedId) ?? null,
  );
  const meetingModel = $derived(
    models.find((item) => item.id === (config?.whisper_model ?? "base")),
  );
  const dictationModel = $derived(
    models.find(
      (item) => item.id === (config?.dictation_whisper_model ?? "base"),
    ),
  );
  const liveModel = $derived(
    models.find((item) => item.id === (config?.live_whisper_model ?? "small")),
  );
  const missingModels = $derived.by(() => {
    const missing: ModelStatus[] = [];
    const candidates = [dictationModel, meetingModel];
    if (config?.live_transcription) candidates.push(liveModel);
    for (const model of candidates) {
      if (
        model &&
        !model.downloaded &&
        !missing.some((candidate) => candidate.id === model.id)
      ) {
        missing.push(model);
      }
    }
    return missing;
  });
  const downloadPercent = $derived(
    downloading && downloading.total > 0
      ? Math.round((downloading.downloaded / downloading.total) * 100)
      : 0,
  );

  function showToast(message: string, durationMs = 5000) {
    if (toastTimer) clearTimeout(toastTimer);
    toast = message;
    toastTimer = setTimeout(() => (toast = null), durationMs);
  }

  async function handleSearchSelect(hit: SearchHit) {
    try {
      switch (hit.kind) {
        case "snippet":
          await pasteSnippet(hit.id);
          showToast(`Fragmento pegado: ${hit.title}`);
          break;
        case "clipboard":
          await pasteClipboardItem(hit.id);
          showToast("Pegado desde el portapapeles");
          break;
        case "capture":
          await activateCapture(hit.id);
          showToast("Captura abierta");
          break;
        case "scratchpad":
          snippetsInitialTab = "scratchpad";
          activeTool = "snippets";
          shellView = "tool";
          showToast("Bloc de notas");
          break;
        case "recording":
          activeTool = "meetings";
          shellView = "tool";
          selectedId = hit.id;
          showToast(hit.title);
          break;
      }
    } catch (error) {
      showToast(String(error));
    }
  }

  function openSearch() {
    searchOpen = true;
  }

  function startTimer() {
    if (timer) clearInterval(timer);
    startedAt = Date.now();
    elapsed = 0;
    timer = setInterval(() => {
      elapsed = Math.floor((Date.now() - startedAt) / 1000);
    }, 500);
  }

  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = null;
  }

  async function refresh() {
    const next = await listRecordings();
    recordings = next;
    if (!selectedId || !next.some((item) => item.id === selectedId)) {
      selectedId = next[0]?.id ?? null;
    }
  }

  async function loadModelState() {
    [modelReady, models] = await Promise.all([
      currentModelReady(),
      listModels(),
    ]);
  }

  async function loadSummarySetup() {
    if (!config) return;
    if (config.summary_backend === "ollama") {
      summarySetupNeeded = !(await ollamaAvailable());
      return;
    }
    const status = await secretsStatus();
    summarySetupNeeded = !Boolean(status.providers?.[config.summary_backend]);
  }

  async function toggleRecording() {
    if (busy) return;
    busy = true;
    try {
      if (recording) {
        await stopRecording();
      } else {
        const preflight = await audioPreflight();
        if (preflight.risk === "bluetooth_hands_free") {
          bluetoothConfirmMessage =
            preflight.message ??
            "El micrófono Bluetooth activará el perfil Hands-Free y reducirá la calidad.";
          return;
        }
        await startRecording();
      }
    } catch (error) {
      const message = String(error).replace(
        /^BLUETOOTH_CONFIRM_REQUIRED:/,
        "",
      );
      showToast(message);
    } finally {
      busy = false;
    }
  }

  function captureLiveList(node: HTMLElement) {
    liveListEl = node;
    return () => {
      if (liveListEl === node) liveListEl = undefined;
    };
  }

  async function confirmBluetoothRecording() {
    bluetoothConfirmMessage = null;
    busy = true;
    try {
      await startRecording(true);
    } catch (error) {
      showToast(String(error));
    } finally {
      busy = false;
    }
  }

  async function toggleTextDictation() {
    if (busy || recording || importing || dictation === "transcribing") return;
    busy = true;
    try {
      await toggleDictation();
    } catch (error) {
      showToast(String(error));
    } finally {
      busy = false;
    }
  }

  async function importExternalAudio() {
    if (
      busy ||
      recording ||
      importing ||
      dictation === "listening" ||
      dictation === "transcribing" ||
      dictation === "pasted" ||
      dictation === "error"
    ) {
      return;
    }
    importing = true;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: true,
        title: "Importar audio",
        filters: [
          { name: "Audio", extensions: ["wav", "mp3", "m4a"] },
        ],
      });
      if (selected === null) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length === 0) return;
      const imported = await importAudio(paths);
      await refresh();
      if (imported[0]) selectedId = imported[0].id;
      showToast(
        imported.length === 1
          ? `Importado: ${imported[0].title}`
          : `Importados ${imported.length} archivos`,
      );
    } catch (error) {
      showToast(String(error));
    } finally {
      importing = false;
    }
  }

  async function startDownload() {
    if (!config || downloading) return;
    const missing = missingModels;
    if (missing.length === 0) {
      await loadModelState();
      return;
    }
    for (const m of missing) {
      downloading = { downloaded: 0, total: 1 };
      try {
        await downloadModelAndWait(m.id);
        await loadModelState();
      } catch (error) {
        downloading = null;
        showToast(String(error));
        return;
      }
    }
    downloading = null;
  }

  async function transcribe(item: Recording) {
    try {
      transcribeProgress = { ...transcribeProgress, [item.id]: 0 };
      await transcribeRecording(item.id);
    } catch (error) {
      const { [item.id]: _removed, ...rest } = transcribeProgress;
      transcribeProgress = rest;
      showToast(String(error));
    }
  }

  async function renameSelected(title: string) {
    if (!selectedRecording) return;
    try {
      await renameRecording(selectedRecording.id, title);
      await refresh();
    } catch (error) {
      showToast(String(error));
    }
  }

  async function confirmDelete() {
    const item = deleteTarget;
    if (!item) return;
    deleteTarget = null;
    try {
      if (playback.recordingId === item.id) playback.stop();
      await deleteRecording(item.id);
      await refresh();
    } catch (error) {
      showToast(String(error));
    }
  }

  async function completeOnboarding(next: AppConfig) {
    config = next;
    try {
      await setConfig(next);
      await loadModelState();
      void loadSummarySetup().catch((error) => showToast(String(error)));
      showToast("Listo. Puedes grabar cuando quieras.");
    } catch (error) {
      showToast(String(error));
    }
  }

  /** Parchea config y re-registra atajos al instante. */
  async function patchConfig(patch: Partial<AppConfig>) {
    if (!config) return;
    const previous = config;
    const next = { ...config, ...patch };
    config = next;
    try {
      await setConfig(next);
    } catch (error) {
      config = previous;
      showToast(String(error));
    }
  }

  function toggleTheme() {
    const next = cycleTheme(theme);
    uiTheme = next;
    applyTheme(next);
    void patchConfig({ ui_theme: next });
  }

  $effect(() => {
    const next = normalizeTheme(config?.ui_theme ?? uiTheme);
    uiTheme = next;
    applyTheme(next);
  });

  $effect(() => {
    if (theme !== "system" || typeof window === "undefined") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  });

  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    (async () => {
      try {
        const [nextConfig, activeRecording] = await Promise.all([
          getConfig(),
          isRecording(),
        ]);
        config = nextConfig;
        recording = activeRecording;
        try {
          dictation = await dictationPhase();
        } catch {
          dictation = "idle";
        }
        if (recording) startTimer();
        await Promise.all([refresh(), loadModelState()]);
        // Un llavero bloqueado o un proveedor no disponible no debe retrasar
        // que el usuario pueda ver sus grabaciones o iniciar una nueva.
        void loadSummarySetup().catch((error) => showToast(String(error)));
        // Chequeo no intrusivo: solo avisa si hay update; errores se ignoran
        // (pubkey placeholder, sin red, sin release, etc.).
        void checkAppUpdate()
          .then((update) => {
            if (update) {
              showToast(
                `Hay una actualización (${update.version}). Ábrela en Ajustes.`,
              );
            }
          })
          .catch(() => {});
      } catch (error) {
        showToast(`No se pudo iniciar la app: ${String(error)}`);
      }
    })();

    unlisteners.push(
      onStatus((status) => {
        recording = status.active;
        if (status.active) {
          startTimer();
          liveSegments = [];
          livePartial = null;
          liveError = null;
        } else {
          stopTimer();
          livePartial = null;
          recordingNote = null;
          void refresh();
        }
      }),
      onLevels((next) => (levels = next)),
      onLiveTranscriptFinal((segment) => {
        liveSegments = [...liveSegments, segment];
        livePartial = null;
        queueMicrotask(() => {
          liveListEl?.scrollTo({ top: liveListEl.scrollHeight, behavior: "smooth" });
        });
      }),
      onLiveTranscriptPartial((segment) => {
        livePartial = segment;
        queueMicrotask(() => {
          liveListEl?.scrollTo({ top: liveListEl.scrollHeight, behavior: "smooth" });
        });
      }),
      onLiveTranscriptError((message) => {
        liveError = message;
        showToast(message);
      }),
      onRecordingsChanged(() => void refresh()),
      onCaptureError(showToast),
      onCaptureWarn((message) => {
        recordingNote = message;
        showToast(message, 9000);
      }),
      onMeetingDetection((meeting) => {
        detectedMeeting = meeting.active ? meeting : null;
      }),
      onDictationStatus((status) => {
        dictation = status.phase;
        dictationMessage = status.message;
      }),
      onModelDownloadProgress((progress) => {
        downloading = {
          downloaded: progress.downloaded,
          total: progress.total,
        };
      }),
      onModelDownloadDone(() => {
        downloading = null;
        void loadModelState();
      }),
      onModelDownloadError((_id, message) => {
        downloading = null;
        showToast(message);
      }),
      onTranscribeProgress((progress) => {
        transcribeProgress = {
          ...transcribeProgress,
          [progress.id]: progress.progress,
        };
      }),
      onTranscriptReady((id) => {
        const { [id]: _removed, ...rest } = transcribeProgress;
        transcribeProgress = rest;
        void refresh();
      }),
      onTranscribeError((id, message) => {
        const { [id]: _removed, ...rest } = transcribeProgress;
        transcribeProgress = rest;
        showToast(message);
        void refresh();
      }),
      onSummaryReady(() => void refresh()),
      onSummarizeError((_id, message) => {
        showToast(message);
        void refresh();
      }),
      onPasteQueued((preview) => {
        showToast(`En cola para pegar: ${preview}`, 6000);
      }),
      onShortcutsFailed((names) => (shortcutConflicts = names)),
    );

    // Estado inicial: los atajos se registran en el setup, antes de que esta
    // ventana escuche el evento.
    void failedShortcuts()
      .then((names) => (shortcutConflicts = names))
      .catch(() => {});

    const onSearchKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      if (mod && event.key.toLowerCase() === "k") {
        event.preventDefault();
        openSearch();
      }
    };
    window.addEventListener("keydown", onSearchKey);

    return () => {
      stopTimer();
      if (toastTimer) clearTimeout(toastTimer);
      window.removeEventListener("keydown", onSearchKey);
      unlisteners.forEach((listener) => listener.then((unlisten) => unlisten()));
    };
  });
</script>

<AppShell
  bind:activeTool
  bind:view={shellView}
  radialShortcut={config?.pill_radial_shortcut ?? ""}
  theme={theme}
  onToggleTheme={toggleTheme}
  onOpenSettings={() => (settingsOpen = true)}
>
  <div class="rb-app">
    <main id="main-content" class="rb-main" tabindex="-1">
      {#if shortcutConflicts.length > 0}
        <section class="rb-setup-notice" aria-label="Atajos en conflicto">
          <div class="min-w-0 flex-1">
            <strong>
              {shortcutConflicts.length === 1
                ? "Un atajo no quedó activo"
                : "Hay atajos que no quedaron activos"}
            </strong>
            <p>
              Otra app ya tiene tomado el atajo de {shortcutConflicts.join(
                ", ",
              )}. Asígnale otra combinación para poder usarlo.
            </p>
          </div>
          <button
            class="rb-btn rb-btn-soft"
            onclick={() => (settingsOpen = true)}
          >
            Cambiar atajos
          </button>
        </section>
      {/if}

      {#if activeTool === "meetings"}
        <ToolPageShell tool={toolById("meetings")} dataDir="recordings">
          {#snippet meta()}
            {#if modelReady}
              <span class="rb-local-status">Local listo</span>
            {/if}
          {/snippet}

          {#snippet prefs()}
            <div class="atic-shortcut-row">
              <div>
                <p class="atic-shortcut-label">Atajo de grabación</p>
                <p class="atic-shortcut-hint">Desde cualquier app.</p>
              </div>
              <HotkeyCapture
                value={config?.global_shortcut ?? "CmdOrCtrl+Shift+R"}
                defaultValue="CmdOrCtrl+Shift+R"
                ariaLabel="Cambiar atajo de grabación"
                onChange={(sc) => patchConfig({ global_shortcut: sc })}
              />
            </div>
          {/snippet}

        {#if !modelReady}
          <section class="rb-setup-notice" aria-label="Configurar transcripción">
            {#if downloading}
              <div class="min-w-0 flex-1">
                <div class="rb-notice-heading">
                  <strong>Descargando modelos</strong>
                  <span>{downloadPercent}%</span>
                </div>
                <div class="rb-progress-track">
                  <i style="width: {downloadPercent}%"></i>
                </div>
              </div>
            {:else}
              <div class="min-w-0 flex-1">
                <strong>Prepara la transcripción local</strong>
                <p>
                  Falta descargar {missingModels
                    .map((m) => m.display_name)
                    .join(" y ") || "el modelo local"}. El audio se procesa en
                  este equipo.
                </p>
              </div>
              <button class="rb-btn rb-btn-primary" onclick={startDownload}>
                Descargar{missingModels.length
                  ? ` · ${formatMegabytes(
                      missingModels.reduce((s, m) => s + m.approx_size_bytes, 0),
                    )}`
                  : ""}
              </button>
            {/if}
          </section>
        {/if}

        {#if summarySetupNeeded}
          <section
            class="rb-setup-notice rb-setup-info"
            aria-label="Configurar resumen"
          >
            <div class="min-w-0 flex-1">
              <strong>Configura el proveedor de resúmenes</strong>
              <p>
                {config?.summary_backend === "ollama"
                  ? "Ollama no está disponible."
                  : "Falta la API key del proveedor seleccionado."}
              </p>
            </div>
            <button
              class="rb-btn rb-btn-soft"
              onclick={() => (settingsOpen = true)}
            >
              Abrir ajustes
            </button>
          </section>
        {/if}

        {#if detectedMeeting && !recording}
          <section
            class="rb-setup-notice rb-setup-info"
            aria-label="Reunión detectada"
          >
            <div class="min-w-0 flex-1">
              <strong>
                Reunión detectada en {detectedMeeting.provider ??
                  "otra aplicación"}
              </strong>
              <p class="rb-detected-title">
                {detectedMeeting.title ?? "Ventana de reunión activa"}
              </p>
            </div>
            <button
              class="rb-btn rb-btn-primary"
              onclick={toggleRecording}
              disabled={busy}
            >
              Iniciar grabación
            </button>
          </section>
        {/if}

        <RecordingToolbar
          {recording}
          {elapsed}
          {levels}
          {busy}
          {importing}
          {dictation}
          {dictationMessage}
          liveActive={recording && (config?.live_transcription ?? false)}
          liveError={liveError}
          recordingNote={recordingNote}
          onToggle={toggleRecording}
          onToggleDictation={toggleTextDictation}
          onImport={importExternalAudio}
        />

        {#if recording && (liveSegments.length > 0 || livePartial || liveError)}
          <section class="rb-live-panel" aria-label="Transcripción en vivo">
            <div class="rb-live-panel-head">
              <strong>En vivo</strong>
              {#if liveError}
                <span class="rb-chip rb-chip-warn">Error live</span>
              {:else}
                <span class="rb-chip rb-chip-ok">Transcribiendo</span>
              {/if}
            </div>
            <ul class="rb-live-list" {@attach captureLiveList}>
              {#each liveSegments as segment, index (`${segment.speaker}-${segment.start_ms}-${index}`)}
                <li>
                  <span
                    class="rb-live-speaker"
                    class:is-me={segment.speaker === "me"}
                  >
                    {segment.speaker === "me" ? "Yo" : "Otros"}
                  </span>
                  <span class="rb-live-text">{segment.text}</span>
                </li>
              {/each}
              {#if livePartial}
                <li class="is-partial">
                  <span
                    class="rb-live-speaker"
                    class:is-me={livePartial.speaker === "me"}
                  >
                    {livePartial.speaker === "me" ? "Yo" : "Otros"}
                  </span>
                  <span class="rb-live-text">{livePartial.text}</span>
                </li>
              {/if}
            </ul>
          </section>
        {/if}

        <div class="rb-workspace">
          <RecordingList
            {recordings}
            {selectedId}
            progress={transcribeProgress}
            onSelect={(item) => (selectedId = item.id)}
            recordShortcut={config?.global_shortcut ?? ""}
          />
          <RecordingDetail
            recording={selectedRecording}
            {modelReady}
            modelName={meetingModel?.display_name}
            progress={selectedRecording
              ? transcribeProgress[selectedRecording.id]
              : undefined}
            onRename={renameSelected}
            onTranscribe={() =>
              selectedRecording ? transcribe(selectedRecording) : undefined}
            onOpenTranscript={() => {
              if (selectedRecording) transcriptFor = selectedRecording;
            }}
            onOpenSummary={() => {
              if (selectedRecording) summaryFor = selectedRecording;
            }}
            onDelete={() => {
              if (selectedRecording) deleteTarget = selectedRecording;
            }}
          />
        </div>
        </ToolPageShell>
      {:else if activeTool === "dictation"}
        <DictationTool
          phase={dictation}
          message={dictationMessage}
          shortcut={config?.dictation_shortcut ?? ""}
          pillShortcut={config?.summon_pill_shortcut ?? ""}
          mode={config?.dictation_mode ?? "toggle"}
          {busy}
          onToggle={toggleTextDictation}
          onShortcutChange={(sc) => patchConfig({ dictation_shortcut: sc })}
          onPillShortcutChange={(sc) =>
            patchConfig({ summon_pill_shortcut: sc })
          }
          onModeChange={(mode) => patchConfig({ dictation_mode: mode })}
          onOpenSettings={() => (settingsOpen = true)}
        />
      {:else if activeTool === "clipboard"}
        <ClipboardTool
          shortcut={config?.clipboard_shortcut ?? "CmdOrCtrl+Shift+V"}
          onShortcutChange={(sc) => patchConfig({ clipboard_shortcut: sc })}
          onToast={showToast}
        />
      {:else if activeTool === "snippets"}
        <SnippetsTool
          shortcut={config?.snippets_shortcut ?? "CmdOrCtrl+Shift+S"}
          initialTab={snippetsInitialTab}
          onShortcutChange={(sc) => patchConfig({ snippets_shortcut: sc })}
          onToast={showToast}
        />
      {:else if activeTool === "agents"}
        <ToolPageShell tool={toolById("agents")}>
          <AgentsTool />
        </ToolPageShell>
      {:else}
        <CapturesTool
          shortcut={config?.screenshot_shortcut ?? ""}
          onToast={showToast}
          onShortcutChange={(sc) => patchConfig({ screenshot_shortcut: sc })}
          onOpenSettings={() => (settingsOpen = true)}
        />
      {/if}
    </main>

    {#if toast}
      <div class="rb-toast" role="status" aria-live="polite">
        {toast}
      </div>
    {/if}

    {#if transcriptFor}
      <TranscriptModal
        recording={transcriptFor}
        modelReady={modelReady}
        onRetranscribe={() => {
          if (transcriptFor) void transcribe(transcriptFor);
        }}
        onClose={() => (transcriptFor = null)}
      />
    {/if}

    {#if summaryFor}
      <SummaryModal
        recording={summaryFor}
        onClose={() => (summaryFor = null)}
        onToast={showToast}
        onNeedSetup={() => {
          summaryFor = null;
          settingsOpen = true;
          void loadSummarySetup();
        }}
      />
    {/if}

    {#if settingsOpen}
      <SettingsModal
        onClose={() => (settingsOpen = false)}
        onSaved={(next) => {
          config = next;
          void loadModelState().catch((error) => showToast(String(error)));
          void loadSummarySetup().catch((error) => showToast(String(error)));
        }}
        onToast={showToast}
      />
    {/if}

    <SearchModal
      bind:open={searchOpen}
      onSelect={handleSearchSelect}
      onClose={() => (searchOpen = false)}
    />

    {#if config && !config.onboarding_done}
      <OnboardingModal config={config} onDone={completeOnboarding} />
    {/if}

    {#if deleteTarget}
      <ConfirmModal
        title="Eliminar grabación"
        message={`¿Eliminar «${deleteTarget.title}»? También se borrarán el audio, la transcripción y el resumen.`}
        confirmLabel="Eliminar"
        onConfirm={confirmDelete}
        onCancel={() => (deleteTarget = null)}
      />
    {/if}

    {#if bluetoothConfirmMessage}
      <ConfirmModal
        title="El Bluetooth bajará de calidad"
        message={bluetoothConfirmMessage}
        confirmLabel="Grabar de todos modos"
        cancelLabel="Cancelar"
        danger={false}
        onConfirm={confirmBluetoothRecording}
        onCancel={() => (bluetoothConfirmMessage = null)}
      />
    {/if}
  </div>
</AppShell>

<style>
  .rb-local-status {
    display: inline-flex;
    align-items: center;
    min-height: 1.75rem;
    padding: 0.2rem 0.45rem;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-accent);
    background: transparent;
    font-size: 0.6875rem;
    font-weight: 600;
  }

  .rb-main {
    display: flex;
    width: 100%;
    min-height: 100%;
    flex-direction: column;
    gap: 0.75rem;
    padding: 0.85rem 1rem 1rem;
  }

  .rb-main:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--rb-accent) 72%, transparent);
    outline-offset: -2px;
  }
  /* Avisos discretos: son estados temporales de configuración, no deben
     pesar más que la acción principal de la herramienta. El color queda en
     el texto, no en un bloque de relleno. */
  .rb-setup-notice {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.6rem 0.875rem;
    border: 0;
    border-radius: var(--rb-radius);
    background: color-mix(in srgb, var(--rb-warn) 8%, transparent);
  }
  .rb-setup-notice strong {
    color: color-mix(in srgb, var(--rb-warn) 72%, var(--rb-text));
  }
  .rb-setup-info {
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
  }
  .rb-setup-info strong {
    color: var(--rb-text);
  }
  .rb-setup-notice strong {
    color: var(--rb-text);
    font-size: 0.75rem;
    font-weight: 600;
  }
  .rb-setup-notice p {
    margin-top: 0.15rem;
    color: var(--rb-muted);
    font-size: 0.6875rem;
  }
  .rb-detected-title {
    max-width: 70ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rb-notice-heading {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
  }
  .rb-notice-heading span {
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-variant-numeric: tabular-nums;
  }
  .rb-progress-track {
    height: 3px;
    margin-top: 0.5rem;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rb-text) 9%, transparent);
  }
  .rb-progress-track i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--rb-accent);
  }
  /* Narrow-first: el rail fija ~4.5rem; a minWidth 640 el panel ~35.5rem. */
  .rb-workspace {
    display: grid;
    min-height: 22rem;
    grid-template-columns: 1fr;
    gap: 0.75rem;
    overflow: visible;
    border-radius: var(--rb-radius);
    background: transparent;
  }

  @container atic-main (min-width: 37rem) {
    .rb-workspace {
      grid-template-columns: minmax(14rem, 0.85fr) minmax(0, 1.35fr);
      gap: 0;
      overflow: hidden;
      background: var(--rb-surface);
    }
  }
  .rb-live-panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-height: 12rem;
    padding: 0.75rem 0.875rem;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }
  .rb-live-panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }
  .rb-live-panel-head strong {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--rb-text);
  }
  .rb-live-list {
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    min-height: 0;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .rb-live-list li {
    display: grid;
    grid-template-columns: 3.25rem minmax(0, 1fr);
    gap: 0.5rem;
    font-size: 0.8125rem;
    line-height: 1.45;
  }
  .rb-live-list li.is-partial {
    opacity: 0.72;
  }
  .rb-live-speaker {
    color: var(--rb-sys);
    font-size: 0.6875rem;
    font-weight: 600;
  }
  .rb-live-speaker.is-me {
    color: var(--rb-mic);
  }
  .rb-live-text {
    color: var(--rb-text);
  }
  .rb-toast {
    position: fixed;
    z-index: 80;
    right: 1rem;
    bottom: 1rem;
    max-width: min(24rem, calc(100vw - 2rem));
    padding: 0.65rem 0.8rem;
    border: 0;
    border-radius: var(--rb-radius-sm);
    color: var(--rb-text);
    background: var(--rb-surface-2);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.26);
    font-size: 0.75rem;
  }
  @container atic-main (max-width: 36.999rem) {
    .rb-main {
      gap: 0.625rem;
      padding: 0.7rem 0.7rem max(0.75rem, env(safe-area-inset-bottom));
    }

    .rb-setup-notice {
      align-items: stretch;
      flex-direction: column;
      padding: 0.75rem;
    }

    .rb-setup-notice .rb-btn {
      width: 100%;
    }

    .rb-local-status {
      display: none;
    }

    .rb-live-list li {
      grid-template-columns: 3rem minmax(0, 1fr);
      font-size: 0.875rem;
    }

    .rb-toast {
      right: 0.75rem;
      bottom: 0.75rem;
      max-width: calc(100vw - 1.5rem);
      font-size: 0.8125rem;
    }
  }
</style>
