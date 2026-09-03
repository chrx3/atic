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
let resting = false;
let restingResolvers: Array<() => void> = [];

export function captureToolBirth(rect: BirthRect | null): void {
  birth = rect ? { ...rect } : null;
  resting = false;
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
