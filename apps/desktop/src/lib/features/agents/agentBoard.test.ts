import { describe, expect, it } from "vitest";
import {
  CHILD_GAP,
  MAX_ZOOM,
  MIN_H,
  MIN_W,
  MIN_ZOOM,
  arrange,
  bounds,
  cardAt,
  centerOn,
  childRect,
  composerWrites,
  dragRect,
  fitInto,
  fitRect,
  focusedByView,
  fullyVisible,
  parentKind,
  parseCamera,
  parseRect,
  launchCommand,
  parseSpaces,
  placeSpace,
  placeNew,
  pushHistory,
  quotePaths,
  upsertSpace,
  visibleArea,
  zoomAt,
} from "./agentBoard";

const view = { w: 1400, h: 900 };
const identity = { x: 0, y: 0, zoom: 1 };

describe("placeNew", () => {
  const area = { x: 0, y: 0, w: 1400, h: 900 };

  it("nace centrada en lo que se está mirando", () => {
    const r = placeNew([], area, { w: 760, h: 480 });
    expect(r.x + r.w / 2).toBe(700);
    expect(r).toMatchObject({ w: 760, h: 480 });
  });

  it("no cae encima exacta de otra: va en cascada", () => {
    const first = placeNew([], area);
    const second = placeNew([first], area);
    const third = placeNew([first, second], area);
    expect(second.x).toBeGreaterThan(first.x);
    expect(third.y).toBeGreaterThan(second.y);
  });

  it("en un área chica se achica, sin bajar del mínimo", () => {
    const r = placeNew([], { x: 0, y: 0, w: 300, h: 200 });
    expect(r.w).toBe(MIN_W);
    expect(r.h).toBe(MIN_H);
  });
});

describe("cámara", () => {
  it("lo visible descuenta los flotantes y se agranda al alejar", () => {
    const insets = { top: 0, right: 0, bottom: 100, left: 200 };
    expect(visibleArea(identity, view, insets)).toEqual({
      x: 200,
      y: 0,
      w: 1200,
      h: 800,
    });
    const far = visibleArea({ x: 0, y: 0, zoom: 0.5 }, view);
    expect(far).toMatchObject({ w: 2800, h: 1800 });
  });

  it("el zoom deja quieto lo que está bajo el puntero", () => {
    const at = { x: 300, y: 200 };
    const before = { x: (at.x - identity.x) / identity.zoom, y: at.y };
    const cam = zoomAt(identity, 0.5, at);
    expect((at.x - cam.x) / cam.zoom).toBeCloseTo(before.x);
    expect((at.y - cam.y) / cam.zoom).toBeCloseTo(before.y);
  });

  it("el zoom no se sale de sus topes", () => {
    expect(zoomAt(identity, 50, { x: 0, y: 0 }).zoom).toBe(MAX_ZOOM);
    expect(zoomAt(identity, 0.001, { x: 0, y: 0 }).zoom).toBe(MIN_ZOOM);
  });

  it("encuadrar centra en el área libre sin acercar de más", () => {
    const rect = { x: 1000, y: 800, w: 400, h: 200 };
    const cam = fitRect(rect, view);
    expect(cam.zoom).toBe(1);
    expect(cam.x + (rect.x + rect.w / 2) * cam.zoom).toBe(700);
    expect(cam.y + (rect.y + rect.h / 2) * cam.zoom).toBe(450);
  });

  it("encuadrar algo grande aleja hasta que entra", () => {
    const all = { x: 0, y: 0, w: 4000, h: 1000 };
    const cam = fitRect(all, view);
    expect(cam.zoom).toBeLessThan(1);
    expect(fullyVisible(all, cam, view)).toBe(true);
  });

  it("guarda la cámara y descarta una rota", () => {
    expect(parseCamera({ x: 1, y: 2, zoom: 0.5 })).toEqual({ x: 1, y: 2, zoom: 0.5 });
    expect(parseCamera({ x: 1, y: 2 })).toEqual({ x: 1, y: 2, zoom: 1 });
    expect(parseCamera({ x: "1", y: 2 })).toBeNull();
  });
});

