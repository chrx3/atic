import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import type {
  Recording,
  AppConfig,
  Levels,
  StatusPayload,
  ModelStatus,
  Transcript,
  DownloadProgress,
  TranscribeProgress,
  Summary,
  TemplateInfo,
  SummaryProvider,
  SecretsStatus,
  SendMailResult,
  DictationPhase,
  DictationStatusPayload,
  InputDeviceInfo,
  Segment,
  AudioPreflight,
  AudioTestResult,
  ExportResult,
  RetentionPreview,
  RetentionCleanupResult,
  MeetingDetectionPayload,
  CaptureItem,
  ClipboardItem,
  Snippet,
  Scratchpad,
  PasteQueueItem,
  SearchHit,
  OverlayInfo,
} from "./types";

export type { Update as AppUpdate, DownloadEvent as UpdateDownloadEvent };

// --- Comandos ---
export const startRecording = (allowBluetoothHandsFree = false) =>
  invoke<Recording>("start_recording", { allowBluetoothHandsFree });
export const stopRecording = () => invoke<Recording>("stop_recording");
export const isRecording = () => invoke<boolean>("is_recording");
export const listRecordings = () => invoke<Recording[]>("list_recordings");
export const deleteRecording = (id: string) =>
  invoke<void>("delete_recording", { id });
export const renameRecording = (id: string, title: string) =>
  invoke<void>("rename_recording", { id, title });
export const getConfig = () => invoke<AppConfig>("get_config");
export const setConfig = (config: AppConfig) =>
  invoke<void>("set_config", { config });
export const setPillVisible = (visible: boolean) =>
  invoke<void>("set_pill_visible", { visible });
export const showMainWindow = () => invoke<void>("show_main_window");
export const capturePrimaryMonitor = () =>
  invoke<string>("capture_primary_monitor");
export const listRecentCaptures = () =>
  invoke<CaptureItem[]>("list_recent_captures");
export const deleteCapture = (path: string) =>
  invoke<void>("delete_capture", { path });
export const copyCapturePath = (path: string) =>
  invoke<void>("copy_capture_path", { path });
export const copyCaptureImage = (path: string) =>
  invoke<void>("copy_capture_image", { path });
export const revealCapture = (path: string) =>
  invoke<void>("reveal_capture", { path });
export const activateCapture = (path: string) =>
  invoke<void>("activate_capture", { path });
export const cleanupCapturesNow = () =>
  invoke<number>("cleanup_captures_now");
export type DataDirKind =
  | "recordings"
  | "clipboard"
  | "snippets"
  | "captures"
  | "data";

export const openDataDir = (kind: DataDirKind) =>
  invoke<void>("open_data_dir", { kind });
/** @deprecated Prefer `openDataDir("captures")`. */
export const openCapturesDir = () => openDataDir("captures");

// --- Historial de clipboard ---
export const listClipboardHistory = () =>
  invoke<ClipboardItem[]>("list_clipboard_history");
export const pasteClipboardItem = (id: string) =>
  invoke<void>("paste_clipboard_item", { id });
export const pinClipboardItem = (id: string, pinned: boolean) =>
  invoke<void>("pin_clipboard_item", { id, pinned });
export const deleteClipboardItem = (id: string) =>
  invoke<void>("delete_clipboard_item", { id });
/**
 * `fly`: acercar la pill al cursor (atajo global) o expandir donde está.
 * Devuelve los ms que dura el vuelo (0 si no vuela): hay que esperarlos antes
 * de expandir el panel, o el reencuadre se ancla a mitad del recorrido.
 */
export const prepareClipboardPill = (fly: boolean) =>
  invoke<number>("prepare_clipboard_pill", { fly });

/** Guarda el hogar de la pill sin moverla. Llamar ANTES de redimensionar: si
 *  no, se guarda la posición ya desplazada por el pivote y la pill deriva. */
export const stashPillHome = () => invoke<void>("stash_pill_home");

/** Encoge y vuelve al hogar en un solo movimiento. `false` = no había hogar. */
export const morphPillHome = (width: number, height: number) =>
  invoke<boolean>("morph_pill_home", { width, height });
export const restorePillPosition = () =>
  invoke<boolean>("restore_pill_position");

