/** La consola de agentes: sesiones, catálogo, permisos e hilos guardados. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentBackendInfo,
  AgentDeltaPayload,
  AgentModel,
  AgentOrigin,
  AgentSessionInfo,
  AgentSkill,
  AgentPresence,
  PresenceFocusResult,
  AgentStartOptions,
  BubbleOpen,
  AgentTurn,
  ClaudeAccountUsage,
  CodexAccountUsage,
  ClaudeCodeSession,
  ConsoleExitPayload,
  ConsoleOpenOptions,
  ConsoleOutputPayload,
  DirectoryListing,
  PermissionDecision,
  SshHost,
  SshHostSecretFlags,
  SshTestResult,
  StoredThread,
} from "$core/types";
import { on, type AgentsWorkspaceShortcut } from "./events";

export type {
  BubbleOpen,
  ClaudeAccountUsage,
  CodexAccountUsage,
  ConsoleExitPayload,
  ConsoleOpenOptions,
  ConsoleOutputPayload,
  DirectoryListing,
  SshHost,
  SshHostSecretFlags,
  SshTestResult,
};

/** Qué agentes conoce Atic y cuáles están instalados. Lanza un proceso por
 *  backend, así que conviene llamarlo al abrir la vista, no en cada render. */
export const agentBackends = () => invoke<AgentBackendInfo[]>("agent_backends");

/** True si el binario está en el PATH (misma regla que al abrir la PTY). */
export const cliOnPath = (name: string) => invoke<boolean>("cli_on_path", { name });

/** Cupos de ChatGPT/Codex mediante el app-server del CLI ya autenticado. */
export const agentCodexUsage = () => invoke<CodexAccountUsage>("agent_codex_usage");

/** Sesiones vivas: para que una vista recién montada adopte lo que ya corre. */
export const agentSessions = () => invoke<AgentSessionInfo[]>("agent_sessions");

/** Presencias de TUI (pager). Snapshot completo, no deltas. */
export const agentPresences = () => invoke<AgentPresence[]>("agent_presences");

/** Enfoca la TUI de esa presencia. `none` si no hay ventana; no abre el chat. */
export const agentPresenceFocus = (id: string) =>
  invoke<PresenceFocusResult>("agent_presence_focus", { id });

/** Ata la última ventana externa a esa presencia y la enfoca. */
export const agentPresenceBind = (id: string) =>
  invoke<PresenceFocusResult>("agent_presence_bind", { id });

/** Fragmento de hooks para pegar en ~/.claude/settings.json. No se escribe solo. */
export const agentPresenceHookSnippet = () =>
  invoke<string>("agent_presence_hook_snippet");

/** Arranca una sesión y devuelve su clave local. */
export const agentStart = (backend: string, options?: AgentStartOptions) =>
  invoke<string>("agent_start", { backend, options });

/** Catálogo de modelos del proveedor (cacheado ~5 min en Rust). */
export const agentListModels = (backend: string) =>
  invoke<AgentModel[]>("agent_list_models", { backend });

/** `origin` dice por qué puente entró el texto. No viaja al agente. */
export const agentSend = (session: string, text: string, origin?: AgentOrigin) =>
  invoke<void>("agent_send", { session, text, origin: origin ?? null });

/**
 * Escribe una imagen (pegada/arrastrada como bytes) en temp y devuelve la ruta
 * absoluta para `origin.files`.
 */
export const agentStageImage = (dataBase64: string, mime: string) =>
  invoke<string>("agent_stage_image", {
    dataBase64,
    mime,
  });

/** Cambia el modelo y el esfuerzo sin reiniciar la sesión. */
export const agentSetModel = (
  session: string,
  model: string,
  effort?: string,
  fast?: boolean,
) =>
  invoke<void>("agent_set_model", {
    session,
    model,
    effort: effort ?? null,
    fast: fast ?? null,
  });

/** Contesta un permiso. El turno del agente está detenido hasta esta llamada. */
export const agentPermission = (
  session: string,
  id: string,
  decision: PermissionDecision,
) => invoke<void>("agent_permission", { session, id, decision });

/** Skills visibles desde `cwd`. Se consulta cada vez: son archivos editables. */
export const agentSkills = (cwd?: string) =>
  invoke<AgentSkill[]>("agent_skills", { cwd: cwd ?? null });

/** Interrumpe el turno en curso sin cerrar la sesión. */
export const agentInterrupt = (session: string) =>
  invoke<void>("agent_interrupt", { session });

/** Cierra la sesión y mata el proceso del agente. */
export const agentStop = (session: string) => invoke<void>("agent_stop", { session });

/** Conversaciones guardadas, de la más reciente a la más vieja y sin turnos. */
export const agentThreads = () => invoke<StoredThread[]>("agent_threads");
/** Una conversación guardada, ya con todos sus turnos. */
export const agentThread = (id: string) =>
  invoke<StoredThread | null>("agent_thread", { id });
export const agentThreadDelete = (id: string) =>
  invoke<void>("agent_thread_delete", { id });

/** Sesiones locales del CLI Claude Code para un cwd (reanudar con --resume). */
export const agentClaudeSessions = (cwd: string) =>
  invoke<ClaudeCodeSession[]>("agent_claude_sessions", { cwd });

/** Transcript del CLI en turnos canónicos (para pintar al reanudar). */
export const agentClaudeTranscript = (cwd: string, id: string) =>
  invoke<AgentTurn[]>("agent_claude_transcript", { cwd, id });

