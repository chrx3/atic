/**
 * Las PTY de una ventana: un solo oyente de salida y de fin para todas las
 * terminales, y el reclamo de las sesiones que la ventana muestra.
 *
 * En el panel de consolas cada instancia registraba sus propios oyentes
 * globales; con varias terminales en una ventana, cada chunk pasaba por todas.
 * Acá llega una vez y se reparte por id. Lo que llega antes de que la vista
 * monte se guarda (con tope) y se entrega al suscribirse.
 *
 * El reclamo es lo que le dice a Rust «esta sesión tiene quien la mire»: sin
 * heartbeat, el barrido la da por huérfana y la cierra.
 */
import {
  consoleAttach,
  consoleDetach,
  consoleHeartbeat,
  onConsoleExit,
  onConsoleOutput,
} from "$ipc/agents";

/** Id de esta ventana ante Rust. Uno por webview, no por vista. */
export const CONSOLE_VIEW_ID = crypto.randomUUID();

const HEARTBEAT_MS = 10_000;
/** Tope de lo guardado para una sesión sin vista: lo viejo se descarta. */
const BUFFER_MAX = 256 * 1024;

type OutputFn = (data: string) => void;
type ExitFn = (code: number | null) => void;

const outputs = new Map<string, OutputFn>();
const exits = new Map<string, ExitFn>();
const pending = new Map<string, string>();
const claimed = new Set<string>();
const taps = new Set<(session: string) => void>();
let heartbeat = 0;
let started: Promise<void> | null = null;

/** Escucha una vez por ventana. Resuelve cuando los oyentes están activos. */
export function consoleBusReady(): Promise<void> {
  started ??= Promise.all([
    onConsoleOutput(({ session, data }) => {
      for (const tap of taps) tap(session);
      const fn = outputs.get(session);
      if (fn) {
        fn(data);
        return;
      }
      const prev = pending.get(session) ?? "";
      const next = prev + data;
      pending.set(session, next.length > BUFFER_MAX ? next.slice(-BUFFER_MAX) : next);
    }),
    onConsoleExit(({ session, code }) => {
      exits.get(session)?.(code ?? null);
      release(session);
    }),
  ]).then(() => undefined);
  return started;
}

/**
 * La vista de una sesión: recibe su salida (y lo que quedó guardado) y su
 * fin. Devuelve la baja.
 */
export function subscribeConsole(session: string, onOutput: OutputFn, onExit: ExitFn) {
  outputs.set(session, onOutput);
  exits.set(session, onExit);
  const early = pending.get(session);
  if (early) {
    pending.delete(session);
    onOutput(early);
  }
  return () => {
    if (outputs.get(session) === onOutput) outputs.delete(session);
    if (exits.get(session) === onExit) exits.delete(session);
  };
}

/**
 * Aviso de que una sesión escribió, sin el contenido: para saber quién está
 * trabajando sin quitarle la salida a su terminal. Devuelve la baja.
 */
export function watchOutput(fn: (session: string) => void): () => void {
  taps.add(fn);
  return () => taps.delete(fn);
}

/** Esta ventana muestra la sesión: que el barrido de Rust no la toque. */
export function claim(session: string) {
  if (claimed.has(session)) return;
  claimed.add(session);
  void consoleAttach(session, CONSOLE_VIEW_ID).catch(() => {});
  if (!heartbeat) {
    heartbeat = window.setInterval(() => {
      if (claimed.size === 0) return;
      void consoleHeartbeat(CONSOLE_VIEW_ID, [...claimed]).catch(() => {});
    }, HEARTBEAT_MS);
  }
}

/** Soltarla no es cerrarla: si nadie más la reclama, Rust la barre. */
export function release(session: string) {
  if (!claimed.delete(session)) return;
  pending.delete(session);
  void consoleDetach(session, CONSOLE_VIEW_ID).catch(() => {});
}
