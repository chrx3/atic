import {
  attachNearest,
  detachObject,
  moveObject,
  nearestCandidate,
  rectGap,
  upsertObject,
  type FloatingWorld,
} from "./world";
import { connectorBridges } from "./liquid/connectors";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal<T>(actual: T, expected: T, message = "values must be equal") {
  assert(Object.is(actual, expected), `${message}: expected ${String(expected)}, got ${String(actual)}`);
}

function deepEqual(actual: unknown, expected: unknown, message = "values must be deeply equal") {
  const received = JSON.stringify(actual);
  const wanted = JSON.stringify(expected);
  assert(received === wanted, `${message}: expected ${wanted}, got ${received}`);
}

const initial: FloatingWorld = {
  objects: [
    { id: "pill", rect: { x: 100, y: 100, width: 48, height: 48 } },
    { id: "meetings", rect: { x: 160, y: 100, width: 240, height: 180 } },
    { id: "agents", rect: { x: 700, y: 100, width: 520, height: 420 } },
  ],
  groups: [],
};

equal(rectGap(initial.objects[0].rect, initial.objects[1].rect), 12);
equal(nearestCandidate(initial, "pill", 18)?.to, "meetings");
equal(nearestCandidate(initial, "pill", 8), null);

const joined = attachNearest(initial, "pill", 18);
deepEqual(joined.groups[0].members, ["pill", "meetings"]);

const moved = moveObject(joined, "meetings", 20, 30);
deepEqual(moved.objects.find((item) => item.id === "pill")?.rect, {
  x: 120,
  y: 130,
  width: 48,
  height: 48,
});
deepEqual(moved.objects.find((item) => item.id === "meetings")?.rect, {
  x: 180,
  y: 130,
  width: 240,
  height: 180,
});
deepEqual(moved.objects.find((item) => item.id === "agents")?.rect, initial.objects[2].rect);

const detached = detachObject(moved, "meetings");
equal(detached.groups.length, 0);
const freeMove = moveObject(detached, "meetings", 10, 0);
equal(freeMove.objects.find((item) => item.id === "pill")?.rect.x, 120);
equal(freeMove.objects.find((item) => item.id === "meetings")?.rect.x, 190);

const replaced = upsertObject(initial, {
  id: "agents",
  rect: { x: 500, y: 300, width: 600, height: 460 },
});
equal(replaced.objects.filter((item) => item.id === "agents").length, 1);
assert(replaced.objects.find((item) => item.id === "agents")?.rect.x !== 700, "upsert must replace the live tool");

const connectors = connectorBridges(
  [
    { id: "pill", x: 100, y: 100, width: 48, height: 48 },
    { id: "meetings", x: 155, y: 88, width: 240, height: 180 },
    { id: "agents", x: 402, y: 88, width: 520, height: 420 },
  ],
  [{ id: "group:flow", members: ["pill", "meetings", "agents"] }],
);
equal(connectors.length, 2, "a three-object group should use a minimal two-neck tree");
assert(connectors.every((bridge) => bridge.r >= 11 && bridge.r <= 21), "connector radii stay visibly liquid");
assert(connectors.every((bridge) => bridge.ax !== bridge.bx || bridge.ay !== bridge.by), "connectors have length");

const distant = connectorBridges(
  [
    { id: "pill", x: 0, y: 0, width: 48, height: 48 },
    { id: "agents", x: 500, y: 0, width: 520, height: 420 },
  ],
  [{ id: "group:distant", members: ["pill", "agents"] }],
);
equal(distant.length, 0, "a stale group must not draw a cable across the desktop");

console.log("Floating world geometry checks passed.");
