import { afterEach, describe, expect, it } from "vitest";
import {
  BOTTOM_SLOT_INSET,
  hasToolSlot,
  isSpatialTool,
  LAUNCHER_BAR_W,
  LAUNCHER_PILL_GAP,
  resolveSlot,
  setSlotOverrides,
  slotForTool,
} from "./toolSlots";
import { MARGIN } from "./contract";

const work = [{ x: 0, y: 0, w: 1400, h: 900 }];
const size = { w: 48, h: 48 };
const anchor = { x: 200, y: 200 };

afterEach(() => {
  setSlotOverrides(null);
});

describe("toolSlots", () => {
  it("asigna slots solo a launcher y dictado", () => {
    expect(slotForTool("launcher")).toBe("center-left-of-launcher");
    expect(slotForTool("dictation")).toBe("bottom-center");
    expect(slotForTool("agents")).toBeNull();
    expect(slotForTool("clipboard")).toBeNull();
    expect(slotForTool("snippets")).toBeNull();
    expect(hasToolSlot("meetings")).toBe(false);
    expect(hasToolSlot("captures")).toBe(false);
  });

  it("marca floats + launcher como espaciales", () => {
    expect(isSpatialTool("launcher")).toBe(true);
    expect(isSpatialTool("clipboard")).toBe(true);
    expect(isSpatialTool("snippets")).toBe(true);
    expect(isSpatialTool("agents")).toBe(true);
    expect(isSpatialTool("dictation")).toBe(false);
  });

  it("respeta overrides sin tocar el resto", () => {
    setSlotOverrides({ clipboard: "center" });
    expect(slotForTool("clipboard")).toBe("center");
    expect(slotForTool("snippets")).toBeNull();
  });

  it("centra la pill en el work area", () => {
    const p = resolveSlot("center", work, size, anchor);
    expect(p.x).toBe((1400 - 48) / 2);
    expect(p.y).toBe((900 - 48) / 2);
  });

  it("coloca bottom-center arriba de la taskbar", () => {
    const p = resolveSlot("bottom-center", work, size, anchor);
    expect(p.x).toBe((1400 - 48) / 2);
    expect(p.y).toBe(900 - 48 - MARGIN - BOTTOM_SLOT_INSET);
  });

  it("coloca center-right / center-left / bottom-right", () => {
    const right = resolveSlot("center-right", work, size, anchor);
    expect(right.x).toBe(1400 - 48 - MARGIN);
    expect(right.y).toBe((900 - 48) / 2);

    const left = resolveSlot("center-left", work, size, anchor);
    expect(left.x).toBe(MARGIN);
    expect(left.y).toBe((900 - 48) / 2);

    const br = resolveSlot("bottom-right", work, size, anchor);
    expect(br.x).toBe(1400 - 48 - MARGIN);
    expect(br.y).toBe(900 - 48 - MARGIN - BOTTOM_SLOT_INSET);
  });

  it("coloca center-left-of-launcher a la izquierda de la barra centrada", () => {
    const p = resolveSlot("center-left-of-launcher", work, size, anchor);
    const cx = 1400 / 2;
    const cy = 900 / 2;
    expect(p.x).toBe(cx - LAUNCHER_BAR_W / 2 - LAUNCHER_PILL_GAP - 48);
    expect(p.y).toBe(cy - 48 / 2);
  });

  it("elige el monitor del anchor en multi-monitor", () => {
    const areas = [
      { x: 0, y: 0, w: 1000, h: 800 },
      { x: 1000, y: 0, w: 1200, h: 900 },
    ];
    const p = resolveSlot("center", areas, size, { x: 1500, y: 100 });
    expect(p.x).toBe(1000 + (1200 - 48) / 2);
    expect(p.y).toBe((900 - 48) / 2);
  });

  it("centra la barra en el monitor del mouse, no en el de la pill", () => {
    const areas = [
      { x: 0, y: 0, w: 1536, h: 864 },
      { x: 1536, y: 0, w: 1536, h: 864 },
    ];
    const bar = { w: LAUNCHER_BAR_W, h: 40 };
    const p = resolveSlot("center", areas, bar, { x: 2000, y: 400 });
    expect(p.x).toBe(1536 + (1536 - LAUNCHER_BAR_W) / 2);
    expect(p.y).toBe((864 - 40) / 2);
  });
});
