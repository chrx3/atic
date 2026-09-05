/**
 * Escuchar una o dos pistas de una grabación.
 *
 * El controlador es único para la app: elegir un fragmento en la transcripción
 * y el reproductor de abajo mueven el mismo cabezal.
 *
 * «Todos» mezcla micrófono y sistema a la vez (dos `<audio>` sincronizados).
 * «Yo» y «Otros» aíslan una pista.
 */

import { t } from "$domain/i18n.svelte";
import { trackSrc } from "$ipc/recordings";
import type { Recording, Speaker } from "$core/types";
import {
  defaultTrack,
  kindsFor,
  resolveTrack,
  type AudioTrack,
} from "./playbackTracks";

export type { AudioTrack };
export { defaultTrack, listenOptions, trackLabel } from "./playbackTracks";

const DRIFT_SEC = 0.08;

class PlaybackController {
  recordingId = $state<string | null>(null);
  track = $state<AudioTrack | null>(null);
  label = $state<string | null>(null);
  playing = $state(false);
  loading = $state(false);
  currentTime = $state(0);
  duration = $state(0);
  error = $state<string | null>(null);

  #mic: HTMLAudioElement | null = null;
  #sys: HTMLAudioElement | null = null;
  #loaded = { id: null as string | null, mic: false, sys: false };
  #seeking = false;
  /**
   * Cada carga lleva número.
   *
   * Pedir la ruta a Rust es asíncrono, así que dos clics seguidos en pistas
   * distintas pueden resolverse al revés. Comparar el número contra el actual
   * descarta la respuesta que ya no importa.
   */
  #request = 0;

  #make(kind: "mic" | "system"): HTMLAudioElement {
    const audio = new Audio();
    audio.preload = "metadata";
    audio.addEventListener("play", () => this.#syncPlaying());
    audio.addEventListener("pause", () => this.#syncPlaying());
    audio.addEventListener("timeupdate", () => this.#onTime(kind));
    audio.addEventListener("durationchange", () => this.#syncDuration());
    audio.addEventListener("ended", () => this.#onEnded(kind));
    audio.addEventListener("error", () => {
      this.loading = false;
      this.playing = false;
      this.error = t("page.meetings.playError");
    });
    return audio;
  }

  #el(kind: "mic" | "system"): HTMLAudioElement {
    if (kind === "mic") {
      if (!this.#mic) this.#mic = this.#make("mic");
      return this.#mic;
    }
    if (!this.#sys) this.#sys = this.#make("system");
    return this.#sys;
  }

  #syncPlaying(): void {
    const kinds = this.track ? kindsFor(this.track) : [];
    this.playing = kinds.some((kind) => {
      const el = kind === "mic" ? this.#mic : this.#sys;
      return Boolean(el && el.src && !el.paused && !el.ended);
    });
  }

  #syncDuration(): void {
    const kinds = this.track ? kindsFor(this.track) : [];
    let max = 0;
    for (const kind of kinds) {
      const el = kind === "mic" ? this.#mic : this.#sys;
      const d = el?.duration ?? 0;
      if (Number.isFinite(d) && d > max) max = d;
    }
    this.duration = max;
  }

  #clockKind(): "mic" | "system" {
    if (this.track === "system") return "system";
    if (this.track === "mix") {
      const mic = this.#mic;
      if (mic && mic.src && !mic.ended) return "mic";
      return "system";
    }
    return "mic";
  }

  #onTime(kind: "mic" | "system"): void {
    if (this.#seeking) return;
    if (this.track === "mix") this.#correctDrift();
    if (kind !== this.#clockKind()) return;
    const el = kind === "mic" ? this.#mic : this.#sys;
    if (!el) return;
    this.currentTime = el.currentTime;
  }

  #correctDrift(): void {
    const mic = this.#mic;
    const sys = this.#sys;
    if (!mic?.src || !sys?.src || mic.paused || sys.paused) return;
    if (Math.abs(mic.currentTime - sys.currentTime) > DRIFT_SEC) {
      sys.currentTime = mic.currentTime;
    }
  }

  #onEnded(kind: "mic" | "system"): void {
    if (this.track === "mix") {
      const other = kind === "mic" ? this.#sys : this.#mic;
      if (other && other.src && !other.paused && !other.ended) {
        this.#syncPlaying();
        return;
      }
    }
    this.playing = false;
    this.currentTime = 0;
    if (this.track) this.#seekEls(kindsFor(this.track), 0);
  }

  #seekEls(kinds: Array<"mic" | "system">, seconds: number): void {
    this.#seeking = true;
    for (const kind of kinds) {
      const el = this.#el(kind);
      if (el.src) el.currentTime = seconds;
    }
    this.currentTime = seconds;
    queueMicrotask(() => {
      this.#seeking = false;
    });
  }

