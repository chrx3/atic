import { describe, expect, it } from "vitest";
import {
  DOCK_RELEASE_PX,
  DOCK_SNAP_PX,
  dockAxis,
  dockCandidate,
  dockedEdgeAt,
  dockedPoint,
  desktopBounds,
  edgeGaps,
  isOuterEdge,
  shouldUndock,
} from "./edgeDock";
import type { Area } from "$ipc/overlay";

/** Un monitor 1000×800 sin nada reservado. */
const SOLO: Area[] = [{ x: 0, y: 0, w: 1000, h: 800 }];

/** Mismo monitor con 40 px de barra de tareas abajo. */
const SOLO_TASKBAR: Area[] = [
  { x: 0, y: 0, w: 1000, h: 800, work: { x: 0, y: 0, w: 1000, h: 760 } },
];

/** Dos pantallas pegadas: el canto en x=1000 es INTERIOR. */
const DUAL: Area[] = [
  { x: 0, y: 0, w: 1000, h: 800 },
  { x: 1000, y: 0, w: 1000, h: 800 },
];

const PILL = { w: 40, h: 40 };
const at = (x: number, y: number) => ({ x, y, ...PILL });

describe("desktopBounds", () => {
  it("une todos los monitores", () => {
    expect(desktopBounds(DUAL)).toEqual({ x: 0, y: 0, w: 2000, h: 800 });
  });

  it("sin monitores no hay escritorio", () => {
    expect(desktopBounds([])).toBeNull();
  });
});

describe("isOuterEdge", () => {
  it("con una sola pantalla los cuatro bordes son exteriores", () => {
    for (const edge of ["left", "right", "top", "bottom"] as const) {
      expect(isOuterEdge(edge, SOLO[0], SOLO)).toBe(true);
    }
  });

  it("el canto entre dos pantallas no es un borde exterior", () => {
    // Izquierda del monitor izquierdo: sí. Su derecha: no, da al otro monitor.
    expect(isOuterEdge("left", DUAL[0], DUAL)).toBe(true);
    expect(isOuterEdge("right", DUAL[0], DUAL)).toBe(false);
    expect(isOuterEdge("left", DUAL[1], DUAL)).toBe(false);
    expect(isOuterEdge("right", DUAL[1], DUAL)).toBe(true);
  });

  it("tolera el resto de convertir físicos a CSS con escala fraccionaria", () => {
    const off: Area[] = [{ x: 0.4, y: 0, w: 1000, h: 800 }];
    expect(isOuterEdge("left", off[0], off)).toBe(true);
  });
});

describe("edgeGaps", () => {
  it("mide contra el área útil, no contra la pantalla", () => {
    const work = { x: 0, y: 0, w: 1000, h: 760 };
    // Pegada al fondo real de la pantalla: se metió 40 px en la barra.
    expect(edgeGaps(at(500, 760), work).bottom).toBe(-40);
  });
});

describe("dockCandidate", () => {
  it("engancha al borde más cercano dentro del umbral", () => {
    expect(dockCandidate(at(10, 400), SOLO)?.edge).toBe("left");
    expect(dockCandidate(at(950, 400), SOLO)?.edge).toBe("right");
    expect(dockCandidate(at(400, 10), SOLO)?.edge).toBe("top");
    expect(dockCandidate(at(400, 750), SOLO)?.edge).toBe("bottom");
  });

  it("en el medio no engancha a nada", () => {
    expect(dockCandidate(at(500, 400), SOLO)).toBeNull();
  });

  it("justo fuera del umbral tampoco", () => {
    expect(dockCandidate(at(DOCK_SNAP_PX + 1, 400), SOLO)).toBeNull();
    expect(dockCandidate(at(DOCK_SNAP_PX, 400), SOLO)?.edge).toBe("left");
  });

  it("en una esquina gana el borde más cercano", () => {
    expect(dockCandidate(at(2, 20), SOLO)?.edge).toBe("left");
    expect(dockCandidate(at(20, 2), SOLO)?.edge).toBe("top");
  });

  it("NO engancha al canto interior entre dos pantallas", () => {
    // Pegada al borde derecho del monitor izquierdo: es el medio del escritorio.
    expect(dockCandidate(at(955, 400), DUAL)).toBeNull();
    // Pero el borde derecho del monitor derecho sí es exterior.
    expect(dockCandidate(at(1955, 400), DUAL)?.edge).toBe("right");
  });

  it("respeta la barra de tareas: se pega al área útil", () => {
    const c = dockCandidate(at(400, 730), SOLO_TASKBAR);
    expect(c?.edge).toBe("bottom");
    // 760 (fondo útil) − 40 (alto de la pill), no 800.
    expect(c?.at.y).toBe(720);
  });

  it("se puede limitar el juego de bordes", () => {
    expect(dockCandidate(at(400, 10), SOLO, { edges: ["left", "right"] })).toBeNull();
  });
});

describe("dockedPoint", () => {
  it("fija solo el eje perpendicular y deja el otro donde estaba", () => {
    const work = { x: 0, y: 0, w: 1000, h: 800 };
    expect(dockedPoint("left", at(15, 333), work)).toEqual({ x: 0, y: 333 });
    expect(dockedPoint("right", at(985, 333), work)).toEqual({ x: 960, y: 333 });
    expect(dockedPoint("top", at(333, 15), work)).toEqual({ x: 333, y: 0 });
    expect(dockedPoint("bottom", at(333, 785), work)).toEqual({ x: 333, y: 760 });
  });

  it("recorta el eje libre para no salirse del área útil", () => {
    const work = { x: 0, y: 0, w: 1000, h: 800 };
    expect(dockedPoint("left", at(0, 900), work).y).toBe(760);
    expect(dockedPoint("left", at(0, -50), work).y).toBe(0);
  });
});

describe("shouldUndock", () => {
  it("soltar cuesta más que enganchar (histéresis)", () => {
    expect(DOCK_RELEASE_PX).toBeGreaterThan(DOCK_SNAP_PX);
    expect(shouldUndock(at(DOCK_SNAP_PX + 1, 400), "left", SOLO)).toBe(false);
    expect(shouldUndock(at(DOCK_RELEASE_PX + 1, 400), "left", SOLO)).toBe(true);
  });

  it("moverse a lo largo del mismo borde no la suelta", () => {
    expect(shouldUndock(at(0, 50), "left", SOLO)).toBe(false);
    expect(shouldUndock(at(0, 700), "left", SOLO)).toBe(false);
  });
});

describe("dockedEdgeAt", () => {
  it("reconoce un hogar guardado a ras del borde", () => {
    expect(dockedEdgeAt(at(0, 400), SOLO)).toBe("left");
    expect(dockedEdgeAt(at(960, 400), SOLO)).toBe("right");
  });

  it("un hogar flotante no cuenta como acoplado", () => {
    expect(dockedEdgeAt(at(20, 400), SOLO)).toBeNull();
  });
});

describe("dockAxis", () => {
  it("izquierda/derecha aplastan en x; arriba/abajo en y", () => {
    expect(dockAxis("left")).toBe("x");
    expect(dockAxis("right")).toBe("x");
    expect(dockAxis("top")).toBe("y");
    expect(dockAxis("bottom")).toBe("y");
  });
});
