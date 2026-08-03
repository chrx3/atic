/**
 * La grabación en curso: si hay una, cuánto lleva y qué se está oyendo.
 *
 * Estaba duplicado entre la ventana principal y la pill, cada una con su copia
 * del cronómetro y de los niveles, y las dos escuchando los mismos eventos. Es
 * el caso que mejor muestra por qué el estado tiene que vivir fuera de la
 * vista: no es que se repitiera código, es que había dos relojes.
 */

import { isRecording, startRecording, stopRecording } from "$ipc/recordings";
import { subscribe } from "$ipc/events";
import type { Levels, MeetingDetectionPayload, Segment } from "$core/types";
import { toasts } from "./toasts.svelte";
import type { DomainStore } from "./store";

class CaptureStore implements DomainStore {
  active = $state(false);
  /** Segundos desde que arrancó. */
  elapsed = $state(0);
  levels = $state<Levels>({ mic: 0, system: 0 });

  /** Transcripción en vivo, si está activada. */
  segments = $state<Segment[]>([]);
  partial = $state<Segment | null>(null);
  liveError = $state<string | null>(null);

  /** Reunión detectada por el sistema, si la hay. */
  meeting = $state<MeetingDetectionPayload | null>(null);
  /** Aviso no fatal del arranque (p. ej. degradación Bluetooth). */
  note = $state<string | null>(null);

  /** Un comando en vuelo: evita que dos clics arranquen dos grabaciones. */
  busy = $state(false);

  #timer: ReturnType<typeof setInterval> | null = null;
  #startedAt = 0;

  async hydrate(): Promise<void> {
    this.active = await isRecording();
    // El cronómetro arranca de cero aunque la grabación venga de antes: Rust
    // no informa desde cuándo, y mostrar un tiempo inventado sería peor.
    if (this.active) this.#startTimer();
  }

  async listen(): Promise<() => void> {
    const stop = await subscribe({
      "recording-status": (status) => {
        this.active = status.active;
        if (status.active) {
          this.#startTimer();
          this.segments = [];
          this.partial = null;
          this.liveError = null;
        } else {
          this.#stopTimer();
          this.partial = null;
          this.note = null;
        }
      },
      "audio-levels": (next) => (this.levels = next),
      "live-transcript-final": (segment) => {
        this.segments = [...this.segments, segment];
        this.partial = null;
      },
      "live-transcript-partial": (segment) => (this.partial = segment),
      "live-transcript-error": (p) => {
        this.liveError = p.message;
        toasts.push(p.message);
      },
      "capture-error": (p) => toasts.push(p.message),
      "capture-warn": (p) => {
        this.note = p.message;
        toasts.push(p.message, 9000);
      },
      "meeting-detection": (m) => (this.meeting = m.active ? m : null),
    });

    return () => {
      stop();
      this.#stopTimer();
    };
  }

  /** Arranca o para, según corresponda. Devuelve si quedó grabando. */
  async toggle(allowBluetoothHandsFree = false): Promise<boolean> {
    if (this.busy) return this.active;
    this.busy = true;
    try {
      if (this.active) {
        await stopRecording();
        return false;
      }
      await startRecording(allowBluetoothHandsFree);
      return true;
    } finally {
      this.busy = false;
    }
  }

  #startTimer(): void {
    this.#stopTimer();
    this.#startedAt = Date.now();
    this.elapsed = 0;
    this.#timer = setInterval(() => {
      this.elapsed = Math.floor((Date.now() - this.#startedAt) / 1000);
    }, 500);
  }

  #stopTimer(): void {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
  }
}

export const capture = new CaptureStore();
