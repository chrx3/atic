/**
 * Fusión líquida — goo filter, alcance y curvas.
 *
 * Portado de `docs/demos/liquid.js` (la demo standalone de la app), que a su
 * vez generaliza el truco de metaballs que la app usa con SVG:
 *
 *   1. `feGaussianBlur` difumina el grupo. Cada forma queda con un halo.
 *   2. `feColorMatrix` endurece el alfa (`a' = 18a − 7`). Donde dos halos se
 *      suman y pasan el umbral, aparece el cuello.
 *   3. `feComposite atop` devuelve el gráfico nítido encima.
 *
 * Regla de oro: lo que se funde va en una capa aparte (`.skin`) sin contenido,
 * y el texto/iconos viven en otra (`.ink`). El filtro difumina todo lo que
 * tenga adentro; un icono dentro de la piel termina siendo una mancha.
 */

const SVG_NS = "http://www.w3.org/2000/svg";
let seq = 0;
let defsSvg: SVGDefsElement | null = null;

function defs(): SVGDefsElement {
  if (defsSvg) return defsSvg;
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.style.cssText =
    "position:absolute;width:0;height:0;overflow:hidden;pointer-events:none";
  const node = document.createElementNS(SVG_NS, "defs");
  svg.appendChild(node);
  document.body.appendChild(svg);
  defsSvg = node;
  return node;
}

function el<K extends keyof SVGElementTagNameMap>(
  name: K,
  attrs: Record<string, string | number>,
): SVGElementTagNameMap[K] {
  const node = document.createElementNS(SVG_NS, name);
  for (const key of Object.keys(attrs)) {
    node.setAttribute(key, String(attrs[key]));
  }
  return node;
}

const GAIN = 18;
const BIAS = -7;

/** Viscosidad por defecto: la de la app (`GOO_SIGMA` en `GooFilter.svelte`). */
export const GOO_SIGMA = 6;

export type Goo = {
  set(sigma: number): void;
  readonly sigma: number;
  /** Distancia a la que este filtro todavía cruza el hueco, en px. */
  readonly reach: number;
  destroy(): void;
};

export function goo(
  container: HTMLElement,
  options: { sigma?: number; shadow?: string; id?: string } = {},
): Goo {
  const gain = GAIN;
  const bias = BIAS;
  const id = options.id ?? `goo-${++seq}`;
  let sigma = options.sigma ?? GOO_SIGMA;

  const filter = el("filter", {
    id,
    x: "-50%",
    y: "-50%",
    width: "200%",
    height: "200%",
    "color-interpolation-filters": "sRGB",
  });
  const blur = el("feGaussianBlur", {
    in: "SourceGraphic",
    stdDeviation: sigma,
    result: "blur",
  });
  const matrix = el("feColorMatrix", {
    in: "blur",
    type: "matrix",
    values: `1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 ${gain} ${bias}`,
    result: "goo",
  });
  const atop = el("feComposite", {
    in: "SourceGraphic",
    in2: "goo",
    operator: "atop",
  });
  filter.appendChild(blur);
  filter.appendChild(matrix);
  filter.appendChild(atop);
  defs().appendChild(filter);

  function apply() {
    // Sigma 0 no es "difuminar cero": aun así el endurecido come medio píxel.
    let chain = sigma > 0 ? `url(#${id})` : "";
    // La sombra va DESPUÉS del goo: así cae sobre la silueta ya fundida.
    if (options.shadow) chain = chain ? `${chain} ${options.shadow}` : options.shadow;
    container.style.filter = chain || "none";
  }
  apply();

  return {
    set(next: number) {
      sigma = next;
      blur.setAttribute("stdDeviation", String(sigma));
      apply();
    },
    get sigma() {
      return sigma;
    },
    get reach() {
      return reach(sigma, gain, bias);
    },
    destroy() {
      filter.parentNode?.removeChild(filter);
      container.style.filter = "";
    },
  };
}

/** erf, Abramowitz & Stegun 7.1.26. */
function erf(z: number): number {
  const sign = z < 0 ? -1 : 1;
  z = Math.abs(z);
  const t = 1 / (1 + 0.3275911 * z);
  const y =
    1 -
    ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) *
      t +
      0.254829592) *
      t *
      Math.exp(-z * z);
  return sign * y;
}

function phi(x: number): number {
  return 0.5 * (1 + erf(x / Math.SQRT2));
}

/**
 * Alcance de la fusión: el hueco máximo que el cuello todavía cruza.
 * Con los valores de la app da 1.72·σ: con σ = 6, unos 10.3 px.
 */
export function reach(sigma: number, gain = GAIN, bias = BIAS): number {
  if (!(sigma > 0)) return 0;
  const threshold = -bias / gain;
  if (!(threshold > 0 && threshold < 2)) return 0;
  let lo = 0;
  let hi = 12 * sigma;
  for (let i = 0; i < 60; i++) {
    const mid = (lo + hi) / 2;
    if (2 * phi(-mid / (2 * sigma)) > threshold) lo = mid;
    else hi = mid;
  }
  return (lo + hi) / 2;
}

/** `cubic-bezier(x1,y1,x2,y2)` evaluada a mano (Newton sobre x). */
export function ease(x1: number, y1: number, x2: number, y2: number) {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const sampleX = (t: number) => ((ax * t + bx) * t + cx) * t;
  const slopeX = (t: number) => (3 * ax * t + 2 * bx) * t + cx;
  return (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    let t = x;
    for (let i = 0; i < 8; i++) {
      const err = sampleX(t) - x;
      if (Math.abs(err) < 1e-6) break;
      const d = slopeX(t);
      if (Math.abs(d) < 1e-6) break;
      t -= err / d;
    }
    return ((ay * t + by) * t + cy) * t;
  };
}

/**
 * Las curvas de `app.css`, con su nombre.
 * Abrir es el momento expresivo; cerrar es utilitario y se quita del camino.
 */
export const EASE = {
  morph: ease(0.22, 1, 0.36, 1),
  close: ease(0.22, 1, 0.36, 1),
  linear: (x: number) => x,
};

export function prefersReducedMotion(): boolean {
  return (
    typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/**
 * Anima `t` y llama a `onFrame` en cada cuadro. Devuelve la cancelación: si
 * llega otra animación a mitad de camino, la anterior suelta el control.
 */
export function animate(opts: {
  from?: number;
  to?: number;
  duration: number;
  ease?: (x: number) => number;
  onFrame: (t: number) => void;
  onDone?: () => void;
}): () => void {
  const from = opts.from ?? 0;
  const to = opts.to ?? 1;
  const curve = opts.ease ?? EASE.linear;
  const duration = prefersReducedMotion() ? 0 : opts.duration;
  let start = 0;
  let raf = 0;
  let live = true;

  function step(now: number) {
    if (!live) return;
    if (!start) start = now;
    const x = duration > 0 ? Math.min(1, (now - start) / duration) : 1;
    opts.onFrame(from + (to - from) * curve(x));
    if (x < 1) raf = requestAnimationFrame(step);
    else opts.onDone?.();
  }

  raf = requestAnimationFrame(step);
  return () => {
    live = false;
    cancelAnimationFrame(raf);
  };
}

export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
