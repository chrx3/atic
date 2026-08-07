/**
 * Evita que `overlay-dismiss` cierre la consola de agentes cuando el clic
 * “afuera” no es intención de cierre (diálogo nativo del SO, etc.).
 *
 * Raw Input ve el clic en el folder picker como Outside → emitiría dismiss
 * al confirmar. Mientras `depth > 0` (diálogo abierto) y un breve grace
 * después, AgentsFloat ignora ese evento.
 */

let depth = 0;
let graceUntil = 0;

const DEFAULT_GRACE_MS = 500;

/** ¿Hay que ignorar el dismiss automático ahora? */
export function isAgentsDismissSuppressed(): boolean {
  return depth > 0 || performance.now() < graceUntil;
}

/**
 * Envuelve un diálogo nativo (u otra pérdida de foco temporal).
 * El grace cubre el clic de confirmación que a veces llega después del await.
 */
export async function withAgentsDismissSuppressed<T>(
  run: () => Promise<T>,
  graceMs = DEFAULT_GRACE_MS,
): Promise<T> {
  depth += 1;
  try {
    return await run();
  } finally {
    depth = Math.max(0, depth - 1);
    graceUntil = performance.now() + graceMs;
  }
}