describe("foco por lo que se mira", () => {
  const rects = [
    { x: 0, y: 0, w: 600, h: 400 },
    { x: 2000, y: 0, w: 600, h: 400 },
  ];

  it("gana la que está en el centro de la vista", () => {
    expect(focusedByView(rects, fitRect(rects[1], view), view)).toBe(1);
    expect(focusedByView(rects, fitRect(rects[0], view), view)).toBe(0);
  });

  it("si el centro cae en un hueco, la que más se ve; si no se ve ninguna, -1", () => {
    const cam = { x: -300, y: 200, zoom: 1 };
    expect(focusedByView(rects, cam, view)).toBe(0);
    expect(focusedByView(rects, { x: -10000, y: -10000, zoom: 1 }, view)).toBe(-1);
  });
});

describe("arrange", () => {
  const rects = [
    { x: 500, y: 300, w: 600, h: 400 },
    { x: 100, y: 100, w: 500, h: 300 },
    { x: 900, y: 900, w: 700, h: 500 },
    { x: 50, y: 700, w: 400, h: 260 },
  ];

  const overlaps = (list: { x: number; y: number; w: number; h: number }[]) =>
    list.some((a, i) =>
      list.some(
        (b, j) =>
          i < j &&
          a.x < b.x + b.w &&
          b.x < a.x + a.w &&
          a.y < b.y + b.h &&
          b.y < a.y + a.h,
      ),
    );

  it("en fila, una junto a otra alineadas arriba", () => {
    const out = arrange(rects, "row", 20);
    expect(new Set(out.map((r) => r.y)).size).toBe(1);
    expect(out[1].x).toBe(out[0].x + out[0].w + 20);
    expect(overlaps(out)).toBe(false);
  });

  it("en columna, una debajo de otra alineadas a la izquierda", () => {
    const out = arrange(rects, "column", 20);
    expect(new Set(out.map((r) => r.x)).size).toBe(1);
    expect(out[1].y).toBe(out[0].y + out[0].h + 20);
  });

  it("en grilla, sin superponerse y sin cambiar tamaños", () => {
    const out = arrange(rects, "grid");
    expect(overlaps(out)).toBe(false);
    expect(out.map((r) => [r.w, r.h])).toEqual(rects.map((r) => [r.w, r.h]));
  });

  it("una nueva al final no mueve a las que ya estaban en fila", () => {
    const placed = arrange(rects, "row");
    const box = bounds(placed);
    const added = [...placed, { x: box?.x ?? 0, y: box?.y ?? 0, w: 500, h: 300 }];
    const again = arrange(added, "row");
    expect(again.slice(0, placed.length)).toEqual(placed);
    const prev = placed[placed.length - 1];
    expect(again[again.length - 1].x).toBe(prev.x + prev.w + 28);
  });

  it("parte desde donde ya estaban, no desde el origen", () => {
    const out = arrange(rects, "grid");
    const before = bounds(rects);
    expect(out[0]).toMatchObject({ x: before?.x, y: before?.y });
  });
});

describe("dragRect", () => {
  const start = { x: 100, y: 100, w: 600, h: 400 };

  it("mover corre sin cambiar el tamaño", () => {
    expect(dragRect(start, "move", 10, -20)).toEqual({ x: 110, y: 80, w: 600, h: 400 });
  });

  it("desde la izquierda el borde derecho queda quieto", () => {
    const r = dragRect(start, "w", 50, 0);
    expect(r.x + r.w).toBe(700);
    expect(r.w).toBe(550);
  });

  it("no baja del tamaño mínimo", () => {
    const r = dragRect(start, "se", -1000, -1000);
    expect(r).toMatchObject({ w: MIN_W, h: MIN_H });
    const left = dragRect(start, "w", 1000, 0);
    expect(left.w).toBe(MIN_W);
    expect(left.x + left.w).toBe(700);
  });
});

