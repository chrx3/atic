/**
 * Las decisiones del editor de anotaciones, sin DOM ni canvas.
 *
 * Mismo reparto que `pill/pillPlan.ts`: acá entra estado y sale una decisión,
 * y el componente se queda con la ejecución. Lo que se gana es poder probar la
 * parte que no se ve como un error sino como «la flecha quedó torcida» o
 * «deshacer se comió un trazo de más».
 *
 * **Todo se mide en píxeles de la imagen**, nunca en píxeles de pantalla. Es
 * lo que hace que lo exportado coincida con lo dibujado aunque la ventana esté
 * a otra escala, el monitor tenga otro DPI o la imagen se muestre reducida.
 */

/**
 * Qué se dibuja. `highlight` es el mismo trazo libre, ancho y translúcido.
 *
 * `crop` está en la misma lista aunque no deje una forma: para quien usa la
 * barra es una herramienta más —se elige, se arrastra—, y tenerla afuera
 * obligaría a un segundo selector para un solo botón.
 */
export type AnnotateTool =
  | "pen"
  | "arrow"
  | "ellipse"
  | "rect"
  | "highlight"
  | "text"
  | "crop";

/** Las que dejan una forma en el documento. `crop` cambia el encuadre. */
export function drawsShape(tool: AnnotateTool): boolean {
  return tool !== "crop";
}

export interface Point {
  x: number;
  y: number;
}

interface Stroke {
  color: string;
  /** Grosor en píxeles de la imagen, ya resuelto por `strokeWidth`. */
  width: number;
}

/** Trazo a mano alzada: una lista de puntos. */
export interface FreeShape extends Stroke {
  kind: "pen" | "highlight";
  points: Point[];
}

/** Forma definida por un arrastre: de dónde a dónde. */
export interface SpanShape extends Stroke {
  kind: "arrow" | "ellipse" | "rect";
  from: Point;
  to: Point;
}

/**
 * Un texto escrito sobre la imagen.
 *
 * `at` es la esquina superior izquierda de la primera línea, no la línea base:
 * es lo que se puede colocar mirando, y lo que hace que el cuadro de edición y
 * lo dibujado caigan en el mismo sitio.
 */
export interface TextShape extends Stroke {
  kind: "text";
  at: Point;
  /** Puede traer saltos de línea. */
  text: string;
  /** Alto de la letra en píxeles de la imagen. */
  size: number;
}

/*
 * Interfaces y no intersecciones (`Stroke & { … }`): TypeScript no discrimina
 * una unión de intersecciones por su `kind`, así que dentro de un `switch`
 * todas las variantes quedaban en `never` y no se podía leer ni un campo.
 */
export type Shape = FreeShape | SpanShape | TextShape;

/** El trazo libre y el resaltador comparten forma; el resto, no. */
export function isFree(shape: Shape): shape is FreeShape {
  return shape.kind === "pen" || shape.kind === "highlight";
}

export interface AnnotateState {
  shapes: Shape[];
  /** Lo deshecho, para rehacer. Se vacía al dibujar algo nuevo. */
  undone: Shape[];
}

/**
 * Los colores, pensados para leerse sobre una captura cualquiera.
 *
 * El rojo va primero porque es el que se usa casi siempre: señalar.
 */
export const COLORS = [
  "#ff3b30",
  "#ffcc00",
  "#34c759",
  "#0a84ff",
  "#ffffff",
  "#1c1c1e",
] as const;

/** Los tres grosores del selector. El valor real depende de la imagen. */
export const WIDTH_LEVELS = [1, 2, 3] as const;
export type WidthLevel = (typeof WIDTH_LEVELS)[number];

/** Ancho de referencia: a este tamaño, el nivel 2 mide 4 px. */
const REFERENCE_WIDTH = 1280;

/** Distancia mínima entre puntos de un trazo libre, en píxeles de imagen. */
const MIN_STEP = 2;

/** Menos que esto de arrastre y la forma no llegó a existir. */
const MIN_DRAG = 4;

/** El resaltador es el mismo trazo, mucho más ancho. */
export const HIGHLIGHT_FACTOR = 5;

/**
 * Grosor real de un nivel, para una imagen de este ancho.
 *
 * Un trazo de 4 px se ve bien en una captura de 1280 y es un pelo invisible en
 * una de 3840. El grosor tiene que escalar con la imagen, o la herramienta se
 * siente distinta según el monitor donde se capturó.
 */
