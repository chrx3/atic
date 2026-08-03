/**
 * Escuchar una pista de una grabación.
 *
 * Es un `<audio>` único para toda la app, no uno por componente: dos pistas
 * sonando a la vez nunca es lo que se quiere, y tener el elemento acá hace que
 * elegir un fragmento en la transcripción y el reproductor de abajo sean lo
 * mismo sin que ninguno de los dos sepa del otro.
 *
 * No implementa `DomainStore` porque no proyecta nada de Rust: no hay estado
 * que hidratar ni eventos que escuchar. Solo pide la ruta del archivo.
 */

import { trackSrc } from "$ipc/recordings";
import type { Recording, Speaker } from "$core/types";

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

  #audio: HTMLAudioElement | null = null;

  /**
   * Cada carga lleva número.
   *
   * Pedir la ruta a Rust es asíncrono, así que dos clics seguidos en pistas
   * distintas pueden resolverse al revés. Comparar el número contra el actual
   * descarta la respuesta que ya no importa.
   */
  #request = 0;

  #ensure(): HTMLAudioElement {
    if (this.#audio) return this.#audio;

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
    this.#audio = audio;
    return audio;
  }

  /** Sin metadatos no se puede saltar a un segundo concreto. */
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
    const audio = this.#ensure();
    const sameSource = this.isActive(recording.id, track);
    const request = ++this.#request;
    this.error = null;

    try {
      // Volver a cargar la misma pista reiniciaría la descarga y perdería la
      // posición: saltar dentro de lo que ya suena es solo mover el cabezal.
      if (!sameSource) {
        audio.pause();
        this.loading = true;
        const source = await trackSrc(recording.id, track);
        if (request !== this.#request) return;
        audio.src = source;

        this.recordingId = recording.id;
        this.track = track;
        this.label = `${recording.title} · ${track === "mic" ? "Yo" : "Otros"}`;
        this.currentTime = 0;
        this.duration = 0;
        await this.#waitForMetadata(audio);
        if (request !== this.#request) return;
      }

      if (typeof startAtSeconds === "number") {
        audio.currentTime = Math.max(0, startAtSeconds);
        this.currentTime = audio.currentTime;
      }

      this.loading = false;
      await audio.play();
    } catch (error) {
      this.loading = false;
      if (request !== this.#request) return;
      const message = String(error);
      // `AbortError` es lo que tira el navegador cuando una reproducción se
      // interrumpe por otra: es el flujo normal, no un fallo que mostrar.
      if (!message.includes("AbortError")) {
        this.error = message || "No se pudo reproducir esta pista.";
      }
    }
  }

  /**
   * La pista que corresponde a quién habla, con la que haya.
   *
   * Una grabación importada tiene una sola pista, así que pedir la del
   * micrófono cuando solo existe la del sistema tiene que sonar igual en vez
   * de fallar.
   */
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
    const audio = this.#ensure();
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
    const audio = this.#ensure();
    if (!audio.src) return;
    audio.currentTime = Math.max(0, Math.min(seconds, this.duration || seconds));
    this.currentTime = audio.currentTime;
  }

  stop(): void {
    const audio = this.#ensure();
    // Sube el número para que cualquier carga en vuelo se descarte al volver.
    this.#request += 1;
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