/** Vuela la pill al cursor y fija ahí su hogar. Se llama al terminar de
 *  colapsar: antes de eso el ancla se calcularía con el tamaño del panel. */
export const summonPillHere = () => invoke<void>("summon_pill_here");

/** Manda una traza al log de Rust, para verla junto a las de geometría. */
export const pillTrace = (msg: string) => invoke<void>("pill_trace", { msg });

export const onClipboardHistoryChanged = (
  cb: () => void,
): Promise<UnlistenFn> => listen("clipboard-history-changed", () => cb());

export const onPillClipboardToggle = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-clipboard-toggle", () => cb());

export const onPillClipboardClose = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-clipboard-close", () => cb());

// --- Fragmentos y bloc ---
export const listSnippets = () => invoke<Snippet[]>("list_snippets");
export const upsertSnippet = (snippet: Snippet) =>
  invoke<Snippet>("upsert_snippet", { snippet });
export const deleteSnippet = (id: string) =>
  invoke<void>("delete_snippet", { id });
export const pasteSnippet = (id: string) =>
  invoke<void>("paste_snippet", { id });
/** Igual que `prepareClipboardPill`: devuelve los ms de vuelo a esperar. */
export const prepareSnippetsPill = (fly: boolean) =>
  invoke<number>("prepare_snippets_pill", { fly });
export const getScratchpad = () => invoke<Scratchpad>("get_scratchpad");
export const setScratchpad = (body: string) =>
  invoke<Scratchpad>("set_scratchpad", { body });

export const onSnippetsChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("snippets-changed", () => cb());

export const onPillSnippetsToggle = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-snippets-toggle", () => cb());

export const onPillSnippetsClose = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-snippets-close", () => cb());

/** Traer pill / reset: cierra clipboard y vuelve a estado compacto. */
export const onPillReset = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-reset", () => cb());

/** Rueda de la pill: la tecla se mantiene presionada (abre). */
export const onPillRadialPress = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-radial-press", () => cb());

/** Rueda de la pill: la tecla se soltó (activa la selección y cierra). */
export const onPillRadialRelease = (cb: () => void): Promise<UnlistenFn> =>
  listen("pill-radial-release", () => cb());

// --- Atajos globales rechazados por el SO ---
/** Nombres de los atajos que otra app ya tenía tomados. */
export const failedShortcuts = () => invoke<string[]>("failed_shortcuts");

/** Se emite en cada registro, también vacío (permite limpiar el aviso). */
export const onShortcutsFailed = (
  cb: (names: string[]) => void,
): Promise<UnlistenFn> =>
  listen<string[]>("shortcuts-failed", (e) => cb(e.payload));

// --- Cola de pegado ---
export const listPasteQueue = () =>
  invoke<PasteQueueItem[]>("list_paste_queue");
export const enqueuePaste = (text: string) =>
  invoke<PasteQueueItem>("enqueue_paste", { text });
export const dismissPasteQueueItem = (id: string) =>
  invoke<void>("dismiss_paste_queue_item", { id });
export const clearPasteQueue = () => invoke<void>("clear_paste_queue");
export const pasteQueueItemNow = (id: string) =>
  invoke<void>("paste_queue_item_now", { id });
export const pasteQueueFlushReady = () =>
  invoke<boolean>("paste_queue_flush_ready");

export const onPasteQueueChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("paste-queue-changed", () => cb());

export const onPasteQueued = (
  cb: (preview: string) => void,
): Promise<UnlistenFn> =>
  listen<{ preview: string }>("paste-queued", (e) => cb(e.payload.preview));

// --- OCR de capturas ---
export const ocrCaptureText = (path: string) =>
  invoke<string>("ocr_capture_text", { path });
export const ocrCaptureAndCopy = (path: string) =>
  invoke<string>("ocr_capture_and_copy", { path });
export const readCaptureOcrCache = (path: string) =>
  invoke<string | null>("read_capture_ocr_cache", { path });

// --- Búsqueda local ---
export const searchLocal = (query: string) =>
  invoke<SearchHit[]>("search_local", { query });

// --- Overlay de selección de captura ---
export const startCaptureSession = () =>
  invoke<void>("start_capture_session");
export const overlayInfo = () => invoke<OverlayInfo>("overlay_info");
export const completeWindowCapture = (hwnd: number) =>
  invoke<string>("complete_window_capture", { hwnd });
