export type RecordingStatus =
  "recorded" | "transcribing" | "transcribed" | "summarizing" | "summarized" | "error";

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
  /** Motor de transcripción de reuniones: local | groq */
  meeting_backend: string;
  /** Modelo Groq Whisper para reuniones. */
  meeting_groq_model: string;
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
  /** Atajo: abrir/cerrar la consola de agentes. */
  agents_shortcut: string;
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
  /** Tamaño guardado de la burbuja de agentes [w, h], o null. */
  agents_bubble_size: [number, number] | null;
  /** Consola de agentes fijada arriba (always-on-top) mientras está abierta. */
  agents_always_on_top: boolean;
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
  /** Timbre al pasar de herramienta en la rueda de la pill. */
  sound_wheel_tick: string;
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
  /** Práctica guiada de atajos (rueda, dictado, portapapeles) completada. */
  onboarding_practice_done: boolean;
  /** Días de conservación; 0 = para siempre. */
  retention_days: number;
  retention_auto_cleanup: boolean;
  detect_meetings: boolean;
  /** Guardar en disco lo que pasa por el portapapeles. */
  clipboard_history: boolean;
  screenshot_shortcut: string;
  /** Atajo global de la pizarra: dibujar sobre la pantalla congelada. */
  board_shortcut: string;
  /** Atajo global del launcher tipo Spotlight. */
  launcher_shortcut: string;
  /** Ids del launcher marcados como favoritos (`app:…` / `action:…`). */
  launcher_favorites: string[];
  capture_shelf_side: string;
  capture_shelf_timeout_seconds: number;
  capture_retention_hours: number;
  capture_include_cursor: boolean;
  capture_click_action: string;
  /** light | dark | system */
  ui_theme: string;
  /** es | en — idioma de la interfaz, no el de Whisper. */
  ui_language: string;
  /** Hosts SSH para agentes remotos (sin secretos). */
  ssh_hosts: SshHost[];
}

/** Host SSH persistido en config (passphrase/password van al keyring). */
export interface SshHost {
  id: string;
  label: string;
  /** Vacío = alias de ~/.ssh/config (User lo define el config). */
  user: string;
  /** Hostname, IP o alias Host de ssh_config (p.ej. contabo). */
  host: string;
  /** 0 = no pasar -p (usa ssh_config / default OpenSSH). */
  port: number;
  /** agent | key */
  auth: string;
  identity_file: string | null;
  default_remote_cwd: string | null;
  remote_agent_bin: string | null;
  last_test_ok: boolean | null;
  last_test_at: number | null;
}

export interface SshHostSecretFlags {
  hostId: string;
  hasPassphrase: boolean;
  hasPassword: boolean;
}

export interface SshTestResult {
  ok: boolean;
  message: string;
  checkedAt: number;
  agentAvailable: boolean | null;
}

/** Destino de la consola embebida. */
export type ConsoleKind = "local" | "ssh";

export interface ConsoleOpenOptions {
  kind: ConsoleKind;
  hostId?: string | null;
  cwd?: string | null;
  cols?: number;
  rows?: number;
}

export interface ConsoleOutputPayload {
  session: string;
  data: string;
}

