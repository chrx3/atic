import { describe, expect, it } from "vitest";
import { lyricIndex, parseLrc } from "./lyrics";

describe("parseLrc", () => {
  it("lee marcas, ignora cabeceras y ordena versos repetidos", () => {
    const lines = parseLrc(
      "[ar:Alguien]\n[00:12.50]Hola\r\n[00:05.00][01:00.25]Coro\n[00:20.00]\nsin marca",
    );
    expect(lines).toEqual([
      { at: 5000, text: "Coro" },
      { at: 12500, text: "Hola" },
      { at: 20000, text: "" },
      { at: 60250, text: "Coro" },
    ]);
  });
});

describe("lyricIndex", () => {
  const lines = parseLrc("[00:01.00]a\n[00:03.00]b\n[00:05.00]c");

  it("da la última línea que ya empezó", () => {
    expect(lyricIndex(lines, 500)).toBe(-1);
    expect(lyricIndex(lines, 1000)).toBe(0);
    expect(lyricIndex(lines, 4999)).toBe(1);
    expect(lyricIndex(lines, 90_000)).toBe(2);
  });
});
