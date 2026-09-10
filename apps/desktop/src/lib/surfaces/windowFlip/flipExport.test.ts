import { describe, expect, it } from "vitest";
import type { NoteBlock } from "$core/types";
import { bloqueEnPagina, marcoEnPagina, PAGINA_W } from "./flipLayout";
import {
  armarExport,
  distribuirChecks,
  distribuirTexto,
  estructuraPagina,
  lineasDePagina,
  ordenDePintura,
} from "./flipExport";

describe("flipExport", () => {
  it("asigna un bloque a la celda donde está su marco", () => {
    const nota: NoteBlock = {
      kind: "text",
      id: "a",
      body: "hola",
      x: PAGINA_W + 20,
      y: 40,
      w: 200,
      h: 80,
    };
    expect(bloqueEnPagina(nota, 0)).toBe(false);
    expect(bloqueEnPagina(nota, 1)).toBe(true);
  });

  it("saca el texto de la página para Word/PDF", () => {
    const bloques: NoteBlock[] = [
      { kind: "text", id: "t", body: "Resumen", x: 24, y: 24, w: 200, h: 80 },
      {
        kind: "check",
        id: "c",
        items: [{ id: "i", text: "Mail", done: true }],
        x: PAGINA_W + 24,
        y: 24,
        w: 200,
        h: 80,
      },
    ];
    expect(lineasDePagina(bloques, 0)).toEqual(["Resumen"]);
    expect(lineasDePagina(bloques, 1)).toEqual(["[x] Mail"]);
  });

  it("recorta un bloque a la celda y arma texto nativo", () => {
    const bloques: NoteBlock[] = [
      { kind: "text", id: "t", body: "Hola", x: 24, y: 24, w: 200, h: 80 },
      {
        kind: "image",
        id: "f",
        asset: "clip.png",
        width: 100,
        height: 80,
        x: 40,
        y: 120,
        w: 160,
        h: 90,
      },
    ];
    const pagina = estructuraPagina(bloques, 0);
    expect(pagina.textos[0]?.body).toBe("Hola");
    expect(pagina.fotos[0]?.asset).toBe("clip.png");
    expect(marcoEnPagina(-20, 10, 80, 40, 0)).toEqual({
      x: 0,
      y: 10,
      w: 60,
      h: 40,
    });
  });
});

describe("ordenDePintura", () => {
  it("pinta la tinta al final, sobre fotos y texto", () => {
    const bloques: NoteBlock[] = [
      {
        kind: "ink",
        id: "k",
        height: 800,
        strokes: [
          {
            color: "#000000",
            width: 2,
            points: [
              [10, 10],
              [20, 20],
            ],
          },
        ],
      },
      { kind: "text", id: "t", body: "hola", x: 24, y: 24, w: 200, h: 80 },
      {
        kind: "image",
        id: "im",
        asset: "a.png",
        width: 10,
        height: 10,
        x: 40,
        y: 120,
        w: 100,
        h: 80,
      },
    ];
    expect(ordenDePintura(bloques, 0).map((o) => o.tipo)).toEqual([
      "text",
      "image",
      "ink",
    ]);
  });

  it("solo incluye los bloques presentes en la celda pedida", () => {
    const bloques: NoteBlock[] = [
      { kind: "text", id: "t", body: "x", x: PAGINA_W + 20, y: 20, w: 100, h: 50 },
    ];
    expect(ordenDePintura(bloques, 0)).toEqual([]);
    expect(ordenDePintura(bloques, 1).map((o) => o.tipo)).toEqual(["text"]);
  });
});

