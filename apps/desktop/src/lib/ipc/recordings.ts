/** Grabación de reuniones: el ciclo de captura y los archivos que deja. */

import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  ExportResult,
  Levels,
  MeetingDetectionPayload,
  Recording,
  StatusPayload,
} from "$core/types";
import { on } from "./events";

export const startRecording = (allowBluetoothHandsFree = false) =>
  invoke<Recording>("start_recording", { allowBluetoothHandsFree });
export const stopRecording = () => invoke<Recording>("stop_recording");
export const isRecording = () => invoke<boolean>("is_recording");
export const listRecordings = () => invoke<Recording[]>("list_recordings");
export const deleteRecording = (id: string) => invoke<void>("delete_recording", { id });
export const renameRecording = (id: string, title: string) =>
  invoke<void>("rename_recording", { id, title });

/** Importa archivos de audio externos como grabaciones (una pista mic.wav). */
export const importAudio = (paths: string[]) =>
  invoke<Recording[]>("import_audio", { paths });

export const exportRecording = (
  id: string,
  format: ExportResult["format"],
  path: string,
) => invoke<ExportResult>("export_recording", { id, format, path });

const trackPath = (id: string, track: "mic" | "system") =>
  invoke<string>("recording_track_path", { id, track });

/** Devuelve una URL reproducible (asset://) para una pista de audio. */
export async function trackSrc(id: string, track: "mic" | "system"): Promise<string> {
  return convertFileSrc(await trackPath(id, track));
}

export const onLevels = (cb: (levels: Levels) => void): Promise<UnlistenFn> =>
  on("audio-levels", cb);

export const onStatus = (cb: (status: StatusPayload) => void): Promise<UnlistenFn> =>
  on("recording-status", cb);

export const onRecordingsChanged = (cb: () => void): Promise<UnlistenFn> =>
  on("recordings-changed", cb);

export const onCaptureError = (cb: (message: string) => void): Promise<UnlistenFn> =>
  on("capture-error", (p) => cb(p.message));

/** Aviso no fatal al iniciar captura (p. ej. degradación Bluetooth). */
export const onCaptureWarn = (cb: (message: string) => void): Promise<UnlistenFn> =>
  on("capture-warn", (p) => cb(p.message));

export const onMeetingDetection = (
  cb: (meeting: MeetingDetectionPayload) => void,
): Promise<UnlistenFn> => on("meeting-detection", cb);
