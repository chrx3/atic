/** Preferencias, secretos, dispositivos de audio y mantenimiento de datos. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  AudioPreflight,
  AudioTestResult,
  InputDeviceInfo,
  RetentionCleanupResult,
  RetentionPreview,
  SecretsStatus,
} from "$core/types";
import { on } from "./events";

export const getConfig = () => invoke<AppConfig>("get_config");
export const setConfig = (config: AppConfig) => invoke<void>("set_config", { config });

/** Tema persistido. Cruza webviews; `storage` no llega al overlay aislado. */
export const onUiTheme = (cb: (theme: string) => void): Promise<UnlistenFn> =>
  on("ui-theme", cb);

/** Idioma de UI persistido. Cruza webviews igual que el tema. */
export const onUiLanguage = (cb: (language: string) => void): Promise<UnlistenFn> =>
  on("ui-language", cb);

export const setTrayMenu = (labels: {
  show: string;
  capture: string;
  togglePill: string;
  summonPill: string;
  quit: string;
}) => invoke<void>("set_tray_menu", { labels });

export const secretsStatus = () => invoke<SecretsStatus>("secrets_status");
export const setSecret = (kind: string, value: string) =>
  invoke<void>("set_secret", { kind, value });

/** Reproduce una acción con una voz arbitraria, para probar desde Ajustes. */
export const previewSound = (action: string, voice: string) =>
  invoke<void>("preview_sound", { action, voice });

/** Reproduce una acción con el timbre guardado, si `ui_sounds` está activo. */
export const playUiSound = (action: string) =>
  invoke<void>("play_ui_sound", { action });

export const showMainWindow = () => invoke<void>("show_main_window");

// --- Audio ---
export const listInputDevices = () => invoke<InputDeviceInfo[]>("list_input_devices");
export const listOutputDevices = () => invoke<InputDeviceInfo[]>("list_output_devices");
/** Diagnóstico: lista cruda de endpoints que ve cpal (para consola/logs). */
export const debugListAudioDevices = () => invoke<string>("debug_list_audio_devices");
export const audioPreflight = () => invoke<AudioPreflight>("audio_preflight");
export const testAudio = (config: AppConfig) =>
  invoke<AudioTestResult>("test_audio", { config });

// --- Datos y retención ---
export type DataDirKind =
  "recordings" | "clipboard" | "snippets" | "captures" | "logs" | "data";

export const openDataDir = (kind: DataDirKind) =>
  invoke<void>("open_data_dir", { kind });

export const previewRetention = (days: number) =>
  invoke<RetentionPreview>("retention_preview", { days });
export const cleanupRetention = (days: number) =>
  invoke<RetentionCleanupResult>("cleanup_retention", { days, confirm: true });

// --- Atajos globales rechazados por el SO ---
/** Nombres de los atajos que otra app ya tenía tomados. */
export const failedShortcuts = () => invoke<string[]>("failed_shortcuts");

/** Se emite en cada registro, también vacío (permite limpiar el aviso). */
export const onShortcutsFailed = (cb: (names: string[]) => void): Promise<UnlistenFn> =>
  on("shortcuts-failed", cb);

/** Claves de Groq: cuenta gratis, suficiente para dictar. */
export const GROQ_KEYS_URL = "https://console.groq.com/keys";

/** Abre una URL en el navegador del sistema. */
export async function openExternalUrl(url: string): Promise<void> {
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}
