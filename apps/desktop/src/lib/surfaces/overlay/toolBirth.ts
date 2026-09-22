/**
 * Ancla de nacimiento de un float.
 *
 * Clipboard / textos se anclan al cursor (atajo o rueda). Launcher /
 * agentes usan la pill para elegir el monitor y se centran. El reveal
 * corre después: si midiera la pill en vivo, nacerían en el notch.
 */

export type BirthRect = { x: number; y: number; w: number; h: number };

/** Disco del tamaño de la pill centrado en el cursor. */
export function birthAtCursor(
  cursor: { x: number; y: number },
  size: { w: number; h: number },
): BirthRect {
  return {
    x: cursor.x - size.w / 2,
    y: cursor.y - size.h / 2,
    w: size.w,
    h: size.h,
  };
}

let birth: BirthRect | null = null;
/** Reposo EXACTO pedido por un despegue: el float nace y descansa ahí. */
let rest: BirthRect | null = null;
let resting = false;
let restingResolvers: Array<() => void> = [];

export function captureToolBirth(rect: BirthRect | null): void {
  birth = rect ? { ...rect } : null;
  // La limpieza del acto (null) apaga también el reposo: el birth ya
  // consumió el rect de la cara que lo pidió.
  if (!birth) rest = null;
  resting = false;
}

/** Marca el rect EXACTO donde el float debe nacer y descansar. */
export function captureToolResting(rect: BirthRect | null): void {
  rest = rect ? { ...rect } : null;
}

/** Rect de reposo del despegue en curso, si hay. */
export function toolResting(): BirthRect | null {
  return rest;
}

export function toolBirth(): BirthRect | null {
  return birth;
}

export function notifyToolResting(): void {
  resting = true;
  const pending = restingResolvers;
  restingResolvers = [];
  for (const resolve of pending) resolve();
}

/** Espera a que el float aplique su reposo, o el timeout si no hay float. */
export function waitToolResting(ms = 700): Promise<void> {
  if (resting) return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(() => {
      restingResolvers = restingResolvers.filter((r) => r !== done);
      resolve();
    }, ms);
    restingResolvers.push(done);
  });
}

/**
 * Compuerta del traspaso del gesto de detach al motor de arrastre.
 *
 * La pill sondea el handoff por cuadro y el float acepta en cuanto tiene
 * marco — pero ese marco puede ser el VIEJO (dockear y cerrar apagan el
 * globo sin limpiar su anchor). Aceptar ya sembraría el arrastre con la
 * última posición del float, antes de que el evento de Rust lo coloque en
 * el rect de la cara, y el panel seguiría a la mano con ese offset todo el
 * gesto. Con reposo pendiente y marco sin aplicar, se pide reintentar.
 */
export type DetachFrame = { x: number; y: number; w: number; h: number };

export type DetachHandoffGate = { waited: number };

/** Cuadros esperando la colocación antes de aceptar igual (no colgar el gesto). */
export const DETACH_HANDOFF_PLACE_FRAMES = 90;

export function createDetachHandoffGate(): DetachHandoffGate {
  return { waited: 0 };
}

function frameMatchesRest(frame: DetachFrame, rest: BirthRect): boolean {
  return (
    Math.abs(frame.x - rest.x) < 1 &&
    Math.abs(frame.y - rest.y) < 1 &&
    Math.abs(frame.w - rest.w) < 1 &&
    Math.abs(frame.h - rest.h) < 1
  );
}

/**
 * `true` = el traspaso puede sembrar el arrastre con este marco.
 * `false` = la pill reintenta en el cuadro siguiente.
 */
export function nextDetachHandoff(
  gate: DetachHandoffGate,
  anchor: DetachFrame | null,
  rest: BirthRect | null,
): boolean {
  // Sin marco el traspaso sale al vacío: la pill reintenta (caso de siempre).
  if (!anchor) {
    gate.waited = 0;
    return false;
  }
  if (rest && !frameMatchesRest(anchor, rest)) {
    gate.waited += 1;
    // El evento de Rust a veces tarda: pasado un rato se acepta igual para
    // no dejar el gesto colgado (queda el comportamiento de antes).
    if (gate.waited <= DETACH_HANDOFF_PLACE_FRAMES) return false;
  }
  gate.waited = 0;
  return true;
}
