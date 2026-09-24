import { describe, expect, it } from "vitest";
import data from "./emojiData.json";
import {
  moveInGrid,
  normalizeEmojiText,
  parseEmojiRows,
  searchEmojis,
  withSkin,
  type EmojiRow,
} from "./emoji";

const rows = data.rows as EmojiRow[];
const mac = parseEmojiRows(rows, "macos");
const win = parseEmojiRows(rows, "windows");
const chars = (query: string) => searchEmojis(mac, query).map((e) => e.char);

describe("searchEmojis", () => {
  it("encuentra por nombre en español y en inglés", () => {
    expect(chars("fuego")[0]).toBe("🔥");
    expect(chars("fire")[0]).toBe("🔥");
  });

  it("ignora tildes y mayúsculas", () => {
    expect(normalizeEmojiText("Corazón")).toBe("corazon");
    expect(chars("CORAZON ROJO")[0]).toBe("❤️");
  });

  it("usa las palabras clave, no solo el nombre", () => {
    expect(chars("like")).toContain("👍️");
  });

  it("exige que calcen todas las palabras", () => {
    const hits = searchEmojis(mac, "gato feliz");
    expect(hits.length).toBeGreaterThan(0);
    expect(hits.every((e) => e.names.some((n) => n.includes("gato")))).toBe(true);
  });

  it("nada que calce → vacío", () => {
    expect(searchEmojis(mac, "zzqqxx")).toEqual([]);
    expect(searchEmojis(mac, "   ")).toEqual([]);
  });
});

describe("parseEmojiRows", () => {
  it("en Windows saca las banderas de país y lo que Segoe no dibuja", () => {
    expect(mac.some((e) => e.char === "🇨🇱")).toBe(true);
    expect(win.some((e) => e.char === "🇨🇱")).toBe(false);
    expect(win.some((e) => e.char === "🏁")).toBe(true);
    expect(win.length).toBeLessThan(mac.length);
  });
});

describe("withSkin", () => {
  const thumbs = mac.find((e) => e.nameEn === "thumbs up")!;

  it("aplica el tono elegido y 0 deja el amarillo", () => {
    expect(withSkin(thumbs, 0)).toBe(thumbs.char);
    expect(withSkin(thumbs, 3)).toBe("👍🏽");
  });

  it("un emoji sin tonos queda igual", () => {
    const fire = mac.find((e) => e.char === "🔥")!;
    expect(withSkin(fire, 4)).toBe("🔥");
  });
});

describe("moveInGrid", () => {
  // Dos secciones: 10 emojis (filas de 4, 4, 2) y 5 (filas de 4, 1).
  const sizes = [10, 5];

  it("izquierda/derecha avanzan en plano y se frenan en los bordes", () => {
    expect(moveInGrid(sizes, 0, "ArrowLeft", 4)).toBe(0);
    expect(moveInGrid(sizes, 9, "ArrowRight", 4)).toBe(10);
    expect(moveInGrid(sizes, 14, "ArrowRight", 4)).toBe(14);
  });

  it("abajo conserva la columna y salta de sección", () => {
    expect(moveInGrid(sizes, 1, "ArrowDown", 4)).toBe(5);
    expect(moveInGrid(sizes, 5, "ArrowDown", 4)).toBe(9);
    expect(moveInGrid(sizes, 9, "ArrowDown", 4)).toBe(11);
  });

  it("si la fila destino es corta, cae en su último emoji", () => {
    expect(moveInGrid(sizes, 7, "ArrowDown", 4)).toBe(9);
    expect(moveInGrid(sizes, 12, "ArrowUp", 4)).toBe(9);
  });

  it("arriba desde la primera fila no se mueve; secciones vacías se saltan", () => {
    expect(moveInGrid(sizes, 2, "ArrowUp", 4)).toBe(2);
    expect(moveInGrid([0, 3], 1, "ArrowUp", 4)).toBe(1);
    expect(moveInGrid([2, 0, 3], 1, "ArrowDown", 4)).toBe(3);
  });
});
