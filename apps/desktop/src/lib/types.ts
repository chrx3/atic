export type RecordingStatus =
  | "recorded"
  | "transcribing"
  | "transcribed"
  | "summarizing"
  | "summarized"
  | "error";

export interface Recording {
  id: string;
  title: string;
  started_at: string;
  duration_secs: number;
  mic_path: string | null;
  system_path: string | null;
  status: RecordingStatus;
}

export interface AppConfig {
  language: string;
  /** Modelo Whisper para reuniones / grabaciones. */
  whisper_model: string;
  /** Modelo Whisper para dictado (latencia). */
  dictation_whisper_model: string;
  /** Motor de dictado: local | groq */
  dictation_backend: string;
  /** Modelo Groq Whisper para dictado. */
  dictation_groq_model: string;
  /** Transcribir los WAV completos automáticamente al terminar. */
  auto_transcribe_after_recording: boolean;
  /** Vista previa experimental durante la grabación. */
  live_transcription: boolean;
  /** Motor live: local | groq */
  live_engine: string;
  /** Modelo Whisper dedicado a live. */
  live_whisper_model: string;
  /** Modelo Groq Whisper para live. */
  live_groq_model: string;
  summary_backend: string;
  summary_model: string;
  summary_base_url: string;
  mail_backend: string;
  smtp_host: string;
  smtp_port: number;
  smtp_username: string;
  smtp_from: string;
  smtp_use_tls: boolean;
  global_shortcut: string;
  /** Atajo global de dictado (mic → texto → pegar). */
  dictation_shortcut: string;
  /** Atajo global para traer la pill al cursor. */
  summon_pill_shortcut: string;
  /** Atajo: abrir la rueda de herramientas en la pill. */
  pill_radial_shortcut: string;
  /** Atajo: traer pill + abrir historial de clipboard. */
  clipboard_shortcut: string;
  /** Atajo: traer pill + abrir panel de fragmentos. */
  snippets_shortcut: string;
  /** toggle | push_to_talk */
  dictation_mode: string;
  /** Nombre del micrófono. Vacío = por defecto del SO. */
  mic_device_id: string;
  /** Micrófono para dictado. Vacío = reutiliza el de reuniones. */
  dictation_mic_device_id: string;
  /** Nombre de la salida (altavoces). Vacío = por defecto del SO. */
  output_device_id: string;
  show_pill: boolean;
  pill_position: [number, number] | null;
  beep_on_start: boolean;
  /** Toques graves de interfaz (capturas, dictado). Interruptor maestro. */
  ui_sounds: boolean;
  /** Timbre por acción: "" (por defecto de la acción) | grave | suave |
   *  cristal | madera | ninguno. */
  sound_recording_start: string;
  sound_recording_stop: string;
  sound_dictation_start: string;
  sound_dictation_done: string;
  sound_capture: string;
  /** Servidores MCP para el agente, como JSON serializado. */
  agent_mcp_servers: string;
  /** Pistas a grabar: both | mic | system */
  record_tracks: string;
  /** Pistas a transcribir: both | mic | system */
  transcribe_tracks: string;
  /** Prioriza solo audio del sistema (evita eco del mic con parlantes). */
  speakers_mode: boolean;
  /** Supresión de ruido en mic: off | low | medium | high */
  noise_suppression: string;
  /** Arrancar con el sistema. */
  autostart: boolean;
  /** Onboarding de primer uso completado. */
  onboarding_done: boolean;
  /** Días de conservación; 0 = para siempre. */
  retention_days: number;
  retention_auto_cleanup: boolean;
  detect_meetings: boolean;
  screenshot_shortcut: string;
  capture_shelf_side: string;
  capture_shelf_timeout_seconds: number;
  capture_retention_hours: number;
  capture_include_cursor: boolean;
  capture_click_action: string;
  /** light | dark | system */
  ui_theme: string;
}

export interface Levels {
  mic: number;
  system: number;
}

export interface StatusPayload {
  active: boolean;
  recording: Recording | null;
}

export interface MeetingDetectionPayload {
  active: boolean;
  provider: string | null;
  title: string | null;
}

export type DictationPhase =
  | "idle"
  | "listening"
  | "transcribing"
  | "pasted"
  | "error";

export interface DictationStatusPayload {
  phase: DictationPhase;
  message: string | null;
}

export interface InputDeviceInfo {
  id: string;
  name: string;
  is_default: boolean;
  /** true si Windows/cpal no expuso configs; puede fallar al abrir. */
  may_not_open?: boolean;
  is_bluetooth?: boolean;
  is_hands_free?: boolean;
  sample_rate: number | null;
  channels: number | null;
}

export interface AudioPreflight {
  risk: "none" | "bluetooth_hands_free";
  message: string | null;
  current_mic: InputDeviceInfo | null;
  current_output: InputDeviceInfo | null;
  recommended_mic_id: string | null;
  recommended_output_id: string | null;
}

export interface WavAnalysis {
  duration_secs: number;
  sample_rate: number;
  channels: number;
  rms: number;
  peak: number;
  silent: boolean;
  clipped: boolean;
}

export interface AudioTestResult {
  mic: WavAnalysis | null;
  system: WavAnalysis | null;
  preflight: AudioPreflight;
}

export type Speaker = "me" | "others";

export interface Segment {
  start_ms: number;
  end_ms: number;
  speaker: Speaker;
  /** Nombre manual del participante. Ausente = Yo/Otros. */
  speaker_name?: string | null;
  text: string;
}

export interface Transcript {
  language: string | null;
  segments: Segment[];
}

export interface ExportResult {
  path: string;
  format: "md" | "docx" | "pdf";
}

export interface RetentionItem {
  id: string;
  title: string;
  started_at: string;
  bytes: number;
}

