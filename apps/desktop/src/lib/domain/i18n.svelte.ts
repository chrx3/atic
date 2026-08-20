/**
 * Idioma de la interfaz. Reactivo: `t()` se relee al cambiar el selector.
 *
 * El overlay no comparte el store de config de `main`; el locale se aplica
 * acá y por el evento `ui-language`.
 */
import { setFormatLocale } from "$core/format";
import { translate, parseLocale, type Locale } from "$core/i18n/translate";
import type { ToolDef } from "$core/tools";
import { setTrayMenu } from "$ipc/config";

let locale = $state<Locale>("es");

export function uiLocale(): Locale {
  return locale;
}

export function t(key: string, vars?: Record<string, string | number>): string {
  return translate(locale, key, vars);
}

export function whisperModelLabel(id: string): string {
  const key = `models.whisper.${id}`;
  const value = t(key);
  return value === key ? id : value;
}

export function groqModelLabel(id: string): string {
  const key = `models.groq.${id}`;
  const value = t(key);
  return value === key ? t("models.groqFallback") : value;
}

export function localizeTool(tool: ToolDef): ToolDef {
  return {
    ...tool,
    label: t(`tools.${tool.id}.label`),
    short: t(`tools.${tool.id}.short`),
    blurb: t(`tools.${tool.id}.blurb`),
    actionLabel: t(`tools.${tool.id}.actionLabel`),
  };
}

function syncTray(): void {
  void setTrayMenu({
    show: t("tray.show"),
    capture: t("tray.capture"),
    togglePill: t("tray.togglePill"),
    summonPill: t("tray.summonPill"),
    quit: t("tray.quit"),
  }).catch(() => {
    // Fuera de Tauri, o el tray todavía no existe.
  });
}

export function applyUiLocale(raw: string | undefined | null): Locale {
  locale = parseLocale(raw);
  setFormatLocale(locale);
  if (typeof document !== "undefined") {
    document.documentElement.lang = locale;
  }
  syncTray();
  return locale;
}
