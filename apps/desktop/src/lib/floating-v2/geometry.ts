import type { Pivot, Size } from "$lib/pillStage";
import type { FloatingActivity, FloatingSurface, PillRestShape } from "./types";

export const FLOATING_ENVELOPES = {
  pill: { w: 190, h: 90 },
  wheel: { w: 400, h: 280 },
  queue: { w: 520, h: 560 },
  tool: { w: 520, h: 560 },
  agents: { w: 700, h: 620 },
} as const satisfies Record<FloatingSurface["kind"], Size>;

export const CIRCLE_PILL_ENVELOPE = { w: 64, h: 64 } as const satisfies Size;

export function pillPresentationFor(
  surface: FloatingSurface,
  activity: FloatingActivity,
  restShape: PillRestShape,
  hasAttention = false,
): PillRestShape {
  return surface.kind === "pill" && activity === "idle" && !hasAttention ? restShape : "compact";
}

export function envelopeFor(surface: FloatingSurface, pillShape: PillRestShape = "compact"): Size {
  if (surface.kind === "pill" && pillShape === "circle") return CIRCLE_PILL_ENVELOPE;
  return FLOATING_ENVELOPES[surface.kind];
}

export function opensLargeSurface(surface: FloatingSurface): boolean {
  return surface.kind === "tool" || surface.kind === "agents" || surface.kind === "queue";
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
