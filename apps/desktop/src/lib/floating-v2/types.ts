import type { ToolId } from "$lib/tools";

export type FloatingActivity = "idle" | "recording" | "dictating";
export type PillRestShape = "circle" | "compact";

export type FloatingSurface =
  | { kind: "pill" }
  | { kind: "wheel" }
  | { kind: "queue" }
  | { kind: "tool"; tool: Exclude<ToolId, "agents"> }
  | { kind: "agents" };

export type FloatingPlacement = "free" | "edge";

export type FloatingExperienceState = {
  activity: FloatingActivity;
  surface: FloatingSurface;
};

export const INITIAL_FLOATING_STATE: FloatingExperienceState = {
  activity: "idle",
  surface: { kind: "pill" },
};

export function surfaceForTool(tool: ToolId): FloatingSurface {
  return tool === "agents" ? { kind: "agents" } : { kind: "tool", tool };
}
