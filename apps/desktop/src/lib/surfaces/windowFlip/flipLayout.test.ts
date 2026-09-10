import { describe, expect, it } from "vitest";
import {
  ajustarFuente,
  altoEnvolvente,
  borrarLineaCercana,
  borrarPuntos,
  clampZoom,
  colocarSiHaceFalta,
  conAlfa,
  envolver,
  leerPayloadFlip,
  lineasPagina,
  PAGINA_H,
  PAGINA_W,
  payloadFlip,
  puntoEnPapel,
  tamanoImagen,
  TEXTO_FUENTE,
  TEXTO_FUENTE_MIN,
  TEXTO_PAD_X,
  TEXTO_PAD_Y,
  ZOOM_MAX,
  ZOOM_MIN,
} from "./flipLayout";
import type { InkStroke, NoteBlock } from "$core/types";

/** Medidor falso: ancho proporcional al texto. Suficiente y sin canvas. */
const medir = (s: string, tamano: number) => s.length * tamano * 0.5;

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

  it("la imagen pegada se acota al ancho de la página", () => {
    expect(tamanoImagen(2000, 1000).w).toBeLessThan(2000);
    expect(tamanoImagen(2000, 1000).w).toBeLessThanOrEqual(PAGINA_W);
  });

  it("borrar por donde pasa corta el trazo en dos y suelta los tramos cortos", () => {
    const trazo: InkStroke = {
      color: "#e5483f",
      width: 2.6,
      points: [
        [0, 0],
        [10, 0],
        [20, 0],
        [30, 0],
        [40, 0],
        [50, 0],
      ],
    };
    const sinPunto = borrarPuntos([trazo], 25, 0, 5);
    expect(sinPunto).toHaveLength(2);
    expect(sinPunto[0]?.points).toEqual([
      [0, 0],
      [10, 0],
    ]);
    expect(sinPunto[1]?.points).toEqual([
      [40, 0],
      [50, 0],
    ]);
    const vacio = borrarPuntos([trazo], 20, 0, 5);
    expect(vacio).toHaveLength(2);
    const soloUno = borrarPuntos([trazo], 10, 0, 5);
    expect(soloUno).toHaveLength(1);
  });

  it("las líneas de página son interiores y caen en la grilla", () => {
    const una = lineasPagina({ ox: 0, oy: 0, w: PAGINA_W, h: PAGINA_H });
    expect(una.verticales).toEqual([]);
    expect(una.horizontales).toEqual([]);
    const { verticales, horizontales } = lineasPagina({
      ox: -PAGINA_W,
      oy: 0,
      w: PAGINA_W * 2,
      h: PAGINA_H,
    });
    expect(verticales).toEqual([PAGINA_W]);
    expect(horizontales).toEqual([]);
  });

  it("el resaltador tiñe el hex con alfa y deja lo demás igual", () => {
    expect(conAlfa("#e5483f")).toBe("#e5483f66");
    expect(conAlfa("#e5483f", "40")).toBe("#e5483f40");
    expect(conAlfa("#e5483f66")).toBe("#e5483f66");
    expect(conAlfa("red")).toBe("red");
  });

  it("el clic borra la línea entera más cercana y respeta la distancia", () => {
    const a: InkStroke = {
      color: "#e5483f",
      width: 2.6,
      points: [
        [0, 0],
        [10, 0],
      ],
    };
    const b: InkStroke = {
      color: "#3f7355",
      width: 2.6,
      points: [
        [0, 100],
        [10, 100],
      ],
    };
    const cerca = borrarLineaCercana([a, b], 5, 2, 16);
    expect(cerca.borro).toBe(true);
    expect(cerca.trazos).toEqual([b]);
    const lejos = borrarLineaCercana([a, b], 5, 60, 16);
    expect(lejos.borro).toBe(false);
    expect(lejos.trazos).toHaveLength(2);
  });

  it("el payload del cajón parte tipo e id aunque el id tenga dos puntos", () => {
    expect(leerPayloadFlip(payloadFlip("cap", "18:10.png"))).toEqual({
      tipo: "cap",
      id: "18:10.png",
    });
    expect(leerPayloadFlip("nope")).toBeNull();
    expect(leerPayloadFlip("ink:abc")).toBeNull();
  });
});

describe("envolver", () => {
  it("corta al ancho que le da el medidor", () => {
    const linea = (s: string) => s.length * 10;
    expect(envolver("aaaa bbbb cccc", 100, linea)).toEqual(["aaaa bbbb", "cccc"]);
  });

  it("devuelve una línea vacía cuando no hay texto", () => {
    expect(envolver("   ", 100, () => 0)).toEqual([""]);
  });
});

describe("altoEnvolvente", () => {
  it("crece con las líneas y arranca en el padding", () => {
    const una = altoEnvolvente("uno", 560, TEXTO_FUENTE, medir);
    const tres = altoEnvolvente("uno\ndos\ntres", 560, TEXTO_FUENTE, medir);
    expect(una).toBeCloseTo(TEXTO_PAD_Y + TEXTO_FUENTE * 1.5, 5);
    expect(tres).toBeGreaterThan(una);
  });

  it("el ancho útil descuenta el padding del recuadro", () => {
    const justo = medir("ab ab", TEXTO_FUENTE) + TEXTO_PAD_X; // los dos "ab" entran en una línea
    expect(altoEnvolvente("ab ab", justo, TEXTO_FUENTE, medir)).toBeCloseTo(
      TEXTO_PAD_Y + TEXTO_FUENTE * 1.5,
      5,
    );
    expect(altoEnvolvente("ab ab", justo - 1, TEXTO_FUENTE, medir)).toBeCloseTo(
      TEXTO_PAD_Y + 2 * TEXTO_FUENTE * 1.5,
      5,
    );
  });

  it("respeta los saltos de línea explícitos", () => {
    expect(altoEnvolvente("a\nb", 560, TEXTO_FUENTE, medir)).toBeCloseTo(
      TEXTO_PAD_Y + 2 * TEXTO_FUENTE * 1.5,
      5,
    );
  });
});

describe("ajustarFuente", () => {
  it("no toca el tamaño si el texto entra", () => {
    expect(ajustarFuente("hola", 560, 200, medir)).toBe(TEXTO_FUENTE);
  });

  it("encoge cuando la caja es más chica que el texto", () => {
    const largo = "palabra ".repeat(14);
    const chico = ajustarFuente(largo, 240, 96, medir);
    expect(chico).toBeLessThan(TEXTO_FUENTE);
    expect(chico).toBeGreaterThanOrEqual(TEXTO_FUENTE_MIN);
    // y lo que devuelve de verdad entra
    expect(altoEnvolvente(largo, 240, chico, medir)).toBeLessThanOrEqual(96);
  });

  it("no baja del mínimo legible aunque no entre", () => {
    const enorme = "palabra ".repeat(400);
    expect(ajustarFuente(enorme, 140, 60, medir)).toBe(TEXTO_FUENTE_MIN);
  });

  it("sin texto o sin caja se queda en el tamaño base", () => {
    expect(ajustarFuente("", 240, 96, medir)).toBe(TEXTO_FUENTE);
    expect(ajustarFuente("hola", 0, 0, medir)).toBe(TEXTO_FUENTE);
  });

  it("una palabra sola no se envuelve: el ajuste mira el alto, no el ancho", () => {
    // El textarea del navegador parte palabras largas, así que la caja angosta
    // no es motivo para encoger: lo que manda es cuántas líneas pide el cuerpo.
    expect(ajustarFuente("palabralarguisima", 60, 200, medir)).toBe(TEXTO_FUENTE);
  });
});
