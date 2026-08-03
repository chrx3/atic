/** Transcripción: la del archivo terminado y la que llega en vivo. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { Segment, TranscribeProgress, Transcript } from "$core/types";
import { on } from "./events";

export const transcribeRecording = (id: string) =>
  invoke<void>("transcribe_recording", { id });
export const getTranscript = (id: string) =>
  invoke<Transcript | null>("get_transcript", { id });
export const saveTranscript = (id: string, transcript: Transcript) =>
  invoke<void>("save_transcript", { id, transcript });

export const onTranscribeProgress = (
  cb: (p: TranscribeProgress) => void,
): Promise<UnlistenFn> => on("transcribe-progress", cb);

export const onTranscriptReady = (cb: (id: string) => void): Promise<UnlistenFn> =>
  on("transcript-ready", (p) => cb(p.id));

export const onTranscribeError = (
  cb: (id: string, message: string) => void,
): Promise<UnlistenFn> => on("transcribe-error", (p) => cb(p.id, p.message));

// --- En vivo ---
export const onLiveTranscriptPartial = (
  cb: (segment: Segment) => void,
): Promise<UnlistenFn> => on("live-transcript-partial", cb);

export const onLiveTranscriptFinal = (
  cb: (segment: Segment) => void,
): Promise<UnlistenFn> => on("live-transcript-final", cb);

export const onLiveTranscriptError = (
  cb: (message: string) => void,
): Promise<UnlistenFn> => on("live-transcript-error", (p) => cb(p.message));
