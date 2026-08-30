import type { AgentPresence } from "$core/types";

export type PresenceView = AgentPresence & { unread: number };

export function applyPresenceSnapshot(
  prev: { list: AgentPresence[]; unread: Record<string, number>; watching: boolean },
  snapshot: AgentPresence[],
): { list: AgentPresence[]; unread: Record<string, number> } {
  const prevById = new Map(prev.list.map((p) => [p.id, p]));
  const unread: Record<string, number> = { ...prev.unread };
  for (const next of snapshot) {
    const old = prevById.get(next.id);
    // Solo el paso working/waiting → ready cuenta. Un `ready` que ya estaba
    // así al adoptar (sesión vieja en el JSONL, primer snapshot al arrancar)
    // no es un aviso: el usuario no acaba de recibir nada.
    if (
      next.status === "ready" &&
      old != null &&
      !prev.watching &&
      (old.status !== "ready" ||
        (Boolean(next.preview) && next.preview !== old.preview))
    ) {
      unread[next.id] = (unread[next.id] ?? 0) + 1;
    }
  }
  for (const id of Object.keys(unread)) {
    if (!snapshot.some((p) => p.id === id)) delete unread[id];
  }
  return { list: snapshot, unread };
}

/** Solo si el foco se confirmó: si no llevamos al usuario a ningún lado, no se borra. */
export function markPresenceSeen(
  unread: Record<string, number>,
  id: string,
): Record<string, number> {
  if (!(id in unread)) return unread;
  return { ...unread, [id]: 0 };
}
