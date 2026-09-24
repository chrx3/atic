/**
 * La posición del tema entre una muestra y la siguiente.
 *
 * Las apps publican la posición cada varios segundos (Spotify, cada ~5); leída
 * tal cual, la barra avanzaría a saltos. Mientras suena se suma lo que pasó
 * desde que la app la midió, sin pasarse del final.
 */
import type { MediaNow } from "$ipc/media";

export function mediaPosition(now: MediaNow, at: number): number | null {
  const duration = now.duration_ms ?? 0;
  if (duration <= 0 || now.position_ms == null) return null;
  const since = now.playing && now.updated_ms ? Math.max(0, at - now.updated_ms) : 0;
  return Math.min(duration, now.position_ms + since);
}

/** «3:07», o «1:02:45» si pasa de la hora. */
export function mediaClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
