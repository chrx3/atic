/**
 * Detector de "terminó" para las consolas fuera de vista.
 *
 * Sin PTY que pregunte si el turno acabó, la señal es el silencio: una sesión
 * que venía escribiendo y lleva `idleMs` sin una línea —acabó el turno o
 * espera algo (permiso, entrada)— necesita ojos. La decisión es pura para
 * poder probar los bordes; el componente pone el reloj y los avisos.
 */

export type AttentionSession = {
  id: string;
  label: string;
  /** Epoch ms del último output, o null si nunca escribió. */
  lastOutputAt: number | null;
};

export function attentionDue(input: {
  now: number;
  /** La consola está a la vista: a la vista no se avisa nada. */
  visible: boolean;
  idleMs: number;
  /** Última vez que la consola estuvo a la vista. */
  lastSeenVisibleAt: number;
  /** Sesiones ya avisadas de su racha actual. */
  sent: ReadonlySet<string>;
  sessions: AttentionSession[];
}): AttentionSession[] {
  if (input.visible) return [];
  return input.sessions.filter((session) => {
    if (session.lastOutputAt == null) return false;
    // Lo que llegó antes de mirar por última vez ya se vio.
    if (session.lastOutputAt <= input.lastSeenVisibleAt) return false;
    if (input.sent.has(session.id)) return false;
    return input.now - session.lastOutputAt >= input.idleMs;
  });
}
