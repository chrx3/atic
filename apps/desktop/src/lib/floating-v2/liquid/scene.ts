import type { FloatingSurface } from "../types";
import type { LiquidBox, LiquidBridge, LiquidParameters } from "./engine";

export type LiquidScene = {
  boxes: LiquidBox[];
  bridges: LiquidBridge[];
  parameters: LiquidParameters;
};

const WHEEL_POSITIONS = [
  { x: -140, y: 62 },
  { x: -112, y: 116 },
  { x: -42, y: 148 },
  { x: 42, y: 148 },
  { x: 112, y: 116 },
  { x: 140, y: 62 },
];

function box(id: string, x: number, y: number, width: number, height: number, radius: number): LiquidBox {
  return {
    id,
    cx: x + width / 2,
    cy: y + height / 2,
    hw: width / 2,
    hh: height / 2,
    r: radius,
  };
}

function isLarge(surface: FloatingSurface): boolean {
  return surface.kind === "tool" || surface.kind === "agents" || surface.kind === "queue";
}

/** Maps product state to explicit geometry. The content layer uses the same
 * numbers, so the SDF skin and the accessible DOM never drift apart. */
export function sceneFor(
  surface: FloatingSurface,
  width: number,
  height: number,
  opensUp = false,
): LiquidScene {
  const centerX = width / 2;
  const large = isLarge(surface);
  const pillTop = opensUp && large ? height - 82 : 32;
  const pill = box("pill", centerX - 77, pillTop, 154, 50, 25);

  if (surface.kind === "wheel") {
    return {
      boxes: [
        pill,
        ...WHEEL_POSITIONS.map((position, index) =>
          box(`tool-${index}`, centerX + position.x - 26, 34 + position.y, 52, 52, 26),
        ),
      ],
      bridges: [],
      parameters: { k: 18, cell: 4, smooth: 2 },
    };
  }

  if (!large) {
    return {
      boxes: [pill],
      bridges: [],
      parameters: { k: 10, cell: 3, smooth: 2 },
    };
  }

  const agents = surface.kind === "agents";
  const panelWidth = agents ? 640 : 448;
  const panelHeight = agents ? 424 : 346;
  const panelTop = opensUp ? (agents ? 40 : 28) : agents ? 156 : 186;
  const panel = box("panel", centerX - panelWidth / 2, panelTop, panelWidth, panelHeight, agents ? 38 : 34);
  const pillEdge = opensUp ? pillTop : pillTop + 50;
  const panelEdge = opensUp ? panelTop + panelHeight : panelTop;

  return {
    boxes: [pill, panel],
    bridges: [
      {
        id: "pill-panel",
        ax: centerX,
        ay: pillEdge,
        bx: centerX,
        by: panelEdge,
        r: agents ? 24 : 22,
      },
    ],
    parameters: { k: agents ? 22 : 18, cell: agents ? 6 : 5, smooth: 2 },
  };
}
