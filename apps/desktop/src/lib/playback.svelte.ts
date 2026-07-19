import { trackSrc } from "$lib/api";
import type { Recording, Speaker } from "$lib/types";

export type AudioTrack = "mic" | "system";

class PlaybackController {
  recordingId = $state<string | null>(null);
  track = $state<AudioTrack | null>(null);
  label = $state<string | null>(null);
  playing = $state(false);
  loading = $state(false);
  currentTime = $state(0);
  duration = $state(0);
  error = $state<string | null>(null);

  private audio: HTMLAudioElement | null = null;
  private requestId = 0;

  private ensureAudio(): HTMLAudioElement {
    if (this.audio) return this.audio;

    const audio = new Audio();
    audio.preload = "metadata";
    audio.addEventListener("play", () => (this.playing = true));
    audio.addEventListener("pause", () => (this.playing = false));
    audio.addEventListener("timeupdate", () => {
      this.currentTime = audio.currentTime;
    });
    audio.addEventListener("durationchange", () => {
      this.duration = Number.isFinite(audio.duration) ? audio.duration : 0;
    });
    audio.addEventListener("ended", () => {
      this.playing = false;
      this.currentTime = 0;
    });
    audio.addEventListener("error", () => {
      this.loading = false;
      this.playing = false;
      this.error = "No se pudo reproducir esta pista.";
    });
    this.audio = audio;
    return audio;
  }

  private async waitForMetadata(audio: HTMLAudioElement): Promise<void> {
    if (audio.readyState >= HTMLMediaElement.HAVE_METADATA) return;

    await new Promise<void>((resolve, reject) => {
      const cleanup = () => {
        audio.removeEventListener("loadedmetadata", onLoaded);
        audio.removeEventListener("error", onError);
      };
      const onLoaded = () => {
        cleanup();
        resolve();
      };
      const onError = () => {
        cleanup();
        reject(new Error("No se pudo cargar el audio."));
      };
      audio.addEventListener("loadedmetadata", onLoaded, { once: true });
      audio.addEventListener("error", onError, { once: true });
    });
  }

  isActive(recordingId: string, track: AudioTrack): boolean {
    return this.recordingId === recordingId && this.track === track;
  }

  async play(
    recording: Recording,
    track: AudioTrack,
    startAtSeconds?: number,
  ): Promise<void> {
    const audio = this.ensureAudio();
    const sameSource = this.isActive(recording.id, track);
    const currentRequest = ++this.requestId;
    this.error = null;

    try {
      if (!sameSource) {
        audio.pause();
        this.loading = true;
        const source = await trackSrc(recording.id, track);
        if (currentRequest !== this.requestId) return;
        audio.src = source;

        this.recordingId = recording.id;
        this.track = track;
        this.label = `${recording.title} · ${track === "mic" ? "Yo" : "Otros"}`;
        this.currentTime = 0;
        this.duration = 0;
        await this.waitForMetadata(audio);
        if (currentRequest !== this.requestId) return;
      }

      if (typeof startAtSeconds === "number") {
        audio.currentTime = Math.max(0, startAtSeconds);
        this.currentTime = audio.currentTime;
      }

      this.loading = false;
      await audio.play();
    } catch (error) {
      this.loading = false;
      if (currentRequest !== this.requestId) return;
      const message = String(error);
      if (!message.includes("AbortError")) {
        this.error = message || "No se pudo reproducir esta pista.";
      }
    }
  }

  async playSpeaker(
    recording: Recording,
    speaker: Speaker,
    startAtSeconds?: number,
  ): Promise<void> {
    let track: AudioTrack = speaker === "me" ? "mic" : "system";
    if (track === "mic" && !recording.mic_path) track = "system";
    if (track === "system" && !recording.system_path) track = "mic";
    await this.play(recording, track, startAtSeconds);
  }

  async toggle(): Promise<void> {
    const audio = this.ensureAudio();
    if (!audio.src) return;
    try {
      if (audio.paused) await audio.play();
      else audio.pause();
    } catch (error) {
      const message = String(error);
      if (!message.includes("AbortError")) this.error = message;
    }
  }

  seek(seconds: number): void {
    const audio = this.ensureAudio();
    if (!audio.src) return;
    audio.currentTime = Math.max(0, Math.min(seconds, this.duration || seconds));
    this.currentTime = audio.currentTime;
  }

  stop(): void {
    const audio = this.ensureAudio();
    this.requestId += 1;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
    this.recordingId = null;
    this.track = null;
    this.label = null;
    this.currentTime = 0;
    this.duration = 0;
    this.loading = false;
    this.error = null;
  }
}

export const playback = new PlaybackController();
