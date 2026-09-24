/**
 * La pizarra de consolas: geometría y lo que se le manda a cada una.
 *
 * Las consolas viven en un plano sin bordes (coordenadas de pizarra) que se
 * mira con una cámara: un corrimiento en píxeles de pantalla y un zoom
 * (`pantalla = pizarra × zoom + corrimiento`). Acá solo cuentas puras: dónde
 * nace una consola, cómo se encuadra, cómo se acomodan y cómo se escribe el
 * texto de la entrada única para que el CLI lo reciba como un solo mensaje.
 */

export type Rect = { x: number; y: number; w: number; h: number };
export type Point = { x: number; y: number };
export type Size = { w: number; h: number };
export type Camera = { x: number; y: number; zoom: number };
/** Lo que tapan los flotantes en cada borde de la ventana, en pantalla. */
export type Insets = { top: number; right: number; bottom: number; left: number };

/** Por debajo de esto el TUI del agente se dibuja roto. */
export const MIN_W = 420;
export const MIN_H = 260;
export const DEFAULT_SIZE = { w: 760, h: 480 };
export const MIN_ZOOM = 0.15;
export const MAX_ZOOM = 1.6;
/** Separación entre consolas al acomodarlas. */
export const GAP = 28;
/** Cuánto se corre cada consola nueva respecto de la anterior en cascada. */
const CASCADE = 36;
const NO_INSETS: Insets = { top: 0, right: 0, bottom: 0, left: 0 };

