import { LiquidEngine } from "./engine";
import { sceneFor } from "./scene";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal<T>(actual: T, expected: T, message: string) {
  assert(Object.is(actual, expected), `${message}: expected ${String(expected)}, got ${String(actual)}`);
}

function compute(surface: Parameters<typeof sceneFor>[0], width = 760, height = 620, opensUp = false) {
  const scene = sceneFor(surface, width, height, opensUp);
  const engine = new LiquidEngine();
  engine.setBoxes(scene.boxes);
  engine.setBridges(scene.bridges);
  engine.setParameters(scene.parameters);
  return { scene, path: engine.compute() };
}

const pill = compute({ kind: "pill" });
assert(/^M /.test(pill.path.d), "the pill must generate a closed SVG contour");
assert(pill.path.width > 154 && pill.path.height > 50, "field bounds must retain the smooth-union padding");

const circleScene = sceneFor({ kind: "pill" }, 64, 64, false, "circle");
const circle = circleScene.boxes[0];
equal(circle.cx, 32, "the idle circle must stay centered in its native envelope");
equal(circle.cy, 32, "the idle circle must stay centered vertically");
equal(circle.hw, 24, "the idle circle must render at 48px");
equal(circle.r, 24, "the idle circle must not degrade into a rounded rectangle");

const wheel = compute({ kind: "wheel" });
equal(wheel.scene.boxes.length, 7, "the wheel must include the pill and six tool nodes");
assert((wheel.path.d.match(/\bM /g) ?? []).length >= 2, "the wheel may trace multiple droplets in one SVG skin");

const panel = compute({ kind: "tool", tool: "clipboard" });
equal(panel.scene.bridges.length, 1, "large panels need an explicit liquid bridge");
equal((panel.path.d.match(/\bM /g) ?? []).length, 1, "the pill, bridge, and panel must fuse into one contour");

const upward = compute({ kind: "agents" }, 700, 620, true);
const upwardPill = upward.scene.boxes.find((box) => box.id === "pill");
const upwardPanel = upward.scene.boxes.find((box) => box.id === "panel");
assert(upwardPill && upwardPanel && upwardPill.cy > upwardPanel.cy, "upward mode must keep the pill below its panel");
equal((upward.path.d.match(/\bM /g) ?? []).length, 1, "upward agents must remain a single contour");

const crispScene = sceneFor({ kind: "tool", tool: "clipboard" }, 520, 560);
const crispEngine = new LiquidEngine();
crispEngine.setBoxes(crispScene.boxes);
crispEngine.setBridges(crispScene.bridges);
crispEngine.setParameters({ ...crispScene.parameters, k: 0 });
const crisp = crispEngine.compute().d;
crispEngine.setParameters({ ...crispScene.parameters, k: 48 });
const gooPath = crispEngine.compute();
const goo = gooPath.d;
assert(crisp !== goo, "changing Blend must recompute the fused outline");
assert(Object.is(gooPath, crispEngine.compute()), "unchanged geometry must return the cached path object");

console.log("Liquid engine geometry checks passed.");