export const completeRegionCapture = (
  left: number,
  top: number,
  width: number,
  height: number,
) =>
  invoke<string>("complete_region_capture", { left, top, width, height });
export const completeMonitorCapture = (x: number, y: number) =>
  invoke<string>("complete_monitor_capture", { x, y });
export const cancelCaptureSession = () =>
  invoke<void>("cancel_capture_session");
export const toggleDictation = () => invoke<void>("toggle_dictation");
export const dictationPhase = () => invoke<DictationPhase>("dictation_phase");
export const listInputDevices = () =>
  invoke<InputDeviceInfo[]>("list_input_devices");
export const listOutputDevices = () =>
  invoke<InputDeviceInfo[]>("list_output_devices");
/** Diagnóstico: lista cruda de endpoints que ve cpal (para consola/logs). */
export const debugListAudioDevices = () =>
  invoke<string>("debug_list_audio_devices");
export const audioPreflight = () =>
  invoke<AudioPreflight>("audio_preflight");
export const testAudio = (config: AppConfig) =>
  invoke<AudioTestResult>("test_audio", { config });
export const previewRetention = (days: number) =>
  invoke<RetentionPreview>("retention_preview", { days });
export const cleanupRetention = (days: number) =>
  invoke<RetentionCleanupResult>("cleanup_retention", {
    days,
    confirm: true,
  });

/** Importa archivos de audio externos como grabaciones (una pista mic.wav). */
export const importAudio = (paths: string[]) =>
  invoke<Recording[]>("import_audio", { paths });

const trackPath = (id: string, track: "mic" | "system") =>
  invoke<string>("recording_track_path", { id, track });

/** Devuelve una URL reproducible (asset://) para una pista de audio. */
export async function trackSrc(
  id: string,
  track: "mic" | "system",
): Promise<string> {
  return convertFileSrc(await trackPath(id, track));
}

// --- Eventos ---
export const onLevels = (cb: (levels: Levels) => void): Promise<UnlistenFn> =>
  listen<Levels>("audio-levels", (e) => cb(e.payload));

export const onStatus = (
  cb: (status: StatusPayload) => void,
): Promise<UnlistenFn> =>
  listen<StatusPayload>("recording-status", (e) => cb(e.payload));

export const onRecordingsChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("recordings-changed", () => cb());

export const onScreenshotCreated = (
  cb: (item: CaptureItem) => void,
): Promise<UnlistenFn> =>
  listen<CaptureItem>("screenshot-created", (e) => cb(e.payload));

export const onScreenshotShelfUpdated = (cb: () => void): Promise<UnlistenFn> =>
  listen("screenshot-shelf-updated", () => cb());

