/** Dictado por voz. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { DictationPhase, DictationStatusPayload } from "$core/types";
import { on } from "./events";

export const toggleDictation = () => invoke<void>("toggle_dictation");
export const dictationPhase = () => invoke<DictationPhase>("dictation_phase");

export const onDictationStatus = (
  cb: (status: DictationStatusPayload) => void,
): Promise<UnlistenFn> => on("dictation-status", cb);
