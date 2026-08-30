import { describe, expect, it } from "vitest";
import {
  DOCK_RELEASE_PX,
  DOCK_SNAP_PX,
  EDGE_CORNER_PX,
  EDGE_WALL_DEPTH,
  EDGE_WALL_FLARE,
  EDGE_WALL_OVERLAP,
  defaultPillHome,
  dockAxis,
  dockCandidate,
  dockedEdgeAt,
  dockedPoint,
  desktopBounds,
  edgeGaps,
  edgeWallRect,
  edgeWallsFor,
  isOuterEdge,
  nearestOuterWorkEdge,
  shouldUndock,
  snapMagnet,
} from "./edgeDock";
import type { Area } from "$ipc/overlay";
import { pillShape } from "../../liquid/geometry";
import { BLEND } from "../../liquid/constants";
import { Field, shapeSD } from "../../liquid/sdf";

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

describe("nearestOuterWorkEdge", () => {
  it("encuentra el canto cuando la pill está cerca", () => {
    const hit = nearestOuterWorkEdge(at(400, 8), SOLO, 24);
    expect(hit?.edge).toBe("top");
    expect(hit?.gap).toBe(8);
  });

  it("en el medio no hay pared", () => {
    expect(nearestOuterWorkEdge(at(400, 400), SOLO, 24)).toBeNull();
  });

  it("el canto interior entre dos pantallas no cuenta", () => {
    expect(nearestOuterWorkEdge(at(960, 400), DUAL, 24)).toBeNull();
  });

  it("el borde de la barra de tareas no genera pared", () => {
    expect(nearestOuterWorkEdge(at(400, 720), SOLO_TASKBAR, 24)).toBeNull();
  });
});

describe("edgeWallRect", () => {
  const work = { x: 0, y: 0, w: 1000, h: 800 };
  const pill = { x: 200, y: 0, w: 80, h: 40 };

  it("la cara interior queda a ras del área útil, hacia afuera", () => {
    const wall = edgeWallRect("top", pill, work);
    expect(wall.y + wall.h).toBe(work.y + EDGE_WALL_OVERLAP);
    expect(wall.h).toBe(EDGE_WALL_DEPTH);
    expect(wall.w).toBe(pill.w + EDGE_WALL_FLARE * 2);
    expect(wall.x).toBe(pill.x - EDGE_WALL_FLARE);
  });

  it("abajo y a los lados también viven fuera", () => {
    const bottom = edgeWallRect("bottom", { ...pill, y: 760 }, work);
    expect(bottom.y).toBe(work.y + work.h - EDGE_WALL_OVERLAP);
    const left = edgeWallRect("left", { ...pill, x: 0, y: 300 }, work);
    expect(left.x + left.w).toBe(work.x + EDGE_WALL_OVERLAP);
    const right = edgeWallRect("right", { ...pill, x: 960, y: 300 }, work);
    expect(right.x).toBe(work.x + work.w - EDGE_WALL_OVERLAP);
  });

  it("se funde con la pill en el mordisco y no pinta un ala al lado", () => {
    const rect = { x: 200, y: 0, w: 80, h: 40 };
    const pill = pillShape(rect);
    const wall = pillShape(edgeWallRect("top", rect, work));
    const field = new Field([pill, wall], BLEND);
    expect(shapeSD(pill, 201, 6)).toBeGreaterThan(0);
    expect(field.eval(201, 6)).toBeLessThan(0);
    // Fuera del flare: no hay mancha de líquido al costado.
    expect(field.eval(rect.x - EDGE_WALL_FLARE - 12, 6)).toBeGreaterThan(0);
    expect(field.eval(500, 8)).toBeGreaterThan(0);
  });
});