export function strokeWidth(level: WidthLevel, imageWidth: number): number {
  const scale = Math.max(1, imageWidth / REFERENCE_WIDTH);
  return Math.max(1, Math.round(level * 2 * scale));
}

/**
 * Alto de la letra por nivel, a la escala de la imagen.
 *
 * No sale de `strokeWidth`: un texto del grosor de un trazo sería ilegible.
 * Los tres tamaños son los de una anotación que se lee sin acercarse —una
 * etiqueta, una aclaración—, y escalan con la imagen por lo mismo que el
 * grosor.
 */
const TEXT_SIZES: Record<WidthLevel, number> = { 1: 18, 2: 26, 3: 38 };

export function textSize(level: WidthLevel, imageWidth: number): number {
  const scale = Math.max(1, imageWidth / REFERENCE_WIDTH);
  return Math.max(8, Math.round(TEXT_SIZES[level] * scale));
}

/** Alto de renglón. El interlineado va con el cuerpo, no fijo en píxeles. */
export const LINE_HEIGHT = 1.25;

export function emptyState(): AnnotateState {
  return { shapes: [], undone: [] };
}

/** Las que nacen de un arrastre. El texto se escribe y el recorte encuadra. */
export type DragTool = "pen" | "arrow" | "ellipse" | "rect" | "highlight";

export function isDragTool(tool: AnnotateTool): tool is DragTool {
  return tool !== "text" && tool !== "crop";
}

/** Arranca una forma en `at`. Todavía no está en el estado. */
export function beginShape(
  tool: DragTool,
  color: string,
  width: number,
  at: Point,
): Shape {
  if (tool === "pen" || tool === "highlight") {
    return { kind: tool, color, width, points: [at] };
  }
  return { kind: tool, color, width, from: at, to: at };
}

/**
 * Sigue la forma hasta `at`.
 *
 * Devuelve una forma nueva en vez de mutarla: el componente la tiene en
 * `$state` y Svelte necesita ver la asignación para redibujar.
 */
export function extendShape(shape: Shape, at: Point): Shape {
  // El texto no se arrastra: se coloca una vez y después se escribe.
  if (shape.kind === "text") return shape;
  if (isFree(shape)) {
    const last = shape.points[shape.points.length - 1];
    // Sin el filtro, un arrastre lento mete cientos de puntos por segundo: el
    // trazo pesa y se dibuja de más sin verse distinto.
    if (last && Math.hypot(at.x - last.x, at.y - last.y) < MIN_STEP) return shape;
    return { ...shape, points: [...shape.points, at] };
  }
  return { ...shape, to: at };
}

/** Una forma que no llegó a nada: un clic suelto, un temblor de mano. */
export function isDegenerate(shape: Shape): boolean {
  if (isFree(shape)) {
    return shape.points.length < 2;
  }
  // Un cuadro de texto que se cerró sin escribir: colocarlo y arrepentirse es
  // el gesto más normal del mundo y no tiene que dejar nada.
  if (shape.kind === "text") return shape.text.trim().length === 0;
  return Math.hypot(shape.to.x - shape.from.x, shape.to.y - shape.from.y) < MIN_DRAG;
}

/**
 * Fija una forma terminada.
 *
 * Dibujar algo nuevo **borra el rehacer**: es la regla de cualquier editor, y
 * sin ella «rehacer» resucitaría un trazo que ya no tiene sentido al lado de
 * lo que se dibujó después.
 */
export function commit(state: AnnotateState, shape: Shape): AnnotateState {
  if (isDegenerate(shape)) return state;
  return { shapes: [...state.shapes, shape], undone: [] };
}

export function undo(state: AnnotateState): AnnotateState {
  if (state.shapes.length === 0) return state;
  const last = state.shapes[state.shapes.length - 1];
  return { shapes: state.shapes.slice(0, -1), undone: [...state.undone, last] };
}

export function redo(state: AnnotateState): AnnotateState {
  if (state.undone.length === 0) return state;
  const last = state.undone[state.undone.length - 1];
  return { shapes: [...state.shapes, last], undone: state.undone.slice(0, -1) };
}

