import { describe, expect, it } from "vitest";
import {
  dateBucket,
  groupByDate,
  neighbourAfterClose,
  parseWorkspace,
  type WorkspaceItem,
} from "./agentWorkspace";

describe("parseWorkspace", () => {
  it("lee chats y terminales, y el elegido", () => {
    const raw = JSON.stringify({
      items: [
        { key: "a", kind: "chat", session: "s1", label: "Claude Code", cli: "claude" },
        {
          key: "b",
          kind: "terminal",
          session: "p1",
          label: "Codex",
          cli: "codex",
          command: "codex",
          cwd: "C:/repo",
        },
      ],
      active: "b",
    });
    const state = parseWorkspace(raw);
    expect(state.items.map((i) => i.kind)).toEqual(["chat", "terminal"]);
    expect(state.active).toBe("b");
  });

  it("descarta lo que no se puede retomar y corrige el elegido", () => {
    const raw = JSON.stringify({
      items: [
        { key: "x", kind: "terminal", session: null, label: "sin abrir" },
        { key: "y", kind: "chat", session: "s2", label: "OpenCode" },
        { kind: "chat", session: "sin-key" },
      ],
      active: "x",
    });
    const state = parseWorkspace(raw);
    expect(state.items.map((i) => i.key)).toEqual(["y"]);
    expect(state.active).toBe("y");
  });

  it("vacío o roto no rompe", () => {
    expect(parseWorkspace(null)).toEqual({ items: [], active: null });
    expect(parseWorkspace("{roto")).toEqual({ items: [], active: null });
  });
});

describe("neighbourAfterClose", () => {
  const items = ["a", "b", "c"].map((key): WorkspaceItem => ({
    key,
    kind: "chat",
    session: key,
    label: key,
    cli: null,
  }));

  it("cerrar el elegido pasa al de al lado", () => {
    expect(neighbourAfterClose(items, "b", "b")).toBe("c");
    expect(neighbourAfterClose(items, "c", "c")).toBe("b");
  });

  it("cerrar otro no cambia el elegido; cerrar el último deja nada", () => {
    expect(neighbourAfterClose(items, "a", "c")).toBe("c");
    expect(neighbourAfterClose(items.slice(0, 1), "a", "a")).toBeNull();
  });
});

describe("dateBucket", () => {
  const now = new Date(2026, 8, 23, 10, 0);
  const at = (d: Date) => d.getTime() / 1000;

  it("usa días de calendario, no horas", () => {
    expect(dateBucket(at(new Date(2026, 8, 23, 0, 5)), now)).toBe("today");
    expect(dateBucket(at(new Date(2026, 8, 22, 23, 50)), now)).toBe("yesterday");
    expect(dateBucket(at(new Date(2026, 8, 18, 12, 0)), now)).toBe("thisWeek");
    expect(dateBucket(at(new Date(2026, 8, 16, 12, 0)), now)).toBe("older");
  });

  it("agrupa en orden y sin tramos vacíos", () => {
    const groups = groupByDate(
      [
        { id: "a", updatedAt: at(new Date(2026, 8, 23, 9, 0)) },
        { id: "b", updatedAt: at(new Date(2026, 8, 1, 9, 0)) },
        { id: "c", updatedAt: at(new Date(2026, 8, 23, 8, 0)) },
      ],
      now,
    );
    expect(groups.map((g) => [g.bucket, g.items.map((i) => i.id)])).toEqual([
      ["today", ["a", "c"]],
      ["older", ["b"]],
    ]);
  });
});

describe("posición en la pizarra", () => {
  it("guarda el rectángulo de cada terminal y descarta uno roto", () => {
    const raw = JSON.stringify({
      items: [
        {
          key: "a",
          kind: "terminal",
          session: "p1",
          label: "Claude",
          cli: "claude",
          command: "claude",
          cwd: null,
          rect: { x: 10, y: 20, w: 800, h: 500 },
        },
        {
          key: "b",
          kind: "terminal",
          session: "p2",
          label: "Codex",
          cli: "codex",
          command: "codex",
          cwd: null,
          rect: { x: "no" },
        },
      ],
      active: "a",
    });
    const [a, b] = parseWorkspace(raw).items;
    expect(a.kind === "terminal" && a.rect).toEqual({ x: 10, y: 20, w: 800, h: 500 });
    expect(b.kind === "terminal" && b.rect).toBeUndefined();
  });
});
