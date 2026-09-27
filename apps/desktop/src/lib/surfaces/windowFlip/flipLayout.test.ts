import { describe, expect, it } from "vitest";
import {
  ajustarFuente,
  altoEnvolvente,
  borrarLineaCercana,
  borrarPuntos,
  cajaTrazo,
  clampZoom,
  contenidoDePagina,
  enRecuadro,
  grupoCompleto,
  quitarGrupo,
  quitarPaginaDe,
  escalarTrazo,
  medidaTexto,
  moverTrazo,
  TEXTO_ALTO_MIN,
  TEXTO_ANCHO_MAX,
  TEXTO_ANCHO_MIN,
  trazoEnPunto,
  colocarSiHaceFalta,
  conAlfa,
  envolver,
  leerPayloadFlip,
  lineasPagina,
  miniaturasTablero,
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

  it("reparte cada bloque en la miniatura de su página", () => {
    const bloques: NoteBlock[] = [
      { kind: "text", id: "t", body: "hola", x: 60, y: 90, w: 600, h: 48 },
      {
        kind: "image",
        id: "i",
        asset: "img.png",
        width: 10,
        height: 10,
        x: PAGINA_W + 120,
        y: 0,
        w: 240,
        h: 180,
      },
    ];
    const [primera, segunda] = miniaturasTablero(bloques, 2);
    expect(primera.piezas.map((p) => p.id)).toEqual(["t"]);
    expect(primera.piezas[0]).toMatchObject({ x: 5, y: 10, w: 50, renglones: 2 });
    expect(segunda.piezas).toEqual([
      expect.objectContaining({ id: "i", asset: "img.png", x: 10, w: 20, h: 20 }),
    ]);
  });
});

describe("trazos como objetos", () => {
  const recta = {
    color: "#000",
    width: 4,
    points: [
      [0, 0],
      [100, 0],
    ] as [number, number][],
  };
  const encima = {
    color: "#f00",
    width: 2,
    points: [
      [50, -20],
      [50, 20],
    ] as [number, number][],
  };

  it("toca un trazo entre sus puntos, no solo sobre ellos", () => {
    expect(trazoEnPunto([recta], 50, 3, 2)).toBe(0);
    expect(trazoEnPunto([recta], 50, 12, 2)).toBe(-1);
  });

  it("gana el trazo de más arriba, como con los objetos", () => {
    expect(trazoEnPunto([recta, encima], 50, 0, 2)).toBe(1);
  });

  it("la caja cuenta el grosor", () => {
    expect(cajaTrazo(recta)).toEqual({ x: -2, y: -2, w: 104, h: 4 });
  });

  it("mover corre todos los puntos sin tocar el original", () => {
    expect(moverTrazo(recta, 10, 5).points).toEqual([
      [10, 5],
      [110, 5],
    ]);
    expect(recta.points).toEqual([
      [0, 0],
      [100, 0],
    ]);
  });

  it("escalar deja quieta la esquina y escala también el grosor", () => {
    const doble = escalarTrazo(recta, 2);
    expect(doble.points).toEqual([
      [0, 0],
      [200, 0],
    ]);
    expect(doble.width).toBe(8);
    expect(escalarTrazo(recta, 0).width).toBeGreaterThan(0);
  });
});

