import { describe, expect, it } from "vitest";
import {
  QUIET_MS,
  TURN_MS,
  consoleState,
  needsAttention,
  noteOutput,
} from "./consoleStatus";

describe("noteOutput", () => {
  it("la racha sigue mientras la salida no se corte", () => {
    const a = noteOutput(undefined, 1000);
    const b = noteOutput(a, 1000 + QUIET_MS - 1);
    expect(b).toEqual({ last: 1000 + QUIET_MS - 1, since: 1000 });
  });

  it("después de un silencio empieza otra", () => {
    const a = noteOutput(undefined, 1000);
    expect(noteOutput(a, 1000 + QUIET_MS + 1).since).toBe(1000 + QUIET_MS + 1);
  });
});

describe("consoleState", () => {
  const base = { ended: false, presence: null, activity: undefined, now: 10_000 };

  it("la presencia manda sobre la salida", () => {
    const busy = { last: 10_000, since: 9_000 };
    expect(consoleState({ ...base, presence: "waiting", activity: busy })).toBe(
      "waiting",
    );
    expect(consoleState({ ...base, presence: "ready", activity: busy })).toBe("ready");
  });

  it("sin presencia, escribir es trabajar y callarse es estar lista", () => {
    expect(consoleState({ ...base, activity: { last: 9_500, since: 5_000 } })).toBe(
      "working",
    );
    expect(consoleState({ ...base, activity: { last: 5_000, since: 1_000 } })).toBe(
      "ready",
    );
    expect(consoleState(base)).toBe("ready");
  });

  it("terminada es terminada, diga lo que diga lo demás", () => {
    expect(consoleState({ ...base, ended: true, presence: "working" })).toBe("ended");
  });
});

describe("needsAttention", () => {
  const done = { prev: "working", next: "ready", watched: false } as const;

  it("un turno largo que termina fuera de la vista pide atención", () => {
    expect(needsAttention({ ...done, turnMs: TURN_MS })).toBe(true);
  });

  it("un repintado corto no", () => {
    expect(needsAttention({ ...done, turnMs: 400 })).toBe(false);
  });

  it("lo que se está mirando no avisa", () => {
    expect(needsAttention({ ...done, turnMs: 60_000, watched: true })).toBe(false);
  });

  it("un permiso avisa siempre, aunque el turno haya sido corto", () => {
    expect(
      needsAttention({ prev: "working", next: "waiting", turnMs: 0, watched: false }),
    ).toBe(true);
  });
});