describe("distribuirTexto", () => {
  it("envuelve al ancho del medidor inyectado", () => {
    const medir = (s: string) => s.length * 10; // 10px por caracter
    const marco = { x: 0, y: 0, w: 116, h: 1000 }; // ancho útil 100 → 10 chars
    expect(distribuirTexto("aaaa bbbb cccc", marco, medir)).toEqual([
      { texto: "aaaa bbbb", x: 8, y: 10 },
      { texto: "cccc", x: 8, y: 30 },
    ]);
  });

  it("respeta los saltos de línea explícitos del cuerpo", () => {
    const marco = { x: 0, y: 0, w: 1000, h: 1000 };
    expect(distribuirTexto("uno\ndos", marco, () => 0)).toEqual([
      { texto: "uno", x: 8, y: 10 },
      { texto: "dos", x: 8, y: 30 },
    ]);
  });

  it("recorta cuando el texto desborda el fondo del marco", () => {
    const marco = { x: 0, y: 0, w: 1000, h: 40 }; // fondo = 32
    expect(distribuirTexto("a\nb\nc\nd", marco, () => 0)).toEqual([
      { texto: "a", x: 8, y: 10 },
      { texto: "b", x: 8, y: 30 },
    ]);
  });
});

describe("distribuirChecks", () => {
  it("avanza cada ítem y conserva el estado done", () => {
    const marco = { x: 0, y: 0, w: 200, h: 1000 };
    const items = [
      { text: "uno", done: false },
      { text: "dos", done: true },
    ];
    expect(distribuirChecks(items, marco)).toEqual([
      { text: "uno", done: false, x: 24, y: 10, caja: { x: 8, y: 12, w: 10, h: 10 } },
      { text: "dos", done: true, x: 24, y: 32, caja: { x: 8, y: 34, w: 10, h: 10 } },
    ]);
  });

  it("recorta los ítems que caen fuera del marco", () => {
    const marco = { x: 0, y: 0, w: 200, h: 36 }; // fondo = 28
    const items = [
      { text: "a", done: false },
      { text: "b", done: false },
    ];
    expect(distribuirChecks(items, marco)).toHaveLength(1);
  });
});

describe("armarExport", () => {
  const foto = (id: string, asset: string, x: number): NoteBlock => ({
    kind: "image",
    id,
    asset,
    width: 10,
    height: 10,
    x,
    y: 20,
    w: 100,
    h: 80,
  });

  it("reporta los assets que no se pudieron leer en vez de tragarlos", async () => {
    const bloques: NoteBlock[] = [foto("f1", "ok.png", 20), foto("f2", "bad.png", 140)];
    const dataDe = (asset: string) => {
      if (asset === "bad.png") return Promise.reject(new Error("sin lectura"));
      return Promise.resolve("data:image/png;base64,QUJD");
    };
    const rasterFalso = () =>
      Promise.resolve([{ base64: "PREV", width: 10, height: 10 }]);
    const res = await armarExport(
      bloques,
      PAGINA_W,
      "#fff",
      dataDe,
      "png",
      rasterFalso,
    );
    expect(res.fallos).toEqual(["bad.png"]);
    expect(res.paginas[0]?.fotos.map((f) => f.imageBase64)).toEqual(["QUJD"]);
  });

  it("usa el camino nativo para pdf/docx/pptx (sin rasterizar)", async () => {
    const bloques: NoteBlock[] = [
      { kind: "text", id: "t", body: "Hola", x: 24, y: 24, w: 200, h: 80 },
    ];
    for (const formato of ["pdf", "docx", "pptx"] as const) {
      let llamado = 0;
      const rasterFalso = () => {
        llamado++;
        return Promise.resolve([]);
      };
      const res = await armarExport(
        bloques,
        PAGINA_W,
        "#fff",
        () => Promise.resolve(""),
        formato,
        rasterFalso,
      );
      expect(llamado).toBe(0);
      expect(res.paginas[0]?.previewBase64).toBe("");
      expect(res.paginas[0]?.textos[0]?.body).toBe("Hola");
    }
  });

  it("usa el camino rasterizado para png/jpeg (preview poblado)", async () => {
    const bloques: NoteBlock[] = [
      { kind: "text", id: "t", body: "Hola", x: 24, y: 24, w: 200, h: 80 },
    ];
    const rasterFalso = () => Promise.resolve([{ base64: "IMG", width: 2, height: 2 }]);
    const res = await armarExport(
      bloques,
      PAGINA_W,
      "#fff",
      () => Promise.resolve(""),
      "png",
      rasterFalso,
    );
    expect(res.paginas[0]?.previewBase64).toBe("IMG");
  });
});
