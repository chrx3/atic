import { parseLocale, translate, type Locale } from "./i18n/translate";
import type { RecordingStatus } from "./types";

let locale: Locale = "es";

export function setFormatLocale(raw: string | undefined | null): void {
  locale = parseLocale(raw);
}

export function formatLocale(): Locale {
  return locale;
}

function intlTag(): string {
  return locale === "en" ? "en-US" : "es-CL";
}

export function formatDuration(seconds: number): string {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(safeSeconds / 60);
  const remainder = safeSeconds % 60;
  return `${minutes}:${remainder.toString().padStart(2, "0")}`;
}

export function formatDate(iso: string): string {
  const value = new Date(iso);
  if (Number.isNaN(value.getTime())) return iso;
  return new Intl.DateTimeFormat(intlTag(), {
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
  const time = new Intl.DateTimeFormat(intlTag(), {
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
  if (dayDiff === 0) return time;
  if (dayDiff === 1) return translate(locale, "format.yesterday", { time });
  if (dayDiff > 1 && dayDiff < 7) {
    const weekday = new Intl.DateTimeFormat(intlTag(), { weekday: "short" }).format(
      value,
    );
    return `${weekday} · ${time}`;
  }
  return new Intl.DateTimeFormat(intlTag(), {
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
    new Intl.NumberFormat(intlTag(), {
      maximumFractionDigits: 0,
    }).format(bytes / 1_000_000) + " MB"
  );
}

export function statusLabel(status: RecordingStatus): string {
  return translate(locale, `page.meetings.status.${status}`);
}
