import { describe, expect, it } from "vitest";
import { chatTabsKey, parseChatTabs } from "./chatTabs";

describe("parseChatTabs", () => {
  it("lee lo guardado", () => {
    const raw = JSON.stringify([
      { session: "a", label: "Claude Code", command: "claude" },
    ]);
    expect(parseChatTabs(raw)).toEqual([
      { session: "a", label: "Claude Code", command: "claude" },
    ]);
  });

  it("vacío o roto no rompe: no hay nada que retomar", () => {
    expect(parseChatTabs(null)).toEqual([]);
    expect(parseChatTabs("{no es json")).toEqual([]);
    expect(parseChatTabs(JSON.stringify({ session: "a" }))).toEqual([]);
  });

  it("descarta entradas sin sesión y completa lo que falte", () => {
    const raw = JSON.stringify([null, { label: "x" }, { session: "b" }]);
    expect(parseChatTabs(raw)).toEqual([{ session: "b", label: "", command: null }]);
  });
});

describe("chatTabsKey", () => {
  it("una clave por lanzador", () => {
    expect(chatTabsKey("island")).not.toBe(chatTabsKey("overlay"));
  });
});
