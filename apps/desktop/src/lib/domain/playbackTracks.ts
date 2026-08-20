import type { Recording } from "$core/types";
import { formatLocale } from "$core/format";
import { translate } from "$core/i18n/translate";

export type AudioTrack = "mic" | "system" | "mix";

type RecordingTracks = Pick<Recording, "mic_path" | "system_path">;

/** Qué pista tiene sentido para esta grabación. */
export function defaultTrack(recording: RecordingTracks): AudioTrack {
  if (recording.mic_path && recording.system_path) return "mix";
  if (recording.system_path && !recording.mic_path) return "system";
  return "mic";
}

/**
 * Si piden una pista que no existe, cae a la que sí.
 *
 * Una importación suele tener solo mic; una captura de PC, solo sistema.
 */
export function resolveTrack(
  recording: RecordingTracks,
  wanted: AudioTrack,
): AudioTrack {
  const hasMic = Boolean(recording.mic_path);
  const hasSys = Boolean(recording.system_path);
  if (wanted === "mix") {
    if (hasMic && hasSys) return "mix";
    if (hasSys) return "system";
    return "mic";
  }
  if (wanted === "mic" && !hasMic) return hasSys ? "system" : "mic";
  if (wanted === "system" && !hasSys) return hasMic ? "mic" : "system";
  return wanted;
}

export function trackLabel(track: AudioTrack): string {
  if (track === "mic") return translate(formatLocale(), "page.meetings.me");
  if (track === "system") return translate(formatLocale(), "page.meetings.others");
  return translate(formatLocale(), "page.meetings.all");
}

export function kindsFor(track: AudioTrack): Array<"mic" | "system"> {
  return track === "mix" ? ["mic", "system"] : [track];
}

export function listenOptions(
  recording: RecordingTracks,
): { value: AudioTrack; label: string }[] {
  const options: { value: AudioTrack; label: string }[] = [];
  if (recording.mic_path && recording.system_path) {
    options.push({
      value: "mix",
      label: translate(formatLocale(), "page.meetings.all"),
    });
  }
  if (recording.mic_path) {
    options.push({
      value: "mic",
      label: translate(formatLocale(), "page.meetings.me"),
    });
  }
  if (recording.system_path) {
    options.push({
      value: "system",
      label: translate(formatLocale(), "page.meetings.others"),
    });
  }
  return options;
}