  #pauseUnused(active: Array<"mic" | "system">): void {
    if (!active.includes("mic")) this.#mic?.pause();
    if (!active.includes("system")) this.#sys?.pause();
  }

  async #playKinds(kinds: Array<"mic" | "system">): Promise<void> {
    await Promise.all(
      kinds.map((kind) => {
        const el = this.#el(kind);
        return el.src ? el.play() : Promise.resolve();
      }),
    );
  }

  async #waitForMetadata(audio: HTMLAudioElement): Promise<void> {
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
        reject(new Error(t("page.meetings.loadError")));
      };
      audio.addEventListener("loadedmetadata", onLoaded, { once: true });
      audio.addEventListener("error", onError, { once: true });
    });
  }

  async #ensureSrc(id: string, kind: "mic" | "system"): Promise<HTMLAudioElement> {
    const el = this.#el(kind);
    const loadedKey = kind === "system" ? "sys" : "mic";
    if (this.#loaded.id === id && this.#loaded[loadedKey] && el.src) return el;
    const source = await trackSrc(id, kind);
    el.src = source;
    if (this.#loaded.id !== id) this.#loaded = { id, mic: false, sys: false };
    this.#loaded[loadedKey] = true;
    await this.#waitForMetadata(el);
    return el;
  }

  #clearElement(el: HTMLAudioElement | null): void {
    if (!el) return;
    el.pause();
    el.removeAttribute("src");
    el.load();
  }

  async #arm(
    recording: Recording,
    wanted: AudioTrack,
    startAtSeconds: number | undefined,
    shouldPlay: boolean,
  ): Promise<void> {
    const track = resolveTrack(recording, wanted);
    const kinds = kindsFor(track);
    const request = ++this.#request;
    this.error = null;

    const sameRec = this.recordingId === recording.id;
    const sameSource = sameRec && this.track === track;
    const keepTime =
      typeof startAtSeconds === "number"
        ? startAtSeconds
        : sameRec
          ? this.currentTime
          : 0;

    try {
      if (!sameSource) this.loading = true;

      if (!sameRec) {
        this.#clearElement(this.#mic);
        this.#clearElement(this.#sys);
        this.#loaded = { id: null, mic: false, sys: false };
      }

      for (const kind of kinds) {
        await this.#ensureSrc(recording.id, kind);
        if (request !== this.#request) return;
      }

      this.recordingId = recording.id;
      this.track = track;
      this.label = `${recording.title} · ${
        track === "mix"
          ? t("page.meetings.all")
          : track === "mic"
            ? t("page.meetings.me")
            : t("page.meetings.others")
      }`;
      this.#syncDuration();
      this.#seekEls(kinds, Math.max(0, keepTime));
      this.#pauseUnused(kinds);

      this.loading = false;
      if (shouldPlay) await this.#playKinds(kinds);
    } catch (error) {
      this.loading = false;
      if (request !== this.#request) return;
      const message = String(error);
      if (!message.includes("AbortError")) {
        this.error = message || t("page.meetings.playError");
      }
    }
  }

  isActive(recordingId: string, track: AudioTrack): boolean {
    return this.recordingId === recordingId && this.track === track;
  }

  /** Carga la pista sin reproducirla. Sirve para mostrar duración y cambiar de pista en pausa. */
  async load(
    recording: Recording,
    track: AudioTrack,
    startAtSeconds?: number,
  ): Promise<void> {
    await this.#arm(recording, track, startAtSeconds, false);
  }

  async play(
    recording: Recording,
    track: AudioTrack,
    startAtSeconds?: number,
  ): Promise<void> {
    await this.#arm(recording, track, startAtSeconds, true);
  }

  /**
   * Cambia Yo / Otros / Todos conservando la posición.
   *
   * Si ya estaba sonando, sigue; si estaba en pausa, se queda en pausa.
   */
  async switchTrack(recording: Recording, track: AudioTrack): Promise<void> {
    const resume = this.playing && this.recordingId === recording.id;
    if (resume) await this.play(recording, track);
    else await this.load(recording, track);
  }

  /**
   * El momento de un fragmento, en la pista que se está escuchando.
   *
   * Si todavía no hay pista elegida, «Todos» cuando existen las dos.
   */
  async playSpeaker(
    recording: Recording,
    _speaker: Speaker,
    startAtSeconds?: number,
  ): Promise<void> {
    const track =
      this.recordingId === recording.id && this.track
        ? this.track
        : defaultTrack(recording);
    await this.play(recording, track, startAtSeconds);
  }

  async toggle(): Promise<void> {
    if (!this.track) return;
    const kinds = kindsFor(this.track);
    try {
      if (this.playing) {
        for (const kind of kinds) this.#el(kind).pause();
      } else {
        await this.#playKinds(kinds);
      }
    } catch (error) {
      const message = String(error);
      if (!message.includes("AbortError")) this.error = message;
    }
  }

  seek(seconds: number): void {
    if (!this.track) return;
    const kinds = kindsFor(this.track);
    const t = Math.max(0, Math.min(seconds, this.duration || seconds));
    this.#seekEls(kinds, t);
  }

  stop(): void {
    this.#request += 1;
    this.#clearElement(this.#mic);
    this.#clearElement(this.#sys);
    this.#loaded = { id: null, mic: false, sys: false };
    this.recordingId = null;
    this.track = null;
    this.label = null;
    this.currentTime = 0;
    this.duration = 0;
    this.loading = false;
    this.error = null;
    this.playing = false;
  }
}

export const playback = new PlaybackController();