export function clampZoom(zoom: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

/** Un rectángulo guardado, si tiene forma de tal; si no, `null`. */
export function parseRect(value: unknown): Rect | null {
  if (!value || typeof value !== "object") return null;
  const o = value as Record<string, unknown>;
  const nums = [o.x, o.y, o.w, o.h];
  if (!nums.every((n) => typeof n === "number" && Number.isFinite(n))) return null;
  return clampSize({
    x: o.x as number,
    y: o.y as number,
    w: o.w as number,
    h: o.h as number,
  });
}

/** Una cámara guardada, o `null` si no tiene forma. */
export function parseCamera(value: unknown): Camera | null {
  if (!value || typeof value !== "object") return null;
  const o = value as Record<string, unknown>;
  if (![o.x, o.y].every((n) => typeof n === "number" && Number.isFinite(n)))
    return null;
  const zoom = typeof o.zoom === "number" && Number.isFinite(o.zoom) ? o.zoom : 1;
  return { x: o.x as number, y: o.y as number, zoom: clampZoom(zoom) };
}

export function clampSize(rect: Rect): Rect {
  return { ...rect, w: Math.max(MIN_W, rect.w), h: Math.max(MIN_H, rect.h) };
}

/** La parte de la pizarra que se ve, descontando lo que tapan los flotantes. */
export function visibleArea(cam: Camera, view: Size, insets: Insets = NO_INSETS): Rect {
  return {
    x: (insets.left - cam.x) / cam.zoom,
    y: (insets.top - cam.y) / cam.zoom,
    w: Math.max(1, view.w - insets.left - insets.right) / cam.zoom,
    h: Math.max(1, view.h - insets.top - insets.bottom) / cam.zoom,
  };
}

/**
 * Dónde nace una consola: centrada en lo que se está mirando y, si ahí ya
 * hay una en la misma esquina, en cascada hacia abajo a la derecha. Nunca
 * encima exacta de otra: taparía la que el usuario estaba usando.
 */
export function placeNew(
  existing: Rect[],
  area: Rect,
  size: Size = DEFAULT_SIZE,
): Rect {
  const w = Math.min(size.w, Math.max(MIN_W, area.w - 48));
  const h = Math.min(size.h, Math.max(MIN_H, area.h - 48));
  let x = Math.round(area.x + (area.w - w) / 2);
  let y = Math.round(area.y + (area.h - h) / 2);
  for (let i = 0; i < 40; i++) {
    const taken = existing.some(
      (r) => Math.abs(r.x - x) < CASCADE / 2 && Math.abs(r.y - y) < CASCADE / 2,
    );
    if (!taken) break;
    x += CASCADE;
    y += CASCADE;
  }
  return { x, y, w, h };
}

/**
 * Zoom con un punto de pantalla quieto (el cursor, o el centro): lo que está
 * bajo el puntero sigue bajo el puntero, como en cualquier lienzo.
 */
export function zoomAt(cam: Camera, zoom: number, at: Point): Camera {
  const next = clampZoom(zoom);
  const k = next / cam.zoom;
  return { x: at.x - (at.x - cam.x) * k, y: at.y - (at.y - cam.y) * k, zoom: next };
}

/**
 * La cámara que muestra `rect` entero y centrado en el área libre, con
 * margen. Nunca acerca más que `maxZoom`: encuadrar una consola chica no
 * debería agrandarla por encima de su tamaño real.
 */
export function fitRect(
  rect: Rect,
  view: Size,
  insets: Insets = NO_INSETS,
  maxZoom = 1,
  margin = 32,
): Camera {
  const freeW = Math.max(1, view.w - insets.left - insets.right - margin * 2);
  const freeH = Math.max(1, view.h - insets.top - insets.bottom - margin * 2);
  const zoom = clampZoom(Math.min(maxZoom, freeW / rect.w, freeH / rect.h));
  const cx = insets.left + (view.w - insets.left - insets.right) / 2;
  const cy = insets.top + (view.h - insets.top - insets.bottom) / 2;
  return {
    x: Math.round(cx - (rect.x + rect.w / 2) * zoom),
    y: Math.round(cy - (rect.y + rect.h / 2) * zoom),
    zoom,
  };
}

/** La caja que junta a todas; `null` si no hay ninguna. */
export function bounds(rects: Rect[]): Rect | null {
  if (rects.length === 0) return null;
  const x = Math.min(...rects.map((r) => r.x));
  const y = Math.min(...rects.map((r) => r.y));
  const right = Math.max(...rects.map((r) => r.x + r.w));
  const bottom = Math.max(...rects.map((r) => r.y + r.h));
  return { x, y, w: right - x, h: bottom - y };
}

/** ¿Se ve entera en el área libre, al zoom de ahora? */
export function fullyVisible(
  rect: Rect,
  cam: Camera,
  view: Size,
  insets: Insets = NO_INSETS,
): boolean {
  const area = visibleArea(cam, view, insets);
  return (
    rect.x >= area.x - 1 &&
    rect.y >= area.y - 1 &&
    rect.x + rect.w <= area.x + area.w + 1 &&
    rect.y + rect.h <= area.y + area.h + 1
  );
}

/**
 * La consola que el usuario está mirando: la que tiene el centro del área
 * libre encima; si ninguna, la que más se ve. `-1` si no se ve ninguna.
 */
export function focusedByView(
  rects: Rect[],
  cam: Camera,
  view: Size,
  insets: Insets = NO_INSETS,
): number {
  const area = visibleArea(cam, view, insets);
  const cx = area.x + area.w / 2;
  const cy = area.y + area.h / 2;
  const under = rects.findIndex(
    (r) => cx >= r.x && cx <= r.x + r.w && cy >= r.y && cy <= r.y + r.h,
  );
  if (under >= 0) return under;
  let best = -1;
  let bestArea = 0;
  rects.forEach((r, i) => {
    const w = Math.min(r.x + r.w, area.x + area.w) - Math.max(r.x, area.x);
    const h = Math.min(r.y + r.h, area.y + area.h) - Math.max(r.y, area.y);
    const seen = Math.max(0, w) * Math.max(0, h);
    if (seen > bestArea) {
      bestArea = seen;
      best = i;
    }
  });
  return best;
}

export type Arrangement = "row" | "column" | "grid";

/** El fondo de la pizarra: se mueve y escala con la cámara, salvo el liso. */
export const BACKDROPS = ["dots", "grid", "plain"] as const;
export type Backdrop = (typeof BACKDROPS)[number];
/** Lado de la baldosa del fondo en tamaño real; la cuadrícula va más abierta. */
export const BACKDROP_TILE: Record<Backdrop, number> = {
  dots: 24,
  grid: 48,
  plain: 24,
};

/**
 * Acomoda las consolas sin superponerse, en el orden dado y desde la esquina
 * de arriba a la izquierda de la que ya estaba más arriba a la izquierda:
 * ordenar no debería mandarlas a otro lugar de la pizarra.
 *
 * - `row`: una junto a otra, alineadas arriba.
 * - `column`: una debajo de otra, alineadas a la izquierda.
 * - `grid`: filas de tamaño parejo (√n columnas), cada fila tan alta como su
 *   consola más alta.
 */
export function arrange(rects: Rect[], mode: Arrangement, gap = GAP): Rect[] {
  const box = bounds(rects);
  if (!box) return [];
  const cols =
    mode === "row"
      ? rects.length
      : mode === "column"
        ? 1
        : Math.ceil(Math.sqrt(rects.length));
  const out: Rect[] = [];
  let y = box.y;
  for (let start = 0; start < rects.length; start += cols) {
    const row = rects.slice(start, start + cols);
    let x = box.x;
    for (const r of row) {
      out.push({ ...r, x, y });
      x += r.w + gap;
    }
    y += Math.max(...row.map((r) => r.h)) + gap;
  }
  return out;
}

/** Mover o redimensionar desde un borde, con el tamaño mínimo respetado. */
export type Handle = "move" | "e" | "s" | "se" | "w" | "sw";

export function dragRect(start: Rect, handle: Handle, dx: number, dy: number): Rect {
  if (handle === "move") return { ...start, x: start.x + dx, y: start.y + dy };
  let { x, w, h } = start;
  if (handle === "e" || handle === "se") w = start.w + dx;
  if (handle === "s" || handle === "se" || handle === "sw") h = start.h + dy;
  if (handle === "w" || handle === "sw") {
    // Desde la izquierda el borde derecho queda quieto.
    w = Math.max(MIN_W, start.w - dx);
    x = start.x + start.w - w;
  }
  return { x, y: start.y, w: Math.max(MIN_W, w), h: Math.max(MIN_H, h) };
}

/**
 * Lo que se escribe en la PTY para mandar `text` como un solo mensaje.
 *
 * A un CLI de agente se le pega entre marcas de *bracketed paste*: sin ellas
 * cada salto de línea sería un Enter y el mensaje saldría partido. La shell
 * del sistema (cmd, PowerShell) no entiende esas marcas y las mostraría, así
 * que a ella va el texto tal cual. El Enter va aparte: dentro del pegado el
 * TUI lo tomaría como parte del texto.
 */
export function composerWrites(text: string, agentCli: boolean): [string, string] {
  const body = text.replace(/\r\n?/g, "\n");
  if (!agentCli) return [body.replace(/\n/g, " "), "\r"];
  return [`\x1b[200~${body}\x1b[201~`, "\r"];
}

/**
 * Lo mandado por la entrada, del más viejo al más nuevo. Repetir un mensaje
 * lo sube al final en vez de duplicarlo; lo vacío no se guarda.
 */
export function pushHistory(list: string[], text: string, max = 50): string[] {
  const entry = text.trim();
  if (!entry) return list;
  return [...list.filter((t) => t !== entry), entry].slice(-max);
}

/**
 * Rutas soltadas en una consola, listas para pegar: separadas por espacio y
 * entre comillas las que tienen espacios, como las escribiría una shell.
 */
export function quotePaths(paths: string[]): string {
  return paths
    .filter((p) => p.trim())
    .map((p) => (/\s/.test(p) ? `"${p}"` : p))
    .join(" ");
}

/** La de más arriba que contiene el punto (en coordenadas de pizarra). */
export function cardAt(
  entries: { key: string; rect: Rect; z: number }[],
  at: Point,
): string | null {
  let best: { key: string; z: number } | null = null;
  for (const { key, rect, z } of entries) {
    const inside =
      at.x >= rect.x &&
      at.x <= rect.x + rect.w &&
      at.y >= rect.y &&
      at.y <= rect.y + rect.h;
    if (inside && (!best || z > best.z)) best = { key, z };
  }
  return best?.key ?? null;
}

/**
 * Cómo cabe un pedazo de pizarra (`world`) en una caja de `box`: escala y
 * corrimiento para dibujarlo centrado, sin deformar.
 */
export function fitInto(
  world: Rect,
  box: Size,
): { scale: number; x: number; y: number } {
  const scale = Math.min(box.w / Math.max(1, world.w), box.h / Math.max(1, world.h));
  return {
    scale,
    x: (box.w - world.w * scale) / 2 - world.x * scale,
    y: (box.h - world.h * scale) / 2 - world.y * scale,
  };
}

/** La cámara que deja el punto `at` (en pizarra) al centro del área libre. */
export function centerOn(
  at: Point,
  cam: Camera,
  view: Size,
  insets: Insets = NO_INSETS,
): Camera {
  const cx = insets.left + (view.w - insets.left - insets.right) / 2;
  const cy = insets.top + (view.h - insets.top - insets.bottom) / 2;
  return { ...cam, x: cx - at.x * cam.zoom, y: cy - at.y * cam.zoom };
}

/** Una consola dentro de una sesión guardada: lo necesario para reabrirla. */
export type SpaceConsole = {
  label: string;
  cli: string | null;
  command: string | null;
  cwd: string | null;
  rect: Rect;
  /**
   * La conversación del agente al guardar (su id de sesión): al reabrir se
   * retoma en vez de empezar de cero.
   */
  resume?: string;
};

/** Una disposición de consolas guardada con nombre, para reabrirla entera. */
export type SavedSpace = {
  name: string;
  savedAt: number;
  layout: Arrangement | "free";
  consoles: SpaceConsole[];
};

const LAYOUTS = new Set(["row", "column", "grid", "free"]);

/**
 * Un id de sesión: va a la línea de comando, así que nada más que letras,
 * números, `-` y `_` (los UUID de casi todos, los `ses_…` de OpenCode).
 */
const RESUME_ID = /^[a-z0-9][a-z0-9_-]{7,63}$/i;

/** Lo que se agrega al comando de cada CLI para retomar una conversación. */
const RESUME_ARGS: Record<string, (id: string) => string> = {
  claude: (id) => `--resume ${id}`,
  codex: (id) => `resume ${id}`,
  grok: (id) => `-r ${id}`,
  agy: (id) => `--conversation ${id}`,
  "cursor-agent": (id) => `--resume ${id}`,
  opencode: (id) => `--session ${id}`,
};

/** ¿Este CLI sabe retomar una conversación? */
export function canResume(cli: string | null): boolean {
  return !!cli && cli in RESUME_ARGS;
}

/** Con qué se abre una consola guardada: retomando su conversación si la hay. */
export function launchCommand(item: SpaceConsole): string | null {
  const args = item.cli ? RESUME_ARGS[item.cli] : undefined;
  if (item.command && args && item.resume && RESUME_ID.test(item.resume))
    return `${item.command} ${args(item.resume)}`;
  return item.command;
}

/** Lo guardado, sin lo que esté roto: una sesión sin consolas no sirve. */
export function parseSpaces(raw: string | null): SavedSpace[] {
  let value: unknown;
  try {
    value = JSON.parse(raw ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(value)) return [];
  return value.flatMap((entry): SavedSpace[] => {
    if (!entry || typeof entry !== "object") return [];
    const o = entry as Record<string, unknown>;
    if (typeof o.name !== "string" || !o.name.trim() || !Array.isArray(o.consoles))
      return [];
    const consoles = o.consoles.flatMap((c): SpaceConsole[] => {
      if (!c || typeof c !== "object") return [];
      const k = c as Record<string, unknown>;
      const rect = parseRect(k.rect);
      if (!rect || typeof k.label !== "string") return [];
      const str = (v: unknown) => (typeof v === "string" ? v : null);
      return [
        {
          label: k.label,
          cli: str(k.cli),
          command: str(k.command),
          cwd: str(k.cwd),
          rect,
          ...(typeof k.resume === "string" && RESUME_ID.test(k.resume)
            ? { resume: k.resume }
            : {}),
        },
      ];
    });
    if (consoles.length === 0) return [];
    const layout =
      typeof o.layout === "string" && LAYOUTS.has(o.layout) ? o.layout : "free";
    return [
      {
        name: o.name.trim(),
        savedAt: typeof o.savedAt === "number" ? o.savedAt : 0,
        layout: layout as SavedSpace["layout"],
        consoles,
      },
    ];
  });
}

/** Guardar con un nombre que ya existe lo reemplaza; lo más nuevo, primero. */
export function upsertSpace(spaces: SavedSpace[], space: SavedSpace): SavedSpace[] {
  const key = space.name.trim().toLowerCase();
  return [space, ...spaces.filter((s) => s.name.trim().toLowerCase() !== key)];
}

/**
 * Dónde abrir una sesión guardada: donde estaba si la pizarra está vacía;
 * si no, a la derecha de lo que ya hay, alineado arriba, para no tapar nada.
 */
export function placeSpace(saved: Rect[], existing: Rect[], gap = GAP * 3): Rect[] {
  const from = bounds(saved);
  const taken = bounds(existing);
  if (!from || !taken) return saved;
  const dx = taken.x + taken.w + gap - from.x;
  const dy = taken.y - from.y;
  return saved.map((r) => ({ ...r, x: r.x + dx, y: r.y + dy }));
}

/** El hilo entre una consola y el sub-agente que abrió. */
export type Thread = {
  id: string;
  from: Rect;
  to: Rect;
  /** `out`: trabajando; `back`: devolvió algo sin mirar; `idle`: quieto. */
  flow: "out" | "back" | "idle";
  /** Backend del sub-agente, para su color (`data-agent`). */
  tone: string | null;
  label: string;
};

/** Tamaño de la tarjeta de un sub-agente al nacer. */
export const CHILD_SIZE = { w: 520, h: 420 };
/** Distancia horizontal entre quien pide y sus sub-agentes. */
export const CHILD_GAP = 140;

/**
 * Dónde va el sub-agente número `index` de una consola: a su derecha, en
 * columna. `offset` es cuánto lo corrió el usuario (y su tamaño, si lo
 * cambió): se sigue moviendo con la consola.
 */
export function childRect(
  parent: Rect,
  index: number,
  offset: { dx: number; dy: number; w?: number; h?: number } | undefined,
): Rect {
  const w = offset?.w ?? CHILD_SIZE.w;
  const h = offset?.h ?? CHILD_SIZE.h;
  return {
    x: parent.x + parent.w + CHILD_GAP + (offset?.dx ?? 0),
    y: parent.y + index * (CHILD_SIZE.h + GAP) + (offset?.dy ?? 0),
    w,
    h,
  };
}

/**
 * La consola de la pizarra que pidió una sesión, a partir de su `parent`:
 * el id de otra sesión (un sub-agente que delegó) o `external:<cli>:<pid>`,
 * que Rust traduce a consola subiendo por el árbol de procesos.
 */
export function parentKind(
  parent: string | null | undefined,
): "external" | "session" | null {
  if (!parent) return null;
  return parent.startsWith("external:") ? "external" : "session";
}