describe("parseRect", () => {
  it("acepta lo guardado y descarta lo roto", () => {
    expect(parseRect({ x: 1, y: 2, w: 800, h: 500 })).toEqual({
      x: 1,
      y: 2,
      w: 800,
      h: 500,
    });
    expect(parseRect({ x: 1, y: 2, w: 10, h: 10 })).toMatchObject({
      w: MIN_W,
      h: MIN_H,
    });
    expect(parseRect({ x: "1", y: 2, w: 800, h: 500 })).toBeNull();
    expect(parseRect(null)).toBeNull();
  });
});

describe("composerWrites", () => {
  it("a un agente le pega el mensaje entero y después el Enter", () => {
    expect(composerWrites("hola\r\nchau", true)).toEqual([
      "\x1b[200~hola\nchau\x1b[201~",
      "\r",
    ]);
  });

  it("a la shell del sistema, sin marcas y en una línea", () => {
    expect(composerWrites("dir\nls", false)).toEqual(["dir ls", "\r"]);
  });
});

describe("historial de la entrada", () => {
  it("guarda al final, sube lo repetido y no guarda lo vacío", () => {
    let h: string[] = [];
    h = pushHistory(h, "hola");
    h = pushHistory(h, "  chau ");
    h = pushHistory(h, "hola");
    h = pushHistory(h, "   ");
    expect(h).toEqual(["chau", "hola"]);
  });

  it("no pasa del tope", () => {
    let h: string[] = [];
    for (let i = 0; i < 10; i++) h = pushHistory(h, `m${i}`, 3);
    expect(h).toEqual(["m7", "m8", "m9"]);
  });
});

describe("soltar archivos", () => {
  it("comillas solo donde hay espacios", () => {
    expect(quotePaths(["C:\\a.txt", "C:\\mis cosas\\b.png", ""])).toBe(
      'C:\\a.txt "C:\\mis cosas\\b.png"',
    );
  });

  it("cae en la de más arriba bajo el punto", () => {
    const entries = [
      { key: "a", rect: { x: 0, y: 0, w: 500, h: 300 }, z: 1 },
      { key: "b", rect: { x: 200, y: 100, w: 500, h: 300 }, z: 2 },
    ];
    expect(cardAt(entries, { x: 300, y: 200 })).toBe("b");
    expect(cardAt(entries, { x: 50, y: 50 })).toBe("a");
    expect(cardAt(entries, { x: 900, y: 900 })).toBeNull();
  });
});

describe("minimapa", () => {
  it("dibuja el mundo centrado y sin deformar", () => {
    const t = fitInto({ x: 100, y: 0, w: 1000, h: 250 }, { w: 200, h: 100 });
    expect(t.scale).toBe(0.2);
    // El mundo ocupa 200×50 y queda centrado en alto.
    expect(100 * t.scale + t.x).toBe(0);
    expect(0 * t.scale + t.y).toBe(25);
  });

  it("centrar en un punto lo deja al medio del área libre", () => {
    const cam = centerOn({ x: 500, y: 300 }, { x: 0, y: 0, zoom: 0.5 }, view);
    expect(cam.x + 500 * cam.zoom).toBe(700);
    expect(cam.y + 300 * cam.zoom).toBe(450);
    expect(cam.zoom).toBe(0.5);
  });
});

