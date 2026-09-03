/** Búsqueda local y launcher Spotlight (float en el overlay). */

import { invoke } from "@tauri-apps/api/core";
import { emit, type UnlistenFn } from "@tauri-apps/api/event";
import type { BubbleOpen, LauncherHit, SearchHit } from "$core/types";
import { on } from "./events";

export const searchLocal = (query: string) =>
  invoke<SearchHit[]>("search_local", { query });

export const toggleLauncher = () => invoke<void>("toggle_launcher");
/** Abre el float del launcher (tras flyTo al slot). */
export const showLauncher = () => invoke<void>("show_launcher");
export const hideLauncher = () => invoke<void>("hide_launcher");
export const launcherSearch = (query: string) =>
  invoke<LauncherHit[]>("launcher_search", { query });
export const launcherRun = (id: string) => invoke<void>("launcher_run", { id });
export const launcherReindex = () => invoke<number>("launcher_reindex");
export const launcherListFavorites = () =>
  invoke<LauncherHit[]>("launcher_list_favorites");
export const launcherListRecents = () =>
  invoke<LauncherHit[]>("launcher_list_recents");
export const launcherToggleFavorite = (id: string) =>
  invoke<string[]>("launcher_toggle_favorite", { id });
/** Data URL PNG del icono de una app (`null` si no hay / es acción). */
export const launcherIcon = (id: string) =>
  invoke<string | null>("launcher_icon", { id });

export const onLauncherBubbleAnchor = (
  cb: (a: BubbleOpen) => void,
): Promise<UnlistenFn> => on("launcher-bubble-anchor", cb);

export const onLauncherBubbleDismiss = (cb: () => void): Promise<UnlistenFn> =>
  on("launcher-bubble-dismiss", cb);

export const onLauncherOpened = (cb: () => void): Promise<UnlistenFn> =>
  on("launcher-opened", cb);

export const onLauncherClosed = (cb: () => void): Promise<UnlistenFn> =>
  on("launcher-closed", cb);

/** Pedir a la ventana principal que abra SearchModal (mismo que Ctrl+K). */
export const requestOpenSearch = () => emit("open-search");

export const onOpenSearchRequested = (cb: () => void): Promise<UnlistenFn> =>
  on("open-search", cb);
