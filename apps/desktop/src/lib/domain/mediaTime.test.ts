import { describe, expect, it } from "vitest";

import type { MediaNow } from "$ipc/media";
import { mediaClock, mediaPosition } from "./mediaTime";

const base: MediaNow = {
  title: "Tema",
  artist: "Artista",
  app: "Spotify",
  playing: true,
  can_toggle: true,
  can_next: true,
  can_prev: true,
  can_seek: true,
  position_ms: 60_000,
  duration_ms: 180_000,
  updated_ms: 1_000_000,
  thumb_key: "Spotify|Tema|Artista",
};

describe("mediaPosition", () => {
  it("sonando, avanza desde la última medición", () => {
    expect(mediaPosition(base, 1_005_000)).toBe(65_000);
  });

  it("en pausa, se queda donde la app la dejó", () => {
    expect(mediaPosition({ ...base, playing: false }, 1_005_000)).toBe(60_000);
  });

  it("no se pasa del final", () => {
    expect(mediaPosition(base, 9_000_000)).toBe(180_000);
  });

  it("sin duración no hay barra", () => {
    expect(mediaPosition({ ...base, duration_ms: null }, 1_005_000)).toBeNull();
  });
});

describe("mediaClock", () => {
  it("minutos y segundos", () => {
    expect(mediaClock(187_000)).toBe("3:07");
  });

  it("con horas", () => {
    expect(mediaClock(3_765_000)).toBe("1:02:45");
  });
});
