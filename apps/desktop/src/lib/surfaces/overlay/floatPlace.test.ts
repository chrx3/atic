import { describe, expect, it } from "vitest";
import {
  PANEL_GROW_SEED,
  PANEL_RESTING_GAP_PX,
  expandPanelFromSeed,
  placeBesidePill,
  placeOnSide,
  placePanelFusedSeed,
  placePanelResting,
} from "./floatPlace";
import { BOTTOM_SLOT_INSET } from "./toolSlots";

const work = [{ x: 0, y: 0, w: 1400, h: 900 }];

describe("placeBesidePill", () => {
  it("abre abajo-derecha: pill en la esquina superior-izquierda del panel", () => {
    const pill = { x: 200, y: 100, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work,
    });

    expect(placed.side).toBe("top");
    expect(placed.y).toBe(100 + 48 + 10);
    // Panel empieza cerca del borde derecho de la pill, no centrado bajo ella.
    expect(placed.x).toBe(200 + 48 - 18);
    expect(placed.x + placed.w).toBeGreaterThan(pill.x + pill.w);
  });

  it("si no cabe abajo, abre arriba pegado por esquina", () => {
    const pill = { x: 200, y: 700, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work,
    });

    expect(placed.side).toBe("bottom");
    expect(placed.y + placed.h + 10).toBe(pill.y);
    expect(placed.x).toBe(200 + 48 - 18);
  });

  it("no centra el panel bajo la pill", () => {
    const pill = { x: 400, y: 80, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work,
    });
    const centered = pill.x + pill.w / 2 - 312 / 2;
    expect(placed.x).not.toBeCloseTo(centered, 0);
  });

  it("cerca del borde inferior abre arriba, con aire de taskbar", () => {
    // En bounds de 864 cabe abajo (860 <= 864) pero se montaría sobre la
    // taskbar. El inset obliga el flip.
    const screen = [{ x: 0, y: 0, w: 1536, h: 864 }];
    const pill = { x: 200, y: 430, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work: screen,
    });
    expect(placed.side).toBe("bottom");
    expect(placed.y + placed.h).toBeLessThanOrEqual(864 - BOTTOM_SLOT_INSET);
    expect(placed.y).toBeGreaterThanOrEqual(0);
  });

  it("usa rcWork si vino: no hace falta el inset a ojo", () => {
    const screen = [
      {
        x: 0,
        y: 0,
        w: 1536,
        h: 864,
        work: { x: 0, y: 0, w: 1536, h: 808 },
      },
    ];
    const pill = { x: 200, y: 760, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work: screen,
    });
    expect(placed.side).toBe("bottom");
    expect(placed.y + placed.h).toBeLessThanOrEqual(808);
  });

  it("cerca del canto derecho no recorta el panel", () => {
    const screen = [{ x: 0, y: 0, w: 1536, h: 864 }];
    const pill = { x: 1480, y: 80, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work: screen,
    });
    expect(placed.x).toBeGreaterThanOrEqual(0);
    expect(placed.x + placed.w).toBeLessThanOrEqual(1536);
  });

  it("en dual monitor no se cruza al vecino", () => {
    const screens = [
      { x: 0, y: 0, w: 1536, h: 864 },
      { x: 1536, y: 0, w: 1536, h: 864 },
    ];
    const pill = { x: 1480, y: 80, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work: screens,
    });
    expect(placed.x).toBeGreaterThanOrEqual(0);
    expect(placed.x + placed.w).toBeLessThanOrEqual(1536);
  });

  it("pill en el monitor derecho no clampa al izquierdo", () => {
    const screens = [
      { x: 0, y: 0, w: 1536, h: 864 },
      { x: 1536, y: 0, w: 1536, h: 864 },
    ];
    const pill = { x: 2000, y: 80, w: 48, h: 48 };
    const placed = placeBesidePill(pill, { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work: screens,
    });
    expect(placed.x).toBeGreaterThanOrEqual(1536);
    expect(placed.x + placed.w).toBeLessThanOrEqual(3072);
  });
});

describe("placeOnSide", () => {
  it("clampea el eje hacia afuera si el lado no entra", () => {
    const screen = [{ x: 0, y: 0, w: 1400, h: 900 }];
    const pill = { x: 200, y: 700, w: 48, h: 48 };
    const placed = placeOnSide(pill, "top", { w: 312, h: 372 }, {
      corner: 18,
      gap: 10,
      work: screen,
    });
    expect(placed.y).toBeGreaterThanOrEqual(0);
    expect(placed.y + placed.h).toBeLessThanOrEqual(900 - BOTTOM_SLOT_INSET);
  });
});

describe("panel fused grow helpers", () => {
  const pill = { x: 200, y: 100, w: 48, h: 48 };
  const full = { w: 312, h: 372 };

  it("placePanelResting usa gap idle > REACH", () => {
    const placed = placePanelResting(pill, full, { corner: 18, work });
    expect(placed.side).toBe("top");
    expect(placed.y).toBe(pill.y + pill.h + PANEL_RESTING_GAP_PX);
  });

  it("placePanelFusedSeed nace disco solapado (no al lado)", () => {
    const seed = placePanelFusedSeed(pill, full, { corner: 18, work });
    expect(seed.side).toBe("top");
    expect(seed.w).toBe(PANEL_GROW_SEED);
    expect(seed.h).toBe(PANEL_GROW_SEED);
    // gap negativo = overlap sobre el borde inferior de la pill
    expect(seed.y).toBe(pill.y + pill.h - 20);
  });

  it("expandPanelFromSeed crece con borde top clavado", () => {
    const seed = placePanelFusedSeed(pill, full, { corner: 18, work });
    const grown = expandPanelFromSeed(seed, full);
    expect(grown.x).toBe(seed.x);
    expect(grown.y).toBe(seed.y);
    expect(grown.w).toBe(full.w);
    expect(grown.h).toBe(full.h);
  });

  it("expandPanelFromSide bottom bloquea el borde inferior", () => {
    const seed = {
      side: "bottom" as const,
      offset: 20,
      x: 200,
      y: 50,
      w: PANEL_GROW_SEED,
      h: PANEL_GROW_SEED,
    };
    const grown = expandPanelFromSeed(seed, full);
    expect(grown.y + grown.h).toBe(seed.y + seed.h);
    expect(grown.w).toBe(full.w);
    expect(grown.h).toBe(full.h);
  });
});
