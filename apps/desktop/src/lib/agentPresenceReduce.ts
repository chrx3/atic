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
    const next = snapshot.find((p) => p.id === id);
    if (!next || next.status === "idle") delete unread[id];
  }
  return { list: snapshot, unread };
}

/** Apaga varios avisos de una: al cerrar el globo de Atic, esos ya se vieron. */
export function markPresenceSeenMany(
  unread: Record<string, number>,
  ids: Iterable<string>,
): Record<string, number> {
  let next = unread;
  for (const id of ids) {
    const marked = markPresenceSeen(next, id);
    if (marked !== next) next = marked;
  }
  return next;
}

/** El clic sobre el aviso lo apaga aunque el foco no se confirme: el usuario ya actuó (a lo sumo queda con la consola delante). */
export function markPresenceSeen(
  unread: Record<string, number>,
  id: string,
): Record<string, number> {
  if (!(id in unread)) return unread;
  return { ...unread, [id]: 0 };
}