describe("sesiones guardadas", () => {
  const consoleAt = (x: number) => ({
    label: "Claude Code",
    cli: "claude",
    command: "claude",
    cwd: "C:/p",
    rect: { x, y: 0, w: 600, h: 400 },
  });

  it("lee lo guardado y descarta lo roto", () => {
    const raw = JSON.stringify([
      { name: " atic ", savedAt: 5, layout: "row", consoles: [consoleAt(0)] },
      { name: "", consoles: [consoleAt(0)] },
      { name: "vacío", consoles: [] },
      { name: "raro", layout: "diagonal", consoles: [consoleAt(0), { label: 3 }] },
    ]);
    const spaces = parseSpaces(raw);
    expect(spaces.map((s) => [s.name, s.layout, s.consoles.length])).toEqual([
      ["atic", "row", 1],
      ["raro", "free", 1],
    ]);
    expect(parseSpaces("no es json")).toEqual([]);
  });

  it("guardar con el mismo nombre reemplaza, y queda primero", () => {
    const a = {
      name: "atic",
      savedAt: 1,
      layout: "free" as const,
      consoles: [consoleAt(0)],
    };
    const b = {
      name: "api",
      savedAt: 2,
      layout: "free" as const,
      consoles: [consoleAt(0)],
    };
    const again = { ...a, name: "ATIC", savedAt: 3 };
    expect(upsertSpace(upsertSpace([a], b), again).map((s) => s.savedAt)).toEqual([
      3, 2,
    ]);
  });

  it("retoma la conversación de cada agente y descarta un id que no es id", () => {
    const id = "3f2c9a1e-7b4d-4c2a-9e1f-0a1b2c3d4e5f";
    const raw = JSON.stringify([
      {
        name: "atic",
        consoles: [
          { ...consoleAt(0), resume: id },
          { ...consoleAt(700), resume: "x & del *" },
        ],
      },
    ]);
    const [ok, bad] = parseSpaces(raw)[0].consoles;
    expect(launchCommand(ok)).toBe(`claude --resume ${id}`);
    expect(bad.resume).toBeUndefined();
    expect(launchCommand(bad)).toBe("claude");
    expect(launchCommand({ ...ok, cli: null, command: null })).toBeNull();
    expect(launchCommand({ ...ok, cli: "codex", command: "codex" })).toBe(
      `codex resume ${id}`,
    );
    expect(launchCommand({ ...ok, cli: "grok", command: "grok" })).toBe(
      `grok -r ${id}`,
    );
    expect(launchCommand({ ...ok, cli: "agy", command: "agy" })).toBe(
      `agy --conversation ${id}`,
    );
    expect(launchCommand({ ...ok, cli: "cursor-agent", command: "cursor-agent" })).toBe(
      `cursor-agent --resume ${id}`,
    );
    const ses = "ses_f2af25595ffe9ZpdXAhi1PU6cO";
    expect(
      launchCommand({ ...ok, cli: "opencode", command: "opencode", resume: ses }),
    ).toBe(`opencode --session ${ses}`);
    expect(launchCommand({ ...ok, cli: "kimi", command: "kimi" })).toBe("kimi");
  });

  it("en una pizarra vacía se abre donde estaba; si no, al lado", () => {
    const saved = [consoleAt(0).rect, consoleAt(700).rect];
    expect(placeSpace(saved, [])).toEqual(saved);
    const existing = [{ x: 100, y: 50, w: 500, h: 300 }];
    const moved = placeSpace(saved, existing);
    expect(moved[0].x).toBeGreaterThan(600);
    expect(moved[0].y).toBe(50);
    expect(moved[1].x - moved[0].x).toBe(700);
  });
});

describe("sub-agentes", () => {
  const parent = { x: 100, y: 200, w: 700, h: 450 };

  it("van a la derecha de quien los pidió, en columna", () => {
    const first = childRect(parent, 0, undefined);
    const second = childRect(parent, 1, undefined);
    expect(first.x).toBe(100 + 700 + CHILD_GAP);
    expect(first.y).toBe(200);
    expect(second.y).toBeGreaterThan(first.y + first.h);
  });

  it("lo que el usuario los corrió se mantiene aunque la consola se mueva", () => {
    const offset = { dx: 40, dy: -20, w: 600, h: 500 };
    const here = childRect(parent, 0, offset);
    const moved = childRect({ ...parent, x: 1000 }, 0, offset);
    expect(moved.x - here.x).toBe(900);
    expect(moved).toMatchObject({ w: 600, h: 500 });
  });

  it("quién los pidió: otra sesión o un CLI de afuera", () => {
    expect(parentKind("external:claude-code:1234")).toBe("external");
    expect(parentKind("4f0c1c3e-uuid")).toBe("session");
    expect(parentKind(null)).toBeNull();
  });
});
