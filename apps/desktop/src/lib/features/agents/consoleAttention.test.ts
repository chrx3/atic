import { describe, expect, it } from "vitest";
import { attentionDue } from "./consoleAttention";

const BASE = {
  visible: false,
  idleMs: 12_000,
  lastSeenVisibleAt: 0,
  sent: new Set<string>(),
};

function session(id: string, lastOutputAt: number | null, label = id) {
  return { id, label, lastOutputAt };
}

describe("attentionDue", () => {
  it("avisa una racha que enmudeció fuera de vista", () => {
    const out = attentionDue({
      ...BASE,
      now: 20_000,
      sessions: [session("a", 5_000, "OpenCode")],
    });
    expect(out).toEqual([{ id: "a", label: "OpenCode", lastOutputAt: 5_000 }]);
  });

  it("no avisa dos veces la misma racha", () => {
    const sent = new Set(["a"]);
    const out = attentionDue({
      ...BASE,
      now: 60_000,
      sent,
      sessions: [session("a", 5_000)],
    });
    expect(out).toEqual([]);
  });

  it("no avisa lo que llegó antes de mirar por última vez", () => {
    const out = attentionDue({
      ...BASE,
      now: 20_000,
      lastSeenVisibleAt: 10_000,
      sessions: [session("a", 5_000)],
    });
    expect(out).toEqual([]);
  });

  it("no avisa antes del silencio mínimo ni sin output", () => {
    const out = attentionDue({
      ...BASE,
      now: 10_000,
      sessions: [session("a", 5_000), session("b", null)],
    });
    expect(out).toEqual([]);
  });

  it("a la vista no avisa nada", () => {
    const out = attentionDue({
      ...BASE,
      visible: true,
      now: 60_000,
      sessions: [session("a", 5_000)],
    });
    expect(out).toEqual([]);
  });

  it("avisa cada sesión enmudecida, una vez", () => {
    const out = attentionDue({
      ...BASE,
      now: 30_000,
      sessions: [session("a", 5_000), session("b", 6_000)],
    });
    expect(out.map((s) => s.id)).toEqual(["a", "b"]);
  });
});
