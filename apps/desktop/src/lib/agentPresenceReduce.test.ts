import { describe, expect, it } from "vitest";
import type { AgentPresence } from "$core/types";
import { applyPresenceSnapshot, markPresenceSeen } from "./agentPresenceReduce";

function presence(partial: Partial<AgentPresence> & Pick<AgentPresence, "id" | "status">): AgentPresence {
  return {
    backendId: "claude-code",
    backendName: "Claude Code",
    cwd: "/x",
    preview: "hola",
    updatedAt: 1,
    window: null,
    source: "jsonl",
    ...partial,
  };
}

describe("applyPresenceSnapshot", () => {
  it("sube unread al llegar ready sin mirar", () => {
    const first = applyPresenceSnapshot(
      { list: [], unread: {}, watching: false },
      [presence({ id: "s1", status: "working" })],
    );
    expect(first.unread).toEqual({});
    const second = applyPresenceSnapshot(
      { ...first, watching: false },
      [presence({ id: "s1", status: "ready" })],
    );
    expect(second.unread.s1).toBe(1);
  });

  it("no sube unread si se está mirando", () => {
    const next = applyPresenceSnapshot(
      { list: [presence({ id: "s1", status: "working" })], unread: {}, watching: true },
      [presence({ id: "s1", status: "ready" })],
    );
    expect(next.unread.s1).toBeUndefined();
  });

  it("baja unread al marcar visto", () => {
    expect(markPresenceSeen({ s1: 1 }, "s1")).toEqual({ s1: 0 });
  });
});
