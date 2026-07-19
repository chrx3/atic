import type { RecordingStatus } from "$lib/types";

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

export function formatMegabytes(bytes: number): string {
  return new Intl.NumberFormat("es-CL", {
    maximumFractionDigits: 0,
  }).format(bytes / 1_000_000) + " MB";
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
