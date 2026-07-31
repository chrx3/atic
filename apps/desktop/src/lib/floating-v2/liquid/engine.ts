import { DistanceField, type BridgeShape, type LiquidShape } from "./sdf";
import { fieldToPath } from "./marchingSquares";

export type LiquidBox = {
  id: string;
  cx: number;
  cy: number;
  hw: number;
  hh: number;
  r: number;
};

export type LiquidBridge = Omit<BridgeShape, "kind"> & { id: string };

export type LiquidParameters = {
  k: number;
  cell: number;
  smooth: number;
};

export const DEFAULT_LIQUID_PARAMETERS: LiquidParameters = { k: 26, cell: 6, smooth: 2 };

export class LiquidEngine {
  private boxes: LiquidBox[] = [];
  private bridges: LiquidBridge[] = [];
  private parameters = { ...DEFAULT_LIQUID_PARAMETERS };
  private cachedSignature = "";
  private cachedPath = { d: "", minX: 0, minY: 0, width: 0, height: 0 };

  setBoxes(boxes: LiquidBox[]) {
    this.boxes = boxes;
  }

  setBridges(bridges: LiquidBridge[]) {
    this.bridges = bridges;
  }

  setParameters(parameters: Partial<LiquidParameters>) {
    this.parameters = { ...this.parameters, ...parameters };
  }

  private signature(): string {
    const boxes = this.boxes
      .map((box) => `${box.id}:${box.cx.toFixed(1)},${box.cy.toFixed(1)},${box.hw.toFixed(1)},${box.hh.toFixed(1)},${box.r.toFixed(1)}`)
      .join("|");
    const bridges = this.bridges
      .map((bridge) => `${bridge.id}:${bridge.ax.toFixed(1)},${bridge.ay.toFixed(1)},${bridge.bx.toFixed(1)},${bridge.by.toFixed(1)},${bridge.r.toFixed(1)}`)
      .join("|");
    const { k, cell, smooth } = this.parameters;
    return `${boxes}#${bridges}#${k.toFixed(1)},${cell},${smooth}`;
  }

  compute() {
    const signature = this.signature();
    if (signature === this.cachedSignature) return this.cachedPath;
    this.cachedSignature = signature;

    const shapes: LiquidShape[] = [
      ...this.boxes.map<LiquidShape>((box) => ({
        kind: "box",
        cx: box.cx,
        cy: box.cy,
        hw: box.hw,
        hh: box.hh,
        r: box.r,
      })),
      ...this.bridges.map<LiquidShape>((bridge) => ({
        kind: "bridge",
        ax: bridge.ax,
        ay: bridge.ay,
        bx: bridge.bx,
        by: bridge.by,
        r: bridge.r,
      })),
    ];
    const field = new DistanceField(shapes, this.parameters.k);
    this.cachedPath = fieldToPath(field, this.parameters);
    return this.cachedPath;
  }
}
