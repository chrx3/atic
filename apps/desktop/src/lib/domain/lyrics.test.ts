import { describe, expect, it } from "vitest";
import { lyricIndex, parseLrc, withoutTranslation } from "./lyrics";

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

describe("withoutTranslation", () => {
  it("una letra con la traducción en cada verso queda en el original", () => {
    // Así viene NUEVAYoL en LRCLIB.
    const lines = parseLrc(
      "[00:00.12] ¡NUEVAYoL!^NEW YORK!\n[00:20.75] Si te quieres divertir^If you want to have fun",
    );
    expect(lines.map((l) => l.text)).toEqual(["¡NUEVAYoL!", "Si te quieres divertir"]);
    const semicolon = parseLrc(
      "[00:01.00]Hola mi amor; Hello my love\n[00:02.00]Te extraño; I miss you\n[00:03.00]",
    );
    expect(semicolon.map((l) => l.text)).toEqual(["Hola mi amor", "Te extraño", ""]);
  });

  it("un «;» de puntuación en algunos versos no la toca", () => {
    const lines = parseLrc(
      "[00:01.00]Por la mañana, café; por la tarde, ron\n[00:02.00]Otra\n[00:03.00]Más",
    );
    expect(withoutTranslation(lines)).toEqual(lines);
    expect(lines[0].text).toBe("Por la mañana, café; por la tarde, ron");
  });
});