export const onCaptureError = (
  cb: (message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ message: string }>("capture-error", (e) => cb(e.payload.message));

/** Aviso no fatal al iniciar captura (p. ej. degradación Bluetooth). */
export const onCaptureWarn = (
  cb: (message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ message: string }>("capture-warn", (e) => cb(e.payload.message));

export const onMeetingDetection = (
  cb: (meeting: MeetingDetectionPayload) => void,
): Promise<UnlistenFn> =>
  listen<MeetingDetectionPayload>("meeting-detection", (event) =>
    cb(event.payload),
  );

export const onDictationStatus = (
  cb: (status: DictationStatusPayload) => void,
): Promise<UnlistenFn> =>
  listen<DictationStatusPayload>("dictation-status", (e) => cb(e.payload));

// --- Transcripción (fase 2) ---
export const listModels = () => invoke<ModelStatus[]>("list_models");
export const currentModelReady = () => invoke<boolean>("current_model_ready");
export const downloadModel = (id: string) =>
  invoke<void>("download_model", { id });

/** Lanza la descarga y resuelve cuando el backend emite done/error para ese id. */
export async function downloadModelAndWait(id: string): Promise<void> {
  let unDone: UnlistenFn | undefined;
  let unErr: UnlistenFn | undefined;
  const cleanup = () => {
    unDone?.();
    unErr?.();
    unDone = undefined;
    unErr = undefined;
  };

  const done = new Promise<void>((resolve, reject) => {
    void (async () => {
      unDone = await onModelDownloadDone((doneId) => {
        if (doneId === id) {
          cleanup();
          resolve();
        }
      });
      unErr = await onModelDownloadError((errId, message) => {
        if (errId === id) {
          cleanup();
          reject(new Error(message));
        }
      });
      try {
        await downloadModel(id);
      } catch (e) {
        cleanup();
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    })();
  });

  await done;
}
export const transcribeRecording = (id: string) =>
  invoke<void>("transcribe_recording", { id });
export const getTranscript = (id: string) =>
  invoke<Transcript | null>("get_transcript", { id });
export const saveTranscript = (id: string, transcript: Transcript) =>
  invoke<void>("save_transcript", { id, transcript });
export const exportRecording = (
  id: string,
  format: ExportResult["format"],
  path: string,
) => invoke<ExportResult>("export_recording", { id, format, path });

export const onModelDownloadProgress = (
  cb: (p: DownloadProgress) => void,
): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model-download-progress", (e) => cb(e.payload));

export const onModelDownloadDone = (
  cb: (id: string) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string }>("model-download-done", (e) => cb(e.payload.id));

export const onModelDownloadError = (
  cb: (id: string, message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string; message: string }>("model-download-error", (e) =>
    cb(e.payload.id, e.payload.message),
  );

export const onTranscribeProgress = (
  cb: (p: TranscribeProgress) => void,
): Promise<UnlistenFn> =>
  listen<TranscribeProgress>("transcribe-progress", (e) => cb(e.payload));

export const onTranscriptReady = (
  cb: (id: string) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string }>("transcript-ready", (e) => cb(e.payload.id));

export const onTranscribeError = (
  cb: (id: string, message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string; message: string }>("transcribe-error", (e) =>
    cb(e.payload.id, e.payload.message),
  );

// --- Live transcription ---
export const onLiveTranscriptPartial = (
  cb: (segment: Segment) => void,
): Promise<UnlistenFn> =>
  listen<Segment>("live-transcript-partial", (e) => cb(e.payload));

export const onLiveTranscriptFinal = (
  cb: (segment: Segment) => void,
): Promise<UnlistenFn> =>
  listen<Segment>("live-transcript-final", (e) => cb(e.payload));

export const onLiveTranscriptError = (
  cb: (message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ message: string }>("live-transcript-error", (e) =>
    cb(e.payload.message),
  );

// --- Resumen + correo (fase 3 / BYOK) ---
export const listSummaryTemplates = () =>
  invoke<TemplateInfo[]>("list_summary_templates");
export const listSummaryProviders = () =>
  invoke<SummaryProvider[]>("list_summary_providers");
export const ollamaAvailable = () => invoke<boolean>("ollama_available");
export const summarizeRecording = (id: string, template: string) =>
  invoke<void>("summarize_recording", { id, template });
export const getSummary = (id: string) =>
  invoke<Summary | null>("get_summary", { id });
export const saveSummary = (id: string, summary: Summary) =>
  invoke<void>("save_summary", { id, summary });

export const secretsStatus = () => invoke<SecretsStatus>("secrets_status");
export const setSecret = (kind: string, value: string) =>
  invoke<void>("set_secret", { kind, value });
export const sendSummaryEmail = (
  id: string,
  to: string[],
  subject: string,
  body: string,
) => invoke<SendMailResult>("send_summary_email", { id, to, subject, body });

export const onSummaryReady = (cb: (id: string) => void): Promise<UnlistenFn> =>
  listen<{ id: string }>("summary-ready", (e) => cb(e.payload.id));

export const onSummarizeDelta = (
  cb: (id: string, delta: string) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string; delta: string }>("summarize-delta", (e) =>
    cb(e.payload.id, e.payload.delta),
  );

export const onSummarizeError = (
  cb: (id: string, message: string) => void,
): Promise<UnlistenFn> =>
  listen<{ id: string; message: string }>("summarize-error", (e) =>
    cb(e.payload.id, e.payload.message),
  );

// --- Actualizaciones (fase 5) ---
/** Comprueba si hay una actualización en GitHub Releases (`latest.json`). */
export const checkAppUpdate = (): Promise<Update | null> => check();

/** Descarga, instala y reinicia la app con la actualización dada. */
export async function installAppUpdateAndRelaunch(
  update: Update,
  onEvent?: (event: DownloadEvent) => void,
): Promise<void> {
  await update.downloadAndInstall(onEvent);
  await relaunch();
}
