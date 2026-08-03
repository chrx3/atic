/** Búsqueda local y launcher Spotlight. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { LauncherHit, SearchHit } from "$core/types";
import { on } from "./events";

export const searchLocal = (query: string) =>
  invoke<SearchHit[]>("search_local", { query });

export const toggleLauncher = () => invoke<void>("toggle_launcher");
export const hideLauncher = () => invoke<void>("hide_launcher");
export const launcherSearch = (query: string) =>
  invoke<LauncherHit[]>("launcher_search", { query });
export const launcherRun = (id: string) => invoke<void>("launcher_run", { id });
export const launcherReindex = () => invoke<number>("launcher_reindex");

export const onLauncherOpened = (cb: () => void): Promise<UnlistenFn> =>
  on("launcher-opened", cb);