describe("selección por recuadro", () => {
  const tablero = (): NoteBlock[] => [
    { kind: "text", id: "dentro", body: "", x: 10, y: 10, w: 100, h: 40 },
    { kind: "text", id: "borde", body: "", x: 180, y: 10, w: 100, h: 40 },
    { kind: "text", id: "lejos", body: "", x: 900, y: 600, w: 100, h: 40 },
    {
      kind: "ink",
      id: "t",
      strokes: [
        {
          color: "#000",
          width: 2,
          points: [
            [50, 150],
            [60, 160],
          ],
        },
        // Su caja cruza el recuadro, pero ningún punto cae adentro.
        {
          color: "#000",
          width: 2,
          points: [
            [0, 500],
            [800, 500],
          ],
        },
      ],
      height: 0,
    },
  ];
  const recuadro = { x: 0, y: 0, w: 200, h: 200 };

  it("elige objetos que tocan el recuadro y trazos con puntos adentro", () => {
    expect(enRecuadro(tablero(), recuadro)).toEqual({
      ids: ["dentro", "borde"],
      trazos: [0],
    });
  });

  it("quitar el grupo deja lo demás y no muta", () => {
    const antes = tablero();
    const despues = quitarGrupo(antes, enRecuadro(antes, recuadro));
    expect(despues.filter((b) => b.kind === "text").map((b) => b.id)).toEqual([
      "lejos",
    ]);
    const tinta = despues.find((b) => b.kind === "ink");
    expect(tinta?.kind === "ink" && tinta.strokes.length).toBe(1);
    expect(antes).toHaveLength(4);
  });

  it("Ctrl+A elige todo", () => {
    expect(grupoCompleto(tablero())).toEqual({
      ids: ["dentro", "borde", "lejos"],
      trazos: [0, 1],
    });
  });
});

describe("eliminar una página", () => {
  const trazo = (x: number) => ({
    color: "#000",
    width: 2,
    points: [
      [x, 10],
      [x + 20, 10],
    ] as [number, number][],
  });
  const tablero = (): NoteBlock[] => [
    { kind: "text", id: "a", body: "uno", x: 100, y: 10, w: 200, h: 50 },
    // Cruza el borde, pero su centro cae en la página 2: es de la 2.
    { kind: "text", id: "b", body: "dos", x: PAGINA_W - 50, y: 10, w: 200, h: 50 },
    { kind: "text", id: "c", body: "tres", x: PAGINA_W * 2 + 10, y: 10, w: 100, h: 50 },
    {
      kind: "ink",
      id: "t",
      strokes: [trazo(10), trazo(PAGINA_W + 10), trazo(PAGINA_W * 2 + 10)],
      height: 0,
    },
  ];

  it("cuenta lo que se perdería, asignando lo que cruza por su centro", () => {
    expect(contenidoDePagina(tablero(), 0)).toEqual({ objetos: 1, trazos: 1 });
    expect(contenidoDePagina(tablero(), 1)).toEqual({ objetos: 1, trazos: 1 });
  });

  it("quita lo de la página y corre lo de la derecha, sin tocar la izquierda", () => {
    const antes = tablero();
    const despues = quitarPaginaDe(antes, 1);
    expect(despues.filter((b) => b.kind === "text").map((b) => [b.id, b.x])).toEqual([
      ["a", 100],
      ["c", PAGINA_W + 10],
    ]);
    const tinta = despues.find((b) => b.kind === "ink");
    expect(tinta?.kind === "ink" && tinta.strokes.map((s) => s.points[0]?.[0])).toEqual(
      [10, PAGINA_W + 10],
    );
    // No muta: el historial guarda el estado de antes.
    expect(antes.find((b) => b.id === "c")?.x).toBe(PAGINA_W * 2 + 10);
  });
});

describe("medidaTexto", () => {
  // Monoespaciado de mentira: cada carácter mide medio tamaño de fuente.
  const medir = (s: string, tamano: number) => s.length * tamano * 0.5;

  it("una caja vacía es el mínimo de ancho y un renglón de alto", () => {
    const vacia = medidaTexto("", medir);
    expect(vacia.w).toBe(TEXTO_ANCHO_MIN);
    expect(vacia.h).toBeGreaterThanOrEqual(TEXTO_ALTO_MIN);
    expect(vacia.h).toBeLessThan(TEXTO_ALTO_MIN + TEXTO_FUENTE);
  });

  it("se ensancha con el renglón más largo", () => {
    const corto = medidaTexto("hola", medir).w;
    const largo = medidaTexto(
      "hola\nun renglón bastante más largo que el otro",
      medir,
    ).w;
    expect(largo).toBeGreaterThan(corto);
  });

  it("pasado el tope envuelve y crece hacia abajo", () => {
    const una = medidaTexto("a", medir);
    const mucha = medidaTexto("palabra ".repeat(200), medir);
    expect(mucha.w).toBe(TEXTO_ANCHO_MAX);
    expect(mucha.h).toBeGreaterThan(una.h);
  });
});
