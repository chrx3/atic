import { afterEach, describe, expect, it, vi } from "vitest";
import {
  formatColor,
  formatHsl,
  hsvToRgb,
  inkOn,
  parseCssColor,
  parseHex,
  rgbToHex,
  rgbToHsv,
  roseSwatch,
  loadRecentColors,
  pushRecentColor,
  normalizeRecentColors,
} from "./colorMath";

describe("colorMath", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("reads the colors a clipboard entry can actually hold", () => {
    const red = { r: 255, g: 0, b: 0 };
    expect(parseCssColor("#ff0000")).toEqual(red);
    expect(parseCssColor("  #F00 ")).toEqual(red);
    expect(parseCssColor("ff0000")).toEqual(red);
    expect(parseCssColor("#ff0000ff")).toEqual(red);
    expect(parseCssColor("rgb(255, 0, 0)")).toEqual(red);
    expect(parseCssColor("rgb(255 0 0)")).toEqual(red);
    expect(parseCssColor("rgba(255, 0, 0, 0.5)")).toEqual(red);
    expect(parseCssColor("rgb(100%, 0%, 0%)")).toEqual(red);
    expect(parseCssColor("hsl(0, 100%, 50%)")).toEqual(red);
    expect(parseCssColor("hsl(220, 16%, 22%)")).toEqual({ r: 47, g: 53, b: 65 });
  });

  it("leaves ordinary text without a swatch", () => {
    for (const text of [
      "",
      "   ",
      "Texto copiado",
      // Sin almohadilla, seis dígitos es un número tan a menudo como un color.
      "123",
      "12345678",
      // El color tiene que ser TODO el texto, no una palabra suelta dentro.
      "el fondo es #ff0000 y el borde no",
      "rgb(300, 0, 0)",
      "rgb(255, 0)",
      "hsl(0, 120%, 50%)",
      "rgb(a, b, c)",
      "#ff00zz",
      "#".padEnd(40, "a"),
    ]) {
      expect(parseCssColor(text), text).toBeNull();
    }
  });

  it("deduplicates stored colors before rendering keyed swatches", () => {
    expect(
      normalizeRecentColors(["#ff0000", "FF0000", null, "bad", "#00ff00"]),
    ).toEqual(["#FF0000", "#00FF00"]);
  });

  it("keeps session history when storage cannot be written", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new Error("quota");
      },
    });
    const first = pushRecentColor("#FF0000");
    expect(pushRecentColor("#0000FF", first)).toEqual(["#0000FF", "#FF0000"]);
  });

  it("recovers from corrupt storage and caps recent colors at eight", () => {
    vi.stubGlobal("localStorage", { getItem: () => "broken", setItem: vi.fn() });
    expect(loadRecentColors()).toEqual([]);
    expect(
      normalizeRecentColors(
        Array.from({ length: 12 }, (_, r) => rgbToHex({ r, g: 0, b: 0 })),
      ),
    ).toHaveLength(8);
  });

  it("chooses dark ink for saturated red and green", () => {
    expect(inkOn({ r: 255, g: 0, b: 0 })).toBe("#111");
    expect(inkOn({ r: 0, g: 255, b: 0 })).toBe("#111");
    expect(inkOn({ r: 0, g: 0, b: 255 })).toBe("#fff");
  });
  it("redondea HEX en mayúsculas", () => {
    expect(rgbToHex({ r: 255, g: 0, b: 128 })).toBe("#FF0080");
    expect(parseHex("#ff0080")).toEqual({ r: 255, g: 0, b: 128 });
    expect(parseHex("nope")).toBeNull();
  });

  it("HSV ida y vuelta conserva primarios", () => {
    const red = { r: 255, g: 0, b: 0 };
    expect(hsvToRgb(rgbToHsv(red))).toEqual(red);
    expect(hsvToRgb({ h: 120, s: 1, v: 1 })).toEqual({ r: 0, g: 255, b: 0 });
    expect(hsvToRgb({ h: 240, s: 1, v: 1 })).toEqual({ r: 0, g: 0, b: 255 });
  });

  it("formatea RGB y HSL para el portapapeles", () => {
    const cyan = { r: 0, g: 255, b: 255 };
    expect(formatColor(cyan, "hex")).toBe("#00FFFF");
    expect(formatColor(cyan, "rgb")).toBe("rgb(0, 255, 255)");
    expect(formatHsl(cyan)).toBe("hsl(180, 100%, 50%)");
  });

  it("elige tinta contrastada sobre el swatch", () => {
    expect(inkOn({ r: 255, g: 255, b: 255 })).toBe("#111");
    expect(inkOn({ r: 0, g: 0, b: 0 })).toBe("#fff");
  });

  it("la rosa de 0° es rojo puro", () => {
    expect(roseSwatch(0)).toEqual({ r: 255, g: 0, b: 0 });
  });
});
