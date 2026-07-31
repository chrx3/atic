import type { ToolId } from "$lib/tools";

export type FloatingObjectId = "pill" | ToolId;

export type WorldRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type FloatingObject = {
  id: FloatingObjectId;
  rect: WorldRect;
};

export type FloatingGroup = {
  id: string;
  members: FloatingObjectId[];
};

export type FloatingWorld = {
  objects: FloatingObject[];
  groups: FloatingGroup[];
};

export type ProximityCandidate = {
  from: FloatingObjectId;
  to: FloatingObjectId;
  gap: number;
};

const unique = <T>(items: T[]) => [...new Set(items)];

function byId(world: FloatingWorld, id: FloatingObjectId): FloatingObject | undefined {
  return world.objects.find((object) => object.id === id);
}

function groupOf(world: FloatingWorld, id: FloatingObjectId): FloatingGroup | undefined {
  return world.groups.find((group) => group.members.includes(id));
}

/** Distance between the closest points of two axis-aligned surface bounds.
 * Zero means that the surfaces overlap. The SDF renderer owns the organic
 * contour; this model only decides when a magnetic relationship is eligible. */
export function rectGap(a: WorldRect, b: WorldRect): number {
  const dx = Math.max(a.x - (b.x + b.width), b.x - (a.x + a.width), 0);
  const dy = Math.max(a.y - (b.y + b.height), b.y - (a.y + a.height), 0);
  return Math.hypot(dx, dy);
}

/** Finds the nearest surface outside the dragged surface's current group.
 * Passing close can render a liquid neck immediately, but grouping is only
 * committed on pointer release through attachNearest(). */
export function nearestCandidate(
  world: FloatingWorld,
  movingId: FloatingObjectId,
  maxGap: number,
): ProximityCandidate | null {
  const moving = byId(world, movingId);
  if (!moving) return null;
  const ownMembers = new Set(groupOf(world, movingId)?.members ?? [movingId]);

  let nearest: ProximityCandidate | null = null;
  for (const candidate of world.objects) {
    if (ownMembers.has(candidate.id)) continue;
    const gap = rectGap(moving.rect, candidate.rect);
    if (gap > maxGap || (nearest && gap >= nearest.gap)) continue;
    nearest = { from: movingId, to: candidate.id, gap };
  }
  return nearest;
}

function normalizedGroups(groups: FloatingGroup[]): FloatingGroup[] {
  return groups
    .map((group) => ({ ...group, members: unique(group.members) }))
    .filter((group) => group.members.length > 1);
}

/** Commits the closest magnetic join. Existing groups merge, so dragging any
 * member afterwards moves the complete connected liquid cluster. */
export function attachNearest(
  world: FloatingWorld,
  movingId: FloatingObjectId,
  maxGap: number,
): FloatingWorld {
  const candidate = nearestCandidate(world, movingId, maxGap);
  if (!candidate) return world;

  const fromGroup = groupOf(world, candidate.from);
  const toGroup = groupOf(world, candidate.to);
  const members = unique([
    ...(fromGroup?.members ?? [candidate.from]),
    ...(toGroup?.members ?? [candidate.to]),
  ]);
  const removed = new Set([fromGroup?.id, toGroup?.id].filter(Boolean));
  const nextId = fromGroup?.id ?? toGroup?.id ?? `group:${members.slice().sort().join("+")}`;

  return {
    objects: world.objects,
    groups: normalizedGroups([
      ...world.groups.filter((group) => !removed.has(group.id)),
      { id: nextId, members },
    ]),
  };
}

/** Moves one free surface or its entire connected group. */
export function moveObject(
  world: FloatingWorld,
  id: FloatingObjectId,
  dx: number,
  dy: number,
): FloatingWorld {
  const members = new Set(groupOf(world, id)?.members ?? [id]);
  return {
    ...world,
    objects: world.objects.map((object) =>
      members.has(object.id)
        ? {
            ...object,
            rect: { ...object.rect, x: object.rect.x + dx, y: object.rect.y + dy },
          }
        : object,
    ),
  };
}

/** Separates one surface from its cluster. The UI exposes this as a visible
 * action and as Alt+drag; ordinary drag deliberately keeps the group alive. */
export function detachObject(world: FloatingWorld, id: FloatingObjectId): FloatingWorld {
  const current = groupOf(world, id);
  if (!current) return world;
  return {
    objects: world.objects,
    groups: normalizedGroups(
      world.groups.map((group) =>
        group.id === current.id
          ? { ...group, members: group.members.filter((member) => member !== id) }
          : group,
      ),
    ),
  };
}

/** Product invariant: one live object per tool, plus the pill. Reopening an
 * existing tool focuses it instead of creating a duplicate surface. */
export function upsertObject(world: FloatingWorld, object: FloatingObject): FloatingWorld {
  const exists = world.objects.some((current) => current.id === object.id);
  return {
    ...world,
    objects: exists
      ? world.objects.map((current) => (current.id === object.id ? object : current))
      : [...world.objects, object],
  };
}
