/**
 * El reproductor de la pill: qué suena y cómo controlarlo.
 *
 * Se sondea mientras alguien lo mira (`watch`), no siempre: el SO no avisa por
 * un canal que llegue barato al webview, y preguntar cada segundo y medio
 * cuesta poco mientras la carátula no cambie (Rust la guarda por tema).
 */
import {
  mediaControl,
  mediaFocus,
  mediaLyrics,
  mediaNow,
  mediaSeek,
  mediaSetVolume,
  mediaVolume,
  type MediaAction,
  type MediaNow,
  type MediaVolume,
} from "$ipc/media";
import { parseLrc, type LyricLine } from "./lyrics";
import { mediaPosition } from "./mediaTime";

const POLL_MS = 1500;
/** Tras un control, la app tarda un poco en publicar el estado nuevo. */
const SETTLE_MS = 350;
/**
 * Tras un salto, lo que dice la app puede ser anterior a él (Spotify publica
 * la posición cada varios segundos): la barra se queda donde se soltó.
 */
const SEEK_HOLD_MS = 2500;

class MediaStore {
  now = $state<MediaNow | null>(null);
  /** Solo se lee mientras el vistazo está abierto (`watchVolume`). */
  volume = $state<MediaVolume | null>(null);
  /** Letra sincronizada del tema actual; `null` si no hay (o aún no llega). */
  lyrics = $state<LyricLine[] | null>(null);

  #watchers = 0;
  #timer = 0;
  #inFlight = false;
  /** La carátula del tema actual: Rust no la reenvía si no cambió. */
  #thumb: { key: string; data: string | null } | null = null;
  #volumeTimer = 0;
  /** Último valor pedido mientras otro está en vuelo: gana el más nuevo. */
  #pendingVolume: number | null = null;
  #settingVolume = false;
  /** Tras mover el slider, el sondeo no pisa la mano con un valor viejo. */
  #volumeHeldUntil = 0;
  /** El salto pedido, mientras el sondeo aún no lo refleja. */
  #seek: { position: number; at: number; until: number } | null = null;
  /** De qué tema es la letra pedida: una respuesta tardía no pisa la del siguiente. */
  #lyricsKey: string | null = null;

  async refresh(): Promise<void> {
    if (this.#inFlight) return;
    this.#inFlight = true;
    try {
      const next = await mediaNow(this.#thumb?.key ?? null);
      if (next) {
        if (this.#thumb?.key === next.thumb_key) {
          next.thumbnail = this.#thumb.data;
        } else {
          this.#thumb = { key: next.thumb_key, data: next.thumbnail ?? null };
        }
      }
      const seek = this.#seek;
      if (next && seek && Date.now() < seek.until) {
        next.position_ms = seek.position;
        next.updated_ms = seek.at;
      } else {
        this.#seek = null;
      }
      this.now = next;
      this.#loadLyrics(next);
    } catch {
      // Fuera de Tauri o sin SMTC: no hay reproductor, y eso no es un error.
      this.now = null;
    } finally {
      this.#inFlight = false;
    }
  }

  #loadLyrics(now: MediaNow | null): void {
    const key = now?.title ? `${now.title}${now.artist}` : null;
    if (key === this.#lyricsKey) return;
    this.#lyricsKey = key;
    this.lyrics = null;
    if (!now || !key) return;
    mediaLyrics(now.title, now.artist, now.duration_ms ?? null)
      .then((lrc) => {
        if (this.#lyricsKey !== key) return;
        const lines = lrc ? parseLrc(lrc) : [];
        this.lyrics = lines.length > 0 ? lines : null;
      })
      .catch(() => {
        // Sin red o LRCLIB caído: el tema suena igual, solo que sin letra.
      });
  }

  /** Empieza a sondear. Devuelve la función que deja de hacerlo. */
  watch(): () => void {
    this.#watchers += 1;
    if (this.#watchers === 1) {
      void this.refresh();
      this.#timer = window.setInterval(() => void this.refresh(), POLL_MS);
    }
    return () => {
      this.#watchers -= 1;
      if (this.#watchers === 0) {
        window.clearInterval(this.#timer);
        this.#timer = 0;
      }
    };
  }

  /** Lee el volumen mientras alguien lo muestra. Devuelve cómo dejar de hacerlo. */
  watchVolume(): () => void {
    const read = async () => {
      if (Date.now() < this.#volumeHeldUntil) return;
      try {
        this.volume = await mediaVolume();
      } catch {
        this.volume = null;
      }
    };
    void read();
    this.#volumeTimer = window.setInterval(() => void read(), POLL_MS);
    return () => {
      window.clearInterval(this.#volumeTimer);
      this.#volumeTimer = 0;
    };
  }

  /**
   * Mueve el volumen. Un arrastre dispara un valor por píxel: se manda el
   * primero y, al volver, el último que haya llegado, sin encolar los del medio.
   */
  async setVolume(level: number): Promise<void> {
    this.#volumeHeldUntil = Date.now() + POLL_MS;
    if (this.volume) this.volume = { ...this.volume, level };
    this.#pendingVolume = level;
    if (this.#settingVolume) return;
    this.#settingVolume = true;
    try {
      while (this.#pendingVolume != null) {
        const value = this.#pendingVolume;
        this.#pendingVolume = null;
        await mediaSetVolume(value).catch(() => {});
      }
    } finally {
      this.#settingVolume = false;
    }
  }

  /** Lleva el tema a `position` (ms). La barra se mueve al toque. */
  async seek(position: number): Promise<void> {
    const current = this.now;
    if (!current?.can_seek) return;
    const duration = current.duration_ms ?? 0;
    const target = Math.round(Math.min(Math.max(0, position), duration || position));
    const at = Date.now();
    this.#seek = { position: target, at, until: at + SEEK_HOLD_MS };
    this.now = { ...current, position_ms: target, updated_ms: at };
    try {
      await mediaSeek(target);
    } catch {
      // Igual que los otros controles: el próximo sondeo corrige.
    }
  }

  /** Adelanta o retrocede `delta` ms desde donde va ahora. */
  seekBy(delta: number): Promise<void> {
    const current = this.now;
    if (!current) return Promise.resolve();
    const position = mediaPosition(current, Date.now()) ?? current.position_ms ?? 0;
    return this.seek(position + delta);
  }

  /** Trae al frente la app que suena. `false` si no se dio con ella. */
  async focus(): Promise<boolean> {
    return mediaFocus().catch(() => false);
  }

  async control(action: MediaAction): Promise<void> {
    const current = this.now;
    // Optimista: el botón cambia al toque, no cuando la app se entere.
    if (action === "toggle" && current) {
      const at = Date.now();
      this.now = {
        ...current,
        playing: !current.playing,
        // La posición de ahora, no la de la última muestra: si no, pausar
        // haría retroceder la barra hasta donde la app la midió.
        position_ms: mediaPosition(current, at) ?? current.position_ms,
        updated_ms: at,
      };
    }
    try {
      await mediaControl(action);
    } catch {
      // La app pudo cerrarse entre el sondeo y el clic: el próximo lo corrige.
    }
    window.setTimeout(() => void this.refresh(), SETTLE_MS);
  }
}

export const media = new MediaStore();
