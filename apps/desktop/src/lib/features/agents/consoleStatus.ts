/**
 * En qué anda cada consola de la pizarra, y cuándo pide que se la mire.
 *
 * Dos fuentes, de la más fina a la más gruesa:
 * - la presencia que arma Rust (hooks de Claude, watchers de otros CLI): sabe
 *   si espera un permiso, pero solo existe para algunos agentes;
 * - la salida de la PTY, que tienen todas: un TUI que trabaja repinta sin
 *   parar (el spinner, el texto que llega); uno que terminó se calla.
 *
 * Todo puro: el reloj y los eventos los pone la pizarra.
 */
import type { PresenceStatus } from "$core/types";

export type ConsoleState = "working" | "waiting" | "ready" | "ended";

/** La racha de salida de una sesión: cuándo escribió por última vez y desde cuándo seguido. */
export type Activity = { last: number; since: number };

/** Sin salida por esto, la consola se da por callada. */
export const QUIET_MS = 1500;
/**
 * Menos que esto no es un turno: un repintado por cambiar el tamaño o un
 * eco de teclas. No merece avisar al callarse.
 */
export const TURN_MS = 3000;

export function noteOutput(prev: Activity | undefined, now: number): Activity {
  if (!prev || now - prev.last > QUIET_MS) return { last: now, since: now };
  return { last: now, since: prev.since };
}

export function consoleState(input: {
  ended: boolean;
  presence: PresenceStatus | null;
  activity: Activity | undefined;
  now: number;
}): ConsoleState {
  if (input.ended) return "ended";
  if (input.presence === "waiting") return "waiting";
  if (input.presence === "working") return "working";
  if (input.presence === "ready" || input.presence === "idle") return "ready";
  const a = input.activity;
  return a && input.now - a.last <= QUIET_MS ? "working" : "ready";
}

/**
 * ¿Pasa a pedir atención? Solo si no se la está mirando, y por algo que
 * valga: un permiso siempre; un turno que termina, si fue un turno de verdad.
 */
export function needsAttention(input: {
  prev: ConsoleState;
  next: ConsoleState;
  /** Cuánto duró la racha de trabajo que acaba de terminar. */
  turnMs: number;
  watched: boolean;
}): boolean {
  if (input.watched || input.prev === input.next) return false;
  if (input.next === "waiting") return true;
  return input.prev === "working" && input.next === "ready" && input.turnMs >= TURN_MS;
}
