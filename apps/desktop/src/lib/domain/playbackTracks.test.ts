import { describe, expect, it } from "vitest";
import {
  defaultTrack,
  kindsFor,
  listenOptions,
  resolveTrack,
  trackLabel,
} from "./playbackTracks";

const both = { mic_path: "mic.wav", system_path: "sys.wav" };
const micOnly = { mic_path: "mic.wav", system_path: null };
const sysOnly = { mic_path: null, system_path: "sys.wav" };

describe("defaultTrack", () => {
  it("mezcla cuando hay las dos pistas", () => {
    expect(defaultTrack(both)).toBe("mix");
  });

  it("cae a la pista que existe", () => {
    expect(defaultTrack(micOnly)).toBe("mic");
    expect(defaultTrack(sysOnly)).toBe("system");
  });
});

describe("resolveTrack", () => {
  it("deja Todos si hay mic y sistema", () => {
    expect(resolveTrack(both, "mix")).toBe("mix");
  });

  it("no pide una pista que no está", () => {
    expect(resolveTrack(micOnly, "mix")).toBe("mic");
    expect(resolveTrack(micOnly, "system")).toBe("mic");
    expect(resolveTrack(sysOnly, "mix")).toBe("system");
    expect(resolveTrack(sysOnly, "mic")).toBe("system");
  });
});

describe("listenOptions", () => {
  it("ofrece Todos, Yo y Otros cuando hay las dos", () => {
    expect(listenOptions(both).map((o) => o.value)).toEqual(["mix", "mic", "system"]);
  });

  it("no ofrece Todos si falta una pista", () => {
    expect(listenOptions(micOnly).map((o) => o.value)).toEqual(["mic"]);
    expect(listenOptions(sysOnly).map((o) => o.value)).toEqual(["system"]);
  });
});

describe("trackLabel / kindsFor", () => {
  it("nombra las pistas como en la UI", () => {
    expect(trackLabel("mix")).toBe("Todos");
    expect(trackLabel("mic")).toBe("Yo");
    expect(trackLabel("system")).toBe("Otros");
  });

  it("Todos usa las dos pistas a la vez", () => {
    expect(kindsFor("mix")).toEqual(["mic", "system"]);
    expect(kindsFor("mic")).toEqual(["mic"]);
  });
});