/**
 * Cupos de la cuenta Claude (ventana 5 h / semanal). Misma fuente que `/usage`.
 * Cachea unos segundos en Rust; el modal puede pedirlo en poll.
 */
export const agentClaudeUsage = () => invoke<ClaudeAccountUsage>("agent_claude_usage");

/**
 * Subcarpetas de `path` (vacío/`~` → home). Solo lectura; sin abrir el picker
 * nativo (compatible con always-on-top del float).
 */
export const listDirectories = (path?: string | null) =>
  invoke<DirectoryListing>("list_directories", { path: path ?? null });

/** Hosts SSH guardados en config (sin secretos). */
export const sshListHosts = () => invoke<SshHost[]>("ssh_list_hosts");

/** Aliases `Host` de ~/.ssh/config (los mismos que muestran VS Code/Cursor). */
export const sshConfigAliases = () => invoke<string[]>("ssh_config_aliases");

/** Flags has_passphrase / has_password por host. */
export const sshHostSecretsStatus = () =>
  invoke<SshHostSecretFlags[]>("ssh_host_secrets_status");

/** Guarda o borra passphrase/password. Valor vacío elimina. */
export const sshSetHostSecret = (
  hostId: string,
  kind: "passphrase" | "password",
  value: string,
) => invoke<void>("ssh_set_host_secret", { hostId, kind, value });

/** Borra secretos del keyring al eliminar un host. */
export const sshDeleteHostSecrets = (hostId: string) =>
  invoke<void>("ssh_delete_host_secrets", { hostId });

/** Prueba conexión (BatchMode). Si el id ya está en config, actualiza last_test_*. */
export const sshTestHost = (host: SshHost) =>
  invoke<SshTestResult>("ssh_test_host", { host });

/** Abre PTY local o `ssh -t`. Reemplaza solo la sesión del mismo kind (local|ssh). */
export const consoleOpen = (options: ConsoleOpenOptions) =>
  invoke<string>("console_open", { options });

export const consoleWrite = (session: string, data: string) =>
  invoke<void>("console_write", { session, data });

export const consoleResize = (session: string, cols: number, rows: number) =>
  invoke<void>("console_resize", { session, cols, rows });

export const consoleClose = (session: string) =>
  invoke<void>("console_close", { session });

/** Cierra en Rust las PTYs cuyo id no está en `keep`. */
export const consoleGc = (keep: string[]) =>
  invoke<number>("console_gc", { keep });

export const onConsoleOutput = (
  cb: (payload: ConsoleOutputPayload) => void,
): Promise<UnlistenFn> => on("console-output", cb);

export const onConsoleExit = (
  cb: (payload: ConsoleExitPayload) => void,
): Promise<UnlistenFn> => on("console-exit", cb);

/** Atajos que WebView2 reserva y Rust reenvía directamente a la consola. */
export const onAgentsWorkspaceShortcut = (
  cb: (shortcut: AgentsWorkspaceShortcut) => void,
): Promise<UnlistenFn> => on("agents-workspace-shortcut", cb);

// --- La burbuja ---
/** True si la burbuja de agentes está visible. */
export const agentsWindowVisible = () => invoke<boolean>("agents_window_visible");

/** Abre (o repliega) la consola de agentes: sale de la pill y vuelve a ella. */
export const showAgentsWindow = () => invoke<void>("show_agents_window");

/**
 * Pide al lanzador que muestre las consolas ya vivas, sin toggle.
 *
 * El chip de la pill lo dispara antes de abrir el float: si hay PTY,
 * se ve la consola y no el setup.
 */
export const AGENTS_REVEAL_CONSOLE = "atic-agents-reveal-console";

export function revealAgentsConsole() {
  window.dispatchEvent(new Event(AGENTS_REVEAL_CONSOLE));
}

/** Repliega la burbuja sobre la pill. */
export const hideAgentsWindow = () => invoke<void>("hide_agents_window");

/**
 * Guarda a qué tamaño quedó el globo, para la próxima apertura.
 *
 * Solo al soltar: durante el arrastre el tamaño lo aplica la vista, y mandar
 * cada cuadro al disco guardaría sesenta valores que nadie va a leer.
 */
export const saveAgentsBubbleSize = (w: number, h: number) =>
  invoke<void>("save_agents_bubble_size", { w, h });

/** ¿La consola de agentes queda fijada arriba de otras apps? */
export const agentsAlwaysOnTop = () => invoke<boolean>("agents_always_on_top");

/** Fija o desfija la consola (always-on-top del overlay mientras está abierta). */
export const setAgentsAlwaysOnTop = (on: boolean) =>
  invoke<void>("set_agents_always_on_top", { on });

/** Lo decide Rust: es quien ve los monitores y la posición de la pill. */
export const onAgentsBubbleAnchor = (
  cb: (a: BubbleOpen) => void,
): Promise<UnlistenFn> => on("agents-bubble-anchor", cb);

/** La burbuja se está replegando: el contenido tiene que apagarse ya. */
export const onAgentsBubbleDismiss = (cb: () => void): Promise<UnlistenFn> =>
  on("agents-bubble-dismiss", cb);

/** Todos los eventos de todas las sesiones. Filtrar por `session`. */
export const onAgentDelta = (
  cb: (payload: AgentDeltaPayload) => void,
): Promise<UnlistenFn> => on("agent-event", cb);

/** Snapshot completo de agentes que corren en su TUI. */
export const onAgentPresence = (
  cb: (payload: AgentPresence[]) => void,
): Promise<UnlistenFn> => on("agent-presence", cb);
