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
  AgentStartOptions,
  BubbleOpen,
  AgentTurn,
  ClaudeCodeSession,
  PermissionDecision,
  StoredThread,
} from "$core/types";
import { on } from "./events";

export type { BubbleOpen };

/** Qué agentes conoce Atic y cuáles están instalados. Lanza un proceso por
 *  backend, así que conviene llamarlo al abrir la vista, no en cada render. */
export const agentBackends = () => invoke<AgentBackendInfo[]>("agent_backends");

/** Sesiones vivas: para que una vista recién montada adopte lo que ya corre. */
export const agentSessions = () => invoke<AgentSessionInfo[]>("agent_sessions");

/** Arranca una sesión y devuelve su clave local. */
export const agentStart = (backend: string, options?: AgentStartOptions) =>
  invoke<string>("agent_start", { backend, options });

/** Catálogo de modelos del proveedor (cacheado ~5 min en Rust). */
export const agentListModels = (backend: string) =>
  invoke<AgentModel[]>("agent_list_models", { backend });

/** `origin` dice por qué puente entró el texto. No viaja al agente. */
export const agentSend = (session: string, text: string, origin?: AgentOrigin) =>
  invoke<void>("agent_send", { session, text, origin: origin ?? null });

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

// --- La burbuja ---
/** True si la burbuja de agentes está visible. */
export const agentsWindowVisible = () => invoke<boolean>("agents_window_visible");

/** Abre (o repliega) la consola de agentes: sale de la pill y vuelve a ella. */
export const showAgentsWindow = () => invoke<void>("show_agents_window");

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