export interface RetentionPreview {
  days: number;
  count: number;
  bytes: number;
  items: RetentionItem[];
}

export interface RetentionCleanupResult {
  deleted: number;
  bytes_freed: number;
  errors: string[];
}

export interface ModelStatus {
  id: string;
  display_name: string;
  approx_size_bytes: number;
  downloaded: boolean;
}

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number;
}

export interface TranscribeProgress {
  id: string;
  progress: number;
}

export interface Summary {
  template: string;
  title: string;
  body: string;
  subject: string | null;
  backend: string;
  created_at: string;
}

export interface TemplateInfo {
  id: string;
  label: string;
}

export interface SummaryProvider {
  id: string;
  display_name: string;
  kind: string;
  default_base_url: string;
  default_model: string;
  needs_api_key: boolean;
  base_url_editable: boolean;
  secret_kind: string | null;
  /** Si hay elementos, la UI muestra un dropdown; si no, input libre. */
  suggested_models: string[];
}

export interface SecretsStatus {
  providers: Record<string, boolean>;
  has_smtp_password: boolean;
}

export interface SendMailResult {
  backend: string;
  message: string;
  mailto_url: string | null;
}

export interface CaptureItem {
  id: string;
  /** Etiqueta corta para el shelf (p. ej. `18:10`). */
  label: string;
  path: string;
  createdAtMs: number;
  width: number;
  height: number;
}

export type ClipboardKind = "text" | "image";

export interface ClipboardItem {
  id: string;
  kind: ClipboardKind;
  preview: string;
  text?: string | null;
  imagePath?: string | null;
  createdAtMs: number;
  pinned: boolean;
  fingerprint: string;
  source: string;
}

export interface Snippet {
  id: string;
  name: string;
  body: string;
  aliases: string[];
  updatedAtMs: number;
}

export interface Scratchpad {
  body: string;
  updatedAtMs: number;
}

/** Nota guardada. El título sale de la primera línea; no se pide aparte. */
export interface Note {
  id: string;
  title: string;
  body: string;
  updatedAtMs: number;
}

export interface PasteQueueItem {
  id: string;
  text: string;
  createdAtMs: number;
}

export type SearchHitKind =
  | "snippet"
  | "clipboard"
  | "capture"
  | "scratchpad"
  | "recording";

export interface SearchHit {
  id: string;
  kind: SearchHitKind;
  title: string;
  preview: string;
  score?: number;
}

export interface OverlayCandidate {
  hwnd: number;
  title: string;
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface OverlayInfo {
  framePath: string;
  width: number;
  height: number;
  candidates: OverlayCandidate[];
}

/* ─── Agentes ─────────────────────────────────────────────────────────────
 *
 * Estos tipos son el contrato con la capa de agentes de Rust. No describen a
 * Claude Code: describen el denominador común de cualquier agente de consola,
 * para que la UI no haya que rehacerla al sumar Codex, Cursor u otro.
 */

export interface AgentBackendInfo {
  id: string;
  displayName: string;
  /** Instalado y utilizable en este equipo. */
  available: boolean;
}

/** Un comando de barra que ofrece el agente (skills incluidas). */
export interface SlashCommand {
  name: string;
  description: string;
  /** Qué espera después del nombre, si espera algo. */
  argumentHint: string;
}

/** Un servidor MCP tal como lo reporta el agente al arrancar. */
export interface McpServerState {
  name: string;
  status: string;
}

/** Servidor MCP configurado en Atic para pasárselo al agente. */
export interface McpServerConfig {
  /** Nombre con el que el agente lo expone (`mcp__<nombre>__…`). */
  name: string;
  /** Definición JSON, tal cual va en `mcpServers`. */
  json: string;
  enabled: boolean;
}

/** Con qué arrancar una sesión. Todo opcional. */
export interface AgentStartOptions {
  cwd?: string;
  resume?: string;
  model?: string;
  permissionMode?: string;
  /** JSON `{"mcpServers": {…}}` con los servidores que suma Atic. */
  mcpConfig?: string;
  addDirs?: string[];
}

/** Evento normalizado. `kind` discrimina la variante. */
export type AgentEvent =
  | {
      kind: "started";
      sessionId: string;
      tools: string[];
      cwd: string;
      model: string;
      slashCommands: string[];
      mcpServers: McpServerState[];
    }
  | { kind: "message"; text: string }
  /** Trozo de texto según se escribe. El mensaje completo llega después. */
  | { kind: "delta"; text: string }
  | { kind: "thinking"; text: string }
  /** `input` es JSON sin interpretar: cada herramienta tiene su forma. */
  | { kind: "toolCall"; id: string; name: string; input: unknown }
  | { kind: "toolResult"; id: string; output: string; isError: boolean }
  /** El agente está DETENIDO esperando esta respuesta. */
  | {
      kind: "permission";
      id: string;
      tool: string;
      description: string;
      input: unknown;
      suggestions: unknown;
    }
  | { kind: "commands"; commands: SlashCommand[] }
  | { kind: "context"; tokens: number }
  | { kind: "notice"; text: string }
  | {
      kind: "finished";
      stopReason: string | null;
      isError: boolean;
      costUsd: number | null;
    }
  | { kind: "failed"; message: string };

/**
 * Un evento ya etiquetado con la sesión que lo produjo.
 *
 * Trae también el backend: los eventos son globales y una ventana puede recibir
 * los de una sesión que arrancó otra, sin nada más con qué nombrarla.
 */
export type AgentEventPayload = AgentEvent & {
  session: string;
  backendId: string;
  backendName: string;
};

/** Una sesión viva. El proceso lo tiene Rust, no la ventana que lo abrió. */
export interface AgentSessionInfo {
  id: string;
  backendId: string;
  backendName: string;
}
