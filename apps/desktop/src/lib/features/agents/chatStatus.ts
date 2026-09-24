/**
 * El estado de una ficha de chat, para el rail y la pill.
 *
 * Una terminal solo puede decir «activa» o «pausada»: no sabe en qué anda el
 * CLI. Un chat sí, porque la sesión lo informa. Esto lo reduce a lo que se
 * muestra, en orden de urgencia: lo que te necesita gana.
 */

export type ChatTabStatus =
  "gone" | "waiting" | "failed" | "working" | "unread" | "ready";

type StatusSession = {
  status: string;
  pending: readonly unknown[];
  unread: number;
  turns?: readonly { items: readonly { kind: string; role?: string }[] }[];
};

/**
 * `working`, salvo que todavía no le hayas escrito nada: al arrancar, Claude
 * emite sus hooks dentro de un turno y el store lo marca trabajando. Eso es
 * arrancar, no trabajar, y decía «Trabajando…» en un chat recién abierto.
 */
export function isWorking(session: StatusSession): boolean {
  if (session.status !== "working") return false;
  if (!session.turns) return true;
  return session.turns.some((turn) =>
    turn.items.some((item) => item.kind === "message" && item.role === "user"),
  );
}

export function chatTabStatus(session: StatusSession | undefined): ChatTabStatus {
  if (!session) return "gone";
  if (session.pending.length > 0) return "waiting";
  if (session.status === "failed") return "failed";
  if (isWorking(session)) return "working";
  if (session.unread > 0) return "unread";
  return "ready";
}
