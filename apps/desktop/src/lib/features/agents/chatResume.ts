/**
 * Retomar una conversación guardada: arrancar el agente con su id de
 * proveedor y pintar el hilo que ya tenía.
 */
import { agentThread } from "$ipc/agents";
import { agents } from "$lib/agentSessions.svelte";
import { rememberedMode } from "$lib/agentModels";
import type { StoredThread } from "$lib/types";

/**
 * Dónde reanudar es fiable: Claude Code y Codex. El resume de ACP (OpenCode,
 * Cursor) es parcial, y ofrecer «Continuar» ahí sería mentir.
 */
export const RESUMABLE = new Set(["claude-code", "codex"]);

export function canResume(thread: StoredThread): boolean {
  return RESUMABLE.has(thread.backendId) && !!thread.providerSession;
}

/** Arranca la sesión reanudada y la deja con su hilo. Devuelve su id. */
export async function resumeThread(thread: StoredThread): Promise<string> {
  if (!thread.providerSession) throw new Error("conversación sin id del proveedor");
  const full = await agentThread(thread.id);
  const id = await agents.start(thread.backendId, {
    resume: thread.providerSession,
    cwd: thread.cwd || undefined,
    model: thread.model || undefined,
    permissionMode:
      thread.backendId === "claude-code" ? rememberedMode(thread.backendId) : undefined,
  });
  agents.hydrate(id, {
    turns: full?.turns ?? [],
    cwd: thread.cwd,
    model: thread.model,
    providerSession: thread.providerSession,
  });
  return id;
}
