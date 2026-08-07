import { describe, expect, it } from "vitest";
import { parse } from "./agentMarkdown";

describe("agentMarkdown tables", () => {
  it("parsea una tabla GFM simple", () => {
    const src = [
      "| Col A | Col B |",
      "| ----- | ----- |",
      "| 1 | dos |",
      "| tres | 4 |",
    ].join("\n");
    const blocks = parse(src);
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ kind: "table" });
    if (blocks[0].kind !== "table") return;
    expect(blocks[0].headers).toHaveLength(2);
    expect(blocks[0].rows).toHaveLength(2);
    expect(blocks[0].headers[0][0]).toEqual({ kind: "text", text: "Col A" });
    expect(blocks[0].rows[1][0][0]).toEqual({ kind: "text", text: "tres" });
  });

  it("no confunde pipes sueltos con tabla", () => {
    const blocks = parse("usa `|` en código");
    expect(blocks.some((b) => b.kind === "table")).toBe(false);
  });
});