export interface ConsoleExitPayload {
  session: string;
  code: number | null;
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

export type DictationPhase = "idle" | "listening" | "transcribing" | "pasted" | "error";

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

export interface LiveSummaryModels {
  models: string[];
  /** True si la lista salió del proveedor ahora; false = catálogo estático. */
  live: boolean;
  selected: string;
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

/**
 * Con qué imagen arranca el editor de anotaciones.
 *
 * Las medidas vienen de Rust (leídas del IHDR) y no del `<img>`: la ventana ya
 * nació con el tamaño calculado a partir de ellas, y esperar a que la imagen
 * cargue para saberlo dejaría el lienzo con la escala equivocada un instante.
 */
/** Panel del tamaño de la captura, o pizarra sobre la pantalla congelada. */
export type AnnotateMode = "panel" | "board";

/** Un rectángulo dentro de la imagen, en sus mismos píxeles. */
export interface FocusRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface AnnotateOpen {
  path: string;
  width: number;
  height: number;
  mode: AnnotateMode;
  /**
   * Dónde poner los controles, en píxeles de la imagen.
   *
   * Con dos monitores, el centro del escritorio virtual es la costura entre
   * las dos pantallas y una barra centrada ahí queda partida al medio.
   */
  focus: FocusRect | null;
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
  "snippet" | "clipboard" | "capture" | "scratchpad" | "recording";

export interface SearchHit {
  id: string;
  kind: SearchHitKind;
  title: string;
  preview: string;
  score?: number;
}

export type LauncherKind = "app" | "action";

export interface LauncherHit {
  id: string;
  kind: LauncherKind;
  title: string;
  subtitle: string;
  score?: number;
}

export interface OverlayCandidate {
  hwnd: number;
  title: string;
  /** Píxeles del PNG congelado, origen en la esquina del escritorio virtual. */
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface OverlayInfo {
  framePath: string;
  /** Tamaño físico del PNG congelado. */
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
  /** Id de host SSH en config; omitir = local. */
  remoteHostId?: string;
  resume?: string;
  model?: string;
  /** Cuánto tiene que pensar. Los niveles los define cada backend. */
  effort?: string;
  /** Variante rápida (Cursor). */
  fast?: boolean;
  permissionMode?: string;
  /** JSON `{"mcpServers": {…}}` con los servidores que suma Atic. */
  mcpConfig?: string;
  addDirs?: string[];
  /** Al reanudar, bifurcar en vez de seguir escribiendo el hilo original. */
  fork?: boolean;
}

/**
 * Qué se le contesta a un pedido de permiso.
 *
 * `allowAlways` no es `allow` repetido: graba la regla que sugirió el agente
 * —el modo de permisos, o el patrón de herramienta— para el resto de la sesión.
 */
export type PermissionDecision = "allow" | "allowAlways" | "deny";

/** Una skill encontrada en disco, con su descripción. */
export interface AgentSkill {
  name: string;
  description: string;
  /** Ruta del `SKILL.md`. */
  path: string;
  /** `user` (config del CLI) o `project` (la carpeta de trabajo). */
  scope: "user" | "project";
}

/**
 * En qué anda una herramienta. Son los estados de ACP.
 */
export type ToolStatus = "pending" | "in_progress" | "completed" | "failed";

/**
 * Qué clase de herramienta es.
 *
 * Lo decide el backend, no la vista. Antes el ícono y el resumen se deducían
 * de la entrada cruda (`file_path` → archivo, `command` → comando), que acierta
 * con las herramientas que ya conocés y con ninguna otra.
 */
export type ToolKind =
  | "read"
  | "edit"
  | "delete"
  | "move"
  | "search"
  | "execute"
  | "think"
  | "fetch"
  | "switch_mode"
  | "collab"
  | "other";

export type PermissionStatus = "pending" | "allowed" | "denied";

export type PlanStatus = "pending" | "in_progress" | "completed";

export interface PlanEntry {
  text: string;
  status: PlanStatus;
}

/**
 * Lo que la conversación muestra. `kind` discrimina la variante.
 *
 * Todos llevan `id` estable: es lo que permite que un item **cambie** en vez de
 * volver a dibujarse. Una herramienta es UN item que pasa de `in_progress` a
 * `completed`, no dos eventos que la vista tiene que emparejar.
 */
/**
 * De dónde salió lo que escribió el usuario.
 *
 * Solo lo llevan sus mensajes, y solo cuando entraron por un puente de Atic:
 * el dictado, una captura o el portapapeles. Ningún agente lo manda ni lo
 * entiende — se escribe de este lado y se guarda con el hilo.
 */
export interface AgentOrigin {
  /** Etiqueta corta: «dictado», «captura», «portapapeles». */
  via: string;
  /** Nombre visible del adjunto, si lo hubo. Se dibuja como tarjeta. */
  file?: string;
  /** Rutas absolutas de imágenes a embeber en el content del turno. */
  files?: string[];
}

export type AgentItem = { id: string; origin?: AgentOrigin } & (
  | { kind: "message"; role: "user" | "assistant"; text: string; streaming: boolean }
  | { kind: "reasoning"; text: string; streaming: boolean }
  | {
      kind: "tool";
      name: string;
      /** Texto legible. Lo arma el backend una vez, no la vista en cada render. */
      title: string;
      toolKind: ToolKind;
      status: ToolStatus;
      /** JSON sin interpretar: cada herramienta tiene su propia forma. */
      input: unknown;
      output: string;
      /** Archivos que toca, para poder seguirla. */
      locations: string[];
    }
  | {
      kind: "collab";
      name: string;
      title: string;
      subagentType: string;
      status: ToolStatus;
      summary: string;
      parentTurnId?: string;
      creationSource: string;
    }
  | { kind: "plan"; entries: PlanEntry[] }
  /** El agente está DETENIDO hasta que se conteste. */
  | {
      kind: "permission";
      tool: string;
      description: string;
      input: unknown;
      status: PermissionStatus;
    }
  | { kind: "notice"; text: string }
);

/** Qué cambió de un item. Lo ausente no se toca. */
export interface ItemPatch {
  text?: string;
  streaming?: boolean;
  status?: ToolStatus | PermissionStatus;
  output?: string;
  title?: string;
  summary?: string;
  subagentType?: string;
  locations?: string[];
  entries?: PlanEntry[];
}

export type TurnStatus = "running" | "done" | "failed" | "cancelled";

/** Qué cambió del hilo, no de un item suelto. */
/** Un nivel de esfuerzo, con el texto que explica cuándo conviene. */
export interface AgentEffort {
  id: string;
  description: string;
}

/**
 * Un modelo que ofrece el agente.
 *
 * **Lo informa el backend**, no una lista escrita acá. Codex usa `model/list`;
 * Claude Code alias de fallback o lo que descubra el probe; Cursor/OpenCode
 * CLI (y ACP `config_options` en sesión).
 */
export interface AgentModel {
  id: string;
  name: string;
  description: string;
  /** Vacío = este modelo no deja elegir esfuerzo. */
  efforts: AgentEffort[];
  defaultEffort?: string;
  /** Cursor: variantes `*-fast` como switch aparte. */
  supportsFast?: boolean;
}

export interface ThreadPatch {
  /** Los modelos del agente. Llega una vez, al arrancar la sesión. */
  models?: AgentModel[];
  /** Esfuerzo en curso, cuando el backend lo maneja. */
  effort?: string;
  /** Variante rápida (Cursor). */
  fast?: boolean;
  /** Id de sesión DEL BACKEND, con el que se reanuda. */
  providerSession?: string;
  cwd?: string;
  model?: string;
  mode?: string;
  /** Contexto consumido. Llega durante el turno, no al final. */
  tokens?: number;
  /**
   * Tamaño de la ventana de contexto, según el agente.
   *
   * Lo mandan ACP (`usage_update.size`) y Codex (`modelContextWindow`). Sin
   * esto la vista lo adivinaba con una constante de un millón, que es de Claude
   * y de nadie más: la ventana de Codex ronda los 258K, así que el anillo
   * mostraba un cuarto de lo consumido de verdad.
   */
  contextSize?: number;
  commands?: SlashCommand[];
  mcpServers?: McpServerState[];
  tools?: string[];
}

/**
 * Qué cambió. Es lo único que viaja del backend a la vista.
 *
 * Reemplaza al registro plano de eventos: acá `item.chunk` **sabe a qué item
 * pertenece**, así que no hace falta un campo de streaming paralelo, y dos
 * bloques transmitiendo a la vez ya no se pisan.
 */
export type AgentDelta =
  | { t: "turn.start"; turn: string }
  | { t: "item.add"; turn: string; item: AgentItem }
  | { t: "item.chunk"; item: string; text: string }
  | { t: "item.patch"; item: string; patch: ItemPatch }
  | { t: "thread.patch"; patch: ThreadPatch }
  | { t: "turn.end"; turn: string; status: TurnStatus; costUsd: number | null }
  | { t: "failed"; message: string };

/**
 * Un delta ya etiquetado con la sesión que lo produjo.
 *
 * Trae también el backend: los deltas son globales y una ventana puede recibir
 * los de una sesión que arrancó otra, sin nada más con qué nombrarla.
 */
export type AgentDeltaPayload = AgentDelta & {
  session: string;
  backendId: string;
  backendName: string;
};

/** Un agente corriendo en SU terminal. Atic solo mira. */
export type PresenceStatus = "working" | "waiting" | "ready" | "idle";

export type PresenceSource = "jsonl" | "hook" | "process";

export type PresenceWindow = {
  pid: number;
  hwnd: number;
};

export type AgentPresence = {
  id: string;
  backendId: string;
  backendName: string;
  cwd: string;
  status: PresenceStatus;
  preview: string | null;
  updatedAt: number;
  window: PresenceWindow | null;
  source: PresenceSource;
};

export type PresenceFocusResult = { kind: "focused" | "console" | "none" };

/** Un turno: un ciclo usuario → agente. */
export interface AgentTurn {
  id: string;
  items: AgentItem[];
  status: TurnStatus;
  costUsd: number | null;
}

/** Una sesión viva. El proceso lo tiene Rust, no la ventana que lo abrió. */
export interface AgentSessionInfo {
  id: string;
  backendId: string;
  backendName: string;
}

/**
 * Una conversación guardada en `atic.db3`.
 *
 * Los turnos son los MISMOS que los de una sesión viva, y por eso la vista los
 * dibuja con el mismo código: lo guardado no es un resumen ni otro formato, es
 * el hilo tal cual quedó. Al listar llegan vacíos —serían megabytes para pintar
 * unas líneas—, y solo `agentThread` los trae de verdad.
 */
export interface StoredThread {
  id: string;
  backendId: string;
  backendName: string;
  /** Id con el que Claude Code o Codex reanudan la conversación. */
  providerSession: string | null;
  cwd: string;
  /** Host SSH; null = local. */
  remoteHostId: string | null;
  model: string;
  /** Segundos desde epoch. */
  updatedAt: number;
  /** Las primeras palabras del usuario: con lo que se reconoce en la lista. */
  preview: string;
  turns: AgentTurn[];
}

/**
 * Sesión del CLI Claude Code en `~/.claude/projects/…`.
 * Solo índice para `--resume`; el transcript no se importa a Atic.
 */
export interface ClaudeCodeSession {
  id: string;
  preview: string;
  updatedAt: number;
  cwd: string;
}

/** Ventana de cupo de la cuenta Claude (misma fuente que `/usage`). */
export interface ClaudeUsageWindow {
  /** Porcentaje consumido, 0..=100. */
  utilization: number;
  /** RFC3339 UTC, o null si la API no lo manda. */
  resetsAt: string | null;
}

export interface ClaudeExtraUsage {
  isEnabled: boolean;
  monthlyLimit: number | null;
  usedCredits: number | null;
  utilization: number | null;
  currency: string | null;
}

/** Snapshot de cupos Pro/Max vía OAuth usage API. */
export interface ClaudeAccountUsage {
  fiveHour: ClaudeUsageWindow | null;
  sevenDay: ClaudeUsageWindow | null;
  sevenDayOpus: ClaudeUsageWindow | null;
  sevenDaySonnet: ClaudeUsageWindow | null;
  extraUsage: ClaudeExtraUsage | null;
  plan: string | null;
  /** Epoch ms del fetch. */
  fetchedAt: number;
}

/** Entrada de carpeta del explorador interno (solo directorios). */
export interface DirectoryEntry {
  name: string;
  path: string;
}

/** Listado de subcarpetas + atajos (Inicio / Escritorio / Documentos). */
export interface DirectoryListing {
  path: string;
  parent: string | null;
  entries: DirectoryEntry[];
  roots: DirectoryEntry[];
}

/**
 * Dónde va el globo de la consola, ya en píxeles CSS del overlay.
 *
 * Lo decide Rust: es quien ve los monitores y la posición de la pill.
 */
export interface BubbleOpen {
  side: "top" | "bottom" | "left" | "right";
  /** De la esquina del globo al centro del cuello, por el lado anclado. */
  offset: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Lo que se inserta en el compositor cuando el clipboard pega con la consola
 *  de agentes abierta. */
export interface AgentsComposerInsert {
  kind: ClipboardKind;
  text?: string | null;
  imagePath?: string | null;
}
