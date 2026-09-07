import { describe, expect, it } from "vitest";
import {
  altoPapel,
  clampZoom,
  colocarSiHaceFalta,
  PAPEL_MIN_ALTO,
  puntoEnPapel,
  tamanoImagen,
  ZOOM_MAX,
  ZOOM_MIN,
} from "./flipLayout";
import type { NoteBlock } from "$core/types";

describe("flipLayout", () => {
  it("apila notas viejas sin marco y no toca las colocadas", () => {
    const vieja: NoteBlock = { kind: "text", id: "a", body: "hola" };
    const puesta: NoteBlock = {
      kind: "text",
      id: "b",
      body: "ya",
      x: 40,
      y: 80,
      w: 200,
      h: 60,
    };
    const out = colocarSiHaceFalta([vieja, puesta]);
    expect(out[0]?.w).toBeGreaterThan(0);
    expect(out[0]?.y).toBe(24);
    expect(out[1]).toMatchObject({ x: 40, y: 80, w: 200, h: 60 });
  });

  it("no reordena si todos ya tienen marco", () => {
    const lista: NoteBlock[] = [
      { kind: "text", id: "a", body: "", x: 10, y: 10, w: 100, h: 40 },
    ];
    expect(colocarSiHaceFalta(lista)).toEqual(lista);
  });

  it("clampa el zoom y traduce un clic al papel", () => {
    expect(clampZoom(0.1)).toBe(ZOOM_MIN);
    expect(clampZoom(9)).toBe(ZOOM_MAX);
    const papel = { left: 100, top: 50, width: 320, height: 260 } as DOMRect;
    const p = puntoEnPapel(papel, 180, 50, 640);
    expect(p.x).toBeCloseTo(160);
    expect(p.y).toBeCloseTo(0);
  });

  it("el papel crece con el bloque más bajo", () => {
    const lista: NoteBlock[] = [
      { kind: "text", id: "a", body: "", x: 0, y: 700, w: 100, h: 80 },
    ];
    expect(altoPapel(lista)).toBeGreaterThan(PAPEL_MIN_ALTO);
    expect(tamanoImagen(2000, 1000).w).toBeLessThan(2000);
  });
});
