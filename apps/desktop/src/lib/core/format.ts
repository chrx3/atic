import type { RecordingStatus } from "./types";

export function formatDuration(seconds: number): string {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(safeSeconds / 60);
  const remainder = safeSeconds % 60;
  return `${minutes}:${remainder.toString().padStart(2, "0")}`;
}

export function formatDate(iso: string): string {
  const value = new Date(iso);
  if (Number.isNaN(value.getTime())) return iso;
  return new Intl.DateTimeFormat("es-CL", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(value);
}

/**
 * Fecha corta para listas densas: hoy solo hora, ayer etiquetado,
 * esta semana el día, más atrás fecha media.
 */
export function formatListWhen(epochSecs: number): string {
  const value = new Date(epochSecs * 1000);
  if (Number.isNaN(value.getTime())) return "";
  const now = new Date();
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const startThat = new Date(value.getFullYear(), value.getMonth(), value.getDate());
  const dayDiff = Math.round((startToday.getTime() - startThat.getTime()) / 86_400_000);
  const time = new Intl.DateTimeFormat("es-CL", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
  if (dayDiff === 0) return time;
  if (dayDiff === 1) return `Ayer · ${time}`;
  if (dayDiff > 1 && dayDiff < 7) {
    const weekday = new Intl.DateTimeFormat("es-CL", { weekday: "short" }).format(
      value,
    );
    return `${weekday} · ${time}`;
  }
  return new Intl.DateTimeFormat("es-CL", {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
}

/** Atajo legible para texto corrido: `CmdOrCtrl+Shift+P` → `Ctrl + Shift + P`. */
export function formatShortcut(raw: string): string {
  return raw
    .replace(/CmdOrCtrl/gi, "Ctrl")
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .join(" + ");
}

export function formatMegabytes(bytes: number): string {
  return (
    new Intl.NumberFormat("es-CL", {
      maximumFractionDigits: 0,
    }).format(bytes / 1_000_000) + " MB"
  );
}

export function statusLabel(status: RecordingStatus): string {
  return {
    recorded: "Lista",
    transcribing: "Transcribiendo",
    transcribed: "Transcrita",
    summarizing: "Generando resumen",
    summarized: "Resumida",
    error: "Requiere atención",
  }[status];
}
