/**
 * Puente entre los tokens de motion (CSS) y el código que los necesita en JS.
 *
 * Los números viven en `:root` (app.css) y **solo ahí**. Antes había esperas
 * en JS (`RADIAL_MS = 370`, `QUICK_MS = 160`) que duplicaban a mano las
 * duraciones del CSS: cambiar una y olvidar la otra encogía la ventana a mitad
 * del morph. Acá se leen del CSS en runtime, así no pueden separarse.
 */

/** Cache: getComputedStyle en cada transición es caro y el valor no cambia. */
const cache = new Map<string, number>();

/** Invalida el cache. El tema no toca duraciones, pero sí un cambio de escala. */
export function resetMotionCache(): void {
  cache.clear();
}

/**
 * Lee un token de duración de `:root` en milisegundos.
 *
 * Acepta `350ms` y `0.35s`. El fallback cubre SSR y el caso de un token mal
 * escrito: sin él, un NaN dejaría esperas de cero y el morph se cortaría.
 */
export function duration(token: string, fallbackMs: number): number {
  if (typeof window === "undefined") return fallbackMs;
  const hit = cache.get(token);
  if (hit !== undefined) return hit;

  const raw = getComputedStyle(document.documentElement)
    .getPropertyValue(token)
    .trim();

  let ms = fallbackMs;
  if (raw.endsWith("ms")) {
    const parsed = Number.parseFloat(raw);
    if (Number.isFinite(parsed)) ms = parsed;
  } else if (raw.endsWith("s")) {
    const parsed = Number.parseFloat(raw);
    if (Number.isFinite(parsed)) ms = parsed * 1000;
  }

  cache.set(token, ms);
  return ms;
}

/** Preferencia del SO. Se consulta en cada uso: puede cambiar en caliente. */
export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Duración efectiva: 0 si el usuario pidió menos movimiento. */
export function motionMs(token: string, fallbackMs: number): number {
  return prefersReducedMotion() ? 0 : duration(token, fallbackMs);
}

/** Espera pasiva. Solo para cuando no hay un elemento que observar. */
export function wait(ms: number): Promise<void> {
  if (ms <= 0) return Promise.resolve();
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Espera a que termine la transición de `property` sobre `el`.
 *
 * Preferir esto a `wait()`: es el hecho real, no una estimación. El timeout es
 * solo una red de seguridad — si el elemento se oculta o la propiedad no llega
 * a cambiar, `transitionend` nunca dispara y la promesa colgaría para siempre,
 * dejando la ventana a medio reencuadrar.
 */
export function afterTransition(
  el: HTMLElement | null,
  property: string,
  fallbackMs: number,
): Promise<void> {
  if (!el || prefersReducedMotion()) return Promise.resolve();

  return new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      el.removeEventListener("transitionend", onEnd);
      clearTimeout(timer);
      resolve();
    };
    const onEnd = (event: TransitionEvent) => {
      // Las transiciones de los hijos burbujean: ignorar las ajenas.
      if (event.target !== el || event.propertyName !== property) return;
      finish();
    };
    el.addEventListener("transitionend", onEnd);
    const timer = setTimeout(finish, fallbackMs + 60);
  });
}

/** Nombres de token, para no repetir strings sueltos por el código. */
export const MOTION = {
  morphOpen: "--morph-open-dur",
  morphClose: "--morph-close-dur",
  morphFade: "--morph-fade-dur",
  morphQuick: "--morph-quick-dur",
  panel: "--panel-dur",
  micro: "--duration-micro",
  quick: "--duration-quick",
  fast: "--duration-fast",
  slow: "--duration-slow",
  medium: "--duration-medium",
  flight: "--flight-dur",
  floatOpen: "--float-open-dur",
  floatClose: "--float-close-dur",
  launcherBar: "--launcher-bar-open-dur",
  launcherSeparate: "--launcher-separate-dur",
  launcherFavStagger: "--launcher-fav-stagger",
} as const;

/** Fallbacks alineados con app.css. Solo se usan si el token no resuelve. */
export const MOTION_FALLBACK = {
  [MOTION.morphOpen]: 110,
  [MOTION.morphClose]: 100,
  [MOTION.morphFade]: 80,
  [MOTION.morphQuick]: 60,
  [MOTION.panel]: 110,
  [MOTION.micro]: 40,
  [MOTION.quick]: 75,
  [MOTION.fast]: 125,
  [MOTION.slow]: 200,
  [MOTION.medium]: 150,
  [MOTION.flight]: 110,
  [MOTION.floatOpen]: 110,
  [MOTION.floatClose]: 100,
  [MOTION.launcherBar]: 100,
  [MOTION.launcherSeparate]: 90,
  [MOTION.launcherFavStagger]: 90,
} as const;

/** Atajo: duración efectiva de un token conocido. */
export function ms(token: keyof typeof MOTION_FALLBACK): number {
  return motionMs(token, MOTION_FALLBACK[token]);
}

/** ease-in-out: swaps de pestaña / texto (transitions-polish). */
function easeInOut(t: number): number {
  return t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
}

/**
 * Contenido al cambiar de pestaña.
 *
 * Simétrico (`--duration-fast`): no es open/close. Solo opacity + micro
 * translate (sin blur: carísimo en paneles grandes).
 */
export function tabPanel(
  _node: Element,
  { duration }: { duration?: number } = {},
): { duration: number; easing: (t: number) => number; css: (t: number) => string } {
  if (prefersReducedMotion()) {
    return { duration: 0, easing: (t) => t, css: () => "" };
  }
  const dur = duration ?? motionMs(MOTION.fast, MOTION_FALLBACK[MOTION.fast]);
  return {
    duration: dur,
    easing: easeInOut,
    css: (t) => {
      const u = 1 - t;
      // --distance-micro (4px)
      return `opacity:${t};transform:translateY(${u * 4}px)`;
    },
  };
}
