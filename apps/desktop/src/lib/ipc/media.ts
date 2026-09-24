/** Lo que suena en el equipo (SMTC en Windows) y sus controles. */

import { invoke } from "@tauri-apps/api/core";

export type MediaNow = {
  title: string;
  artist: string;
  /** De qué app viene, ya legible. */
  app: string;
  playing: boolean;
  can_toggle: boolean;
  can_next: boolean;
  can_prev: boolean;
  /** Si la app deja mover la posición del tema. */
  can_seek: boolean;
  /**
   * Carátula como data URL. Falta si la app no la publica, o si ya la tienes:
   * se manda solo cuando `thumb_key` cambia (ver `mediaNow`).
   */
  thumbnail?: string | null;
  /** Identifica el tema: cambia cuando cambia la carátula. */
  thumb_key: string;
  position_ms?: number | null;
  duration_ms?: number | null;
  /** Cuándo midió la app `position_ms` (ms Unix). */
  updated_ms?: number | null;
};

export type MediaAction = "toggle" | "next" | "prev";

/**
 * `null` = no suena nada (o está detenido).
 *
 * `known` es el `thumb_key` que ya se tiene: con el mismo tema, la carátula no
 * viaja de nuevo (pesa cientos de KB y esto se pregunta cada segundo y medio).
 */
export const mediaNow = (known: string | null) =>
  invoke<MediaNow | null>("media_now", { known });

/** El volumen que mueve el reproductor: el de la app, o el del equipo. */
export type MediaVolume = {
  level: number;
  /** `false` = la app no se pudo identificar y se mueve el del equipo. */
  app: boolean;
};

export const mediaVolume = () => invoke<MediaVolume>("media_volume");

export const mediaSetVolume = (volume: number) =>
  invoke<void>("media_set_volume", { volume });

export const mediaControl = (action: MediaAction) =>
  invoke<boolean>("media_control", { action });

/** Lleva el tema a `positionMs`, contado desde el inicio. */
export const mediaSeek = (positionMs: number) =>
  invoke<boolean>("media_seek", { positionMs: Math.max(0, Math.round(positionMs)) });

/** Trae al frente la app que suena. `false` si no se dio con su ventana. */
export const mediaFocus = () => invoke<boolean>("media_focus");

/**
 * La letra sincronizada (LRC) del tema, o `null` si no se encontró. Sale de
 * LRCLIB: viajan título, artista y duración. Rust la recuerda por tema.
 */
export const mediaLyrics = (title: string, artist: string, durationMs: number | null) =>
  invoke<string | null>("media_lyrics", { title, artist, durationMs });