describe("edgeWallsFor", () => {
  const work = { x: 0, y: 0, w: 1000, h: 800 };

  it("en el techo, lejos de los costados, el dintel es simétrico", () => {
    const pill = { x: 400, y: 0, w: 80, h: 40 };
    const walls = edgeWallsFor(pill, SOLO, { maxGap: 24, prefer: "top" });
    expect(walls).toHaveLength(1);
    const wall = walls[0];
    expect(wall.x).toBe(pill.x - EDGE_WALL_FLARE);
    expect(wall.w).toBe(pill.w + EDGE_WALL_FLARE * 2);
    const left = pill.x - wall.x;
    const right = wall.x + wall.w - (pill.x + pill.w);
    expect(left).toBe(right);
  });

  it("el menisco del techo no tira a un costado aunque el gap derecho empate", () => {
    const pill = { x: 960, y: 0, w: 40, h: 40 };
    const walls = edgeWallsFor(pill, SOLO, { maxGap: 24 });
    expect(walls).toHaveLength(2);
    const top = walls.find((w) => w.y < work.y);
    expect(top).toBeDefined();
    expect(top!.x + top!.w).toBe(work.x + work.w);
  });

  it("en una esquina emite las dos paredes y el dintel llega al vértice", () => {
    const pill = { x: 960, y: 0, w: 40, h: 40 };
    const walls = edgeWallsFor(pill, SOLO, { maxGap: 24, prefer: "top" });
    expect(walls).toHaveLength(2);
    const top = walls.find((w) => w.y < work.y);
    const right = walls.find((w) => w.x >= work.x + work.w - 1);
    expect(top).toBeDefined();
    expect(right).toBeDefined();
    expect(top!.x + top!.w).toBe(work.x + work.w);
    expect(right!.x).toBe(work.x + work.w);
    expect(right!.y).toBe(pill.y);
  });

  it("a más de EDGE_CORNER_PX del costado no inventa una pared lateral", () => {
    const pill = { x: 400, y: 0, w: 80, h: 40 };
    expect(pill.x + pill.w).toBeLessThan(work.w - EDGE_CORNER_PX);
    const walls = edgeWallsFor(pill, SOLO, { maxGap: 24, prefer: "top" });
    expect(walls).toHaveLength(1);
  });

  it("no pinta un blob sobre la barra de tareas", () => {
    const pill = { x: 400, y: 720, w: 80, h: 40 };
    const walls = edgeWallsFor(pill, SOLO_TASKBAR, {
      maxGap: 24,
      prefer: "bottom",
    });
    expect(walls).toHaveLength(0);
  });

  it("el campo del techo es espejo a izquierda y derecha", () => {
    const rect = { x: 400, y: 0, w: 80, h: 40 };
    const walls = edgeWallsFor(rect, SOLO, { maxGap: 24, prefer: "top" });
    const field = new Field([pillShape(rect), ...walls.map(pillShape)], BLEND);
    const y = 10;
    const d = 20;
    expect(field.eval(rect.x - d, y)).toBeCloseTo(
      field.eval(rect.x + rect.w + d, y),
      5,
    );
  });
});

describe("defaultPillHome", () => {
  it("queda arriba al centro del monitor principal", () => {
    const areas: Area[] = [
      { x: 0, y: 0, w: 1000, h: 800 },
      { x: 1000, y: 0, w: 1200, h: 800, primary: true },
    ];
    expect(defaultPillHome({ w: 40, h: 40 }, areas)).toEqual({
      at: { x: 1000 + (1200 - 40) / 2, y: 0 },
      edge: "top",
    });
  });

  it("sin marca primary usa el primer monitor", () => {
    expect(defaultPillHome({ w: 40, h: 40 }, SOLO)?.at).toEqual({
      x: (1000 - 40) / 2,
      y: 0,
    });
  });
});

describe("snapMagnet", () => {
  it("cerca del centro de la pantalla queda flotante", () => {
    const hit = snapMagnet(at(480, 380), SOLO);
    expect(hit?.edge).toBeNull();
    expect(hit?.at).toEqual({ x: (1000 - 40) / 2, y: (800 - 40) / 2 });
  });

  it("cerca de un canto va al centro de ese canto", () => {
    expect(snapMagnet(at(10, 100), SOLO)).toEqual({
      at: { x: 0, y: (800 - 40) / 2 },
      edge: "left",
    });
    expect(snapMagnet(at(400, 8), SOLO)?.edge).toBe("top");
    expect(snapMagnet(at(400, 8), SOLO)?.at).toEqual({
      x: (1000 - 40) / 2,
      y: 0,
    });
  });

  it("en el medio, lejos de imanes, no arrastra", () => {
    expect(snapMagnet(at(200, 200), SOLO, 48)).toBeNull();
  });

  it("el canto interior entre monitores no es un imán", () => {
    expect(snapMagnet(at(960, 380), DUAL)).toBeNull();
  });
});
