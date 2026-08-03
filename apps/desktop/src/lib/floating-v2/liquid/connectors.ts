import type { LiquidBridge } from "./engine";

export type ConnectorObject = {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

export type ConnectorGroup = {
  id: string;
  members: string[];
};

type Pair = {
  from: ConnectorObject;
  to: ConnectorObject;
  gap: number;
};

const MAX_CONNECTOR_REACH = 72;

const centerX = (rect: ConnectorObject) => rect.x + rect.width / 2;
const centerY = (rect: ConnectorObject) => rect.y + rect.height / 2;
const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

function rectGap(a: ConnectorObject, b: ConnectorObject) {
  const dx = Math.max(a.x - (b.x + b.width), b.x - (a.x + a.width), 0);
  const dy = Math.max(a.y - (b.y + b.height), b.y - (a.y + a.height), 0);
  return Math.hypot(dx, dy);
}

function nearestPair(from: ConnectorObject, to: ConnectorObject): Pair {
  return { from, to, gap: rectGap(from, to) };
}

function connectionPoints(from: ConnectorObject, to: ConnectorObject) {
  let ax = clamp(centerX(to), from.x, from.x + from.width);
  let ay = clamp(centerY(to), from.y, from.y + from.height);
  let bx = clamp(centerX(from), to.x, to.x + to.width);
  let by = clamp(centerY(from), to.y, to.y + to.height);

  let dx = bx - ax;
  let dy = by - ay;
  let distance = Math.hypot(dx, dy);

  // Rectángulos solapados o perfectamente alineados pueden producir el mismo
  // punto. En ese caso elegimos el eje dominante entre sus centros.
  if (distance < 0.5) {
    const centerDx = centerX(to) - centerX(from);
    const centerDy = centerY(to) - centerY(from);
    if (Math.abs(centerDx) >= Math.abs(centerDy)) {
      const direction = centerDx >= 0 ? 1 : -1;
      ax = centerX(from) + direction * from.width / 2;
      ay = clamp(centerY(to), from.y, from.y + from.height);
      bx = centerX(to) - direction * to.width / 2;
      by = clamp(centerY(from), to.y, to.y + to.height);
    } else {
      const direction = centerDy >= 0 ? 1 : -1;
      ax = clamp(centerX(to), from.x, from.x + from.width);
      ay = centerY(from) + direction * from.height / 2;
      bx = clamp(centerX(from), to.x, to.x + to.width);
      by = centerY(to) - direction * to.height / 2;
    }
    dx = bx - ax;
    dy = by - ay;
    distance = Math.hypot(dx, dy);
  }

  const ux = distance > 0 ? dx / distance : 1;
  const uy = distance > 0 ? dy / distance : 0;
  const underlap = Math.min(14, Math.max(9, distance * 0.3));

  return {
    ax: ax - ux * underlap,
    ay: ay - uy * underlap,
    bx: bx + ux * underlap,
    by: by + uy * underlap,
  };
}

function bridgeFor(groupId: string, pair: Pair): LiquidBridge | null {
  if (pair.gap > MAX_CONNECTOR_REACH) return null;
  const points = connectionPoints(pair.from, pair.to);
  const ids = [pair.from.id, pair.to.id].sort();
  return {
    id: `${groupId}:${ids.join("+")}`,
    ...points,
    // Cerca de la unión magnética (7 px) el cuello mide ~40 px: suficiente para
    // que la unión se lea como materia líquida aunque cada ventana pinte su
    // propio cuerpo. Al alejarse se afina antes de romperse.
    r: Math.max(11, Math.min(21, 21 - pair.gap * 0.16)),
  };
}

/**
 * Crea un árbol mínimo por grupo. Así todos los miembros cercanos conservan un
 * vínculo líquido sin dibujar cables redundantes ni cuerpos duplicados.
 */
export function connectorBridges(
  objects: ConnectorObject[],
  groups: ConnectorGroup[],
): LiquidBridge[] {
  const byId = new Map(objects.map((object) => [object.id, object]));
  const bridges: LiquidBridge[] = [];

  for (const group of groups) {
    const members = group.members
      .map((id) => byId.get(id))
      .filter((object): object is ConnectorObject => Boolean(object));
    if (members.length < 2) continue;

    const connected = [members[0]];
    const remaining = members.slice(1);
    while (remaining.length > 0) {
      let best: Pair | null = null;
      let bestIndex = 0;
      for (const from of connected) {
        remaining.forEach((to, index) => {
          const candidate = nearestPair(from, to);
          if (!best || candidate.gap < best.gap) {
            best = candidate;
            bestIndex = index;
          }
        });
      }
      const next = remaining.splice(bestIndex, 1)[0];
      connected.push(next);
      if (best) {
        const bridge = bridgeFor(group.id, best);
        if (bridge) bridges.push(bridge);
      }
    }
  }

  return bridges;
}
