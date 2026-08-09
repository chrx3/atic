import { describe, expect, it } from "vitest";
import {
  PANEL_GROW_SEED,
  PANEL_RESTING_GAP_PX,
  expandPanelFromSeed,
  placeBesidePill,
  placePanelFusedSeed,
  placePanelResting,
} from "./floatPlace";

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
