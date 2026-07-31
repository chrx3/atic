import type { Pivot, Size } from "$lib/pillStage";
import type { FloatingSurface } from "./types";

export const FLOATING_ENVELOPES = {
  pill: { w: 190, h: 90 },
  wheel: { w: 400, h: 280 },
  tool: { w: 520, h: 560 },
  agents: { w: 700, h: 620 },
} as const satisfies Record<FloatingSurface["kind"], Size>;

export function envelopeFor(surface: FloatingSurface): Size {
  return FLOATING_ENVELOPES[surface.kind];
}

export function opensLargeSurface(surface: FloatingSurface): boolean {
  return surface.kind === "tool" || surface.kind === "agents";
}

export function pivotForTransition(
  from: FloatingSurface,
  to: FloatingSurface,
  openedUp: boolean,
): Pivot {
  if (opensLargeSurface(to)) return "panelCenter";
  if (opensLargeSurface(from)) return openedUp ? "bottomCenter" : "topCenter";
  return "topCenter";
}
