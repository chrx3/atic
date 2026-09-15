/** Dictado por voz. */

import { invoke } from "@tauri-apps/api/core";
import type { DictationPhase } from "$core/types";

export const toggleDictation = () => invoke<void>("toggle_dictation");
export const dictationPhase = () => invoke<DictationPhase>("dictation_phase");
/** Último texto dictado en esta sesión; `null` si todavía no hubo. */
export const dictationLastText = () => invoke<string | null>("dictation_last_text");

// El estado llega por eventos: el store los escucha con `subscribe` de
// `ipc/events` porque necesita `dictation-status` y `audio-levels` a la vez.
