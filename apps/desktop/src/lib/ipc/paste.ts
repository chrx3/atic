/** Cola de pegado. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { PasteQueueItem } from "$core/types";
import { on } from "./events";

export const listPasteQueue = () => invoke<PasteQueueItem[]>("list_paste_queue");
export const enqueuePaste = (text: string) =>
  invoke<PasteQueueItem>("enqueue_paste", { text });
export const dismissPasteQueueItem = (id: string) =>
  invoke<void>("dismiss_paste_queue_item", { id });
export const clearPasteQueue = () => invoke<void>("clear_paste_queue");
export const pasteQueueItemNow = (id: string) =>
  invoke<void>("paste_queue_item_now", { id });
export const pasteQueueFlushReady = () => invoke<boolean>("paste_queue_flush_ready");

export const onPasteQueueChanged = (cb: () => void): Promise<UnlistenFn> =>
  on("paste-queue-changed", cb);

export const onPasteQueued = (cb: (preview: string) => void): Promise<UnlistenFn> =>
  on("paste-queued", (p) => cb(p.preview));
