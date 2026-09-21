/** Controles de sistema: recursos, audio, pantallas y cierre de apps. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { BubbleOpen } from "$core/types";
import { on } from "./events";

export type SystemApp = {
  id: string;
  name: string;
  pid: number;
  cpu: number;
  ram_bytes: number;
  can_close: boolean;
  can_force: boolean;
  /** Tiene ventana: se puede traer al frente. */
  can_focus: boolean;
  /** Sin ventana ni ícono (node, cargo, helpers). La vista lo esconde. */
  background: boolean;
};

export type SystemSnapshot = {
  cpu: number;
  ram_used: number;
  ram_total: number;
  apps: SystemApp[];
};

export type AudioSession = {
  id: string;
  name: string;
  volume: number;
  muted: boolean;
};

export type SystemAudio = {
  volume: number;
  muted: boolean;
  per_app: boolean;
  sessions: AudioSession[];
};

export type SystemDisplay = {
  id: string;
  name: string;
  primary: boolean;
  brightness: number | null;
};

export const showSystemWindow = () => invoke<void>("show_system_window");
export const hideSystemWindow = () => invoke<void>("hide_system_window");
export const systemAlwaysOnTop = () => invoke<boolean>("system_always_on_top");
export const setSystemAlwaysOnTop = (on: boolean) =>
  invoke<void>("set_system_always_on_top", { on });

export const systemSnapshot = () => invoke<SystemSnapshot>("system_snapshot");
export const systemAudio = () => invoke<SystemAudio>("system_audio");
export const systemSetVolume = (volume: number) =>
  invoke<void>("system_set_volume", { volume });
export const systemSetMuted = (muted: boolean) =>
  invoke<void>("system_set_muted", { muted });
export const systemSetSessionVolume = (id: string, volume: number) =>
  invoke<void>("system_set_session_volume", { id, volume });
export const systemDisplays = () => invoke<SystemDisplay[]>("system_displays");
export const systemSetBrightness = (id: string, brightness: number) =>
  invoke<void>("system_set_brightness", { id, brightness });
export const systemCloseApp = (id: string) =>
  invoke<number>("system_close_app", { id });
export const systemForceApp = (id: string) => invoke<void>("system_force_app", { id });
/** Trae la app al frente. Lo que uno quiere hacer con una lista de apps. */
export const systemFocusApp = (id: string) => invoke<void>("system_focus_app", { id });
export const systemAction = (id: "lock" | "sleep" | "mute" | "trash") =>
  invoke<void>("system_action", { id });

/** ¿El equipo está retenido despierto (café)? */
export const systemAwake = () => invoke<boolean>("system_awake");
/** Café: mientras esté puesto, el equipo no se duerme ni apaga la pantalla. */
export const setSystemAwake = (on: boolean) => invoke<void>("set_system_awake", { on });

export type SystemAlertKind = "cpu" | "ram";

export type SystemAlert = {
  kind: SystemAlertKind;
  /** Valor que disparó el aviso, en porcentaje. */
  value: number;
  threshold: number;
  /** Quién se lo está comiendo, si se pudo averiguar. */
  culprit: string | null;
};

export type AlertSettings = {
  enabled: boolean;
  /** Umbral de CPU (%). 0 = no vigilar. */
  cpu: number;
  /** Umbral de memoria (%). 0 = no vigilar. */
  ram: number;
  /** Cuántos segundos seguidos por encima antes de avisar. */
  seconds: number;
};

/** Avisos encendidos ahora mismo: la pill los pide al montarse. */
export const systemAlerts = () => invoke<SystemAlert[]>("system_alerts");
export const systemAlertSettings = () => invoke<AlertSettings>("system_alert_settings");
export const setSystemAlertSettings = (settings: AlertSettings) =>
  invoke<void>("set_system_alert_settings", { settings });

/** El vigilante cambió de opinión: lista completa de lo que está encendido. */
export const onSystemAlert = (cb: (a: SystemAlert[]) => void): Promise<UnlistenFn> =>
  on("system-alert", cb);

export const onSystemBubbleAnchor = (
  cb: (a: BubbleOpen) => void,
): Promise<UnlistenFn> => on("system-bubble-anchor", cb);

export const onSystemBubbleDismiss = (cb: () => void): Promise<UnlistenFn> =>
  on("system-bubble-dismiss", cb);