/**
 * Punto del ratón → píxel de la imagen.
 *
 * Es el mismo cálculo que hace `toFrame` en el overlay de captura, y por el
 * mismo motivo: el lienzo se muestra escalado, así que el rectángulo del DOM
 * es la única fuente fiable de la escala real. Usar el `scale_factor` de la
 * ventana falla en cuanto el DPI del monitor y el del webview no coinciden.
 */
export function toImagePoint(
  client: Point,
  rect: { left: number; top: number; width: number; height: number },
  natural: { width: number; height: number },
): Point {
  const w = rect.width > 0 ? rect.width : natural.width;
  const h = rect.height > 0 ? rect.height : natural.height;
  return {
    x: ((client.x - rect.left) * natural.width) / w,
    y: ((client.y - rect.top) * natural.height) / h,
  };
}

/** Deja el punto dentro de la imagen: dibujar fuera del lienzo no existe. */
export function clampToImage(
  point: Point,
  natural: { width: number; height: number },
): Point {
  return {
    x: Math.min(Math.max(point.x, 0), natural.width),
    y: Math.min(Math.max(point.y, 0), natural.height),
  };
}

/** Los puntos de la punta de una flecha, en píxeles de imagen. */
export interface ArrowHead {
  tip: Point;
  left: Point;
  right: Point;
  /** Dónde termina la línea: dentro de la punta, para que no la asome. */
  neck: Point;
}

/**
 * Geometría de la punta de una flecha.
 *
 * La punta crece con el grosor y no con el largo: una flecha corta y gruesa
 * tiene que verse como una flecha, no como un triángulo con un pelo atrás. Y
 * nunca es más larga que la flecha entera, o la punta se pasaría del origen.
 */
export function arrowHead(from: Point, to: Point, width: number): ArrowHead {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const length = Math.hypot(dx, dy);
  if (length === 0) {
    return { tip: to, left: to, right: to, neck: to };
  }
  const ux = dx / length;
  const uy = dy / length;
  const head = Math.min(Math.max(width * 3.6, 10), length);
  const half = head * 0.45;
  const neck = { x: to.x - ux * head, y: to.y - uy * head };
  return {
    tip: to,
    neck,
    left: { x: neck.x - uy * half, y: neck.y + ux * half },
    right: { x: neck.x + uy * half, y: neck.y - ux * half },
  };
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Rectángulo normalizado: el arrastre puede ir en cualquier dirección. */
export function normalizeRect(from: Point, to: Point): Rect {
  return {
    x: Math.min(from.x, to.x),
    y: Math.min(from.y, to.y),
    w: Math.abs(to.x - from.x),
    h: Math.abs(to.y - from.y),
  };
}

/** Lo más chico que puede quedar un recorte, en píxeles de la imagen. */
export const MIN_CROP = 24;

/**
 * El recuadro de un arrastre de recorte, encajado dentro de `bounds`.
 *
 * `null` cuando quedó tan chico que fue un clic: recortar es destructivo a la
 * vista, y un temblor no puede dejar la imagen en una estampilla.
 *
 * Se redondea a enteros porque de acá sale el tamaño de un canvas, y un canvas
 * de 320.4 px trunca —lo dibujado en esa última fracción no existe—.
 */
export function cropRect(from: Point, to: Point, bounds: Rect): Rect | null {
  const r = normalizeRect(from, to);
  const x = Math.max(r.x, bounds.x);
  const y = Math.max(r.y, bounds.y);
  const w = Math.min(r.x + r.w, bounds.x + bounds.w) - x;
  const h = Math.min(r.y + r.h, bounds.y + bounds.h) - y;
  if (w < MIN_CROP || h < MIN_CROP) return null;
  return {
    x: Math.round(x),
    y: Math.round(y),
    w: Math.round(w),
    h: Math.round(h),
  };
}

/** Qué herramienta pide una tecla suelta. `null` = no es del editor. */
export function toolForKey(key: string): AnnotateTool | null {
  switch (key.toLowerCase()) {
    case "1":
    case "p":
      return "pen";
    case "2":
    case "f":
      return "arrow";
    case "3":
    case "c":
      return "ellipse";
    case "4":
    case "r":
      return "rect";
    case "5":
    case "m":
      return "highlight";
    case "6":
    case "t":
      return "text";
    case "7":
    case "x":
      return "crop";
    default:
      return null;
  }
}
