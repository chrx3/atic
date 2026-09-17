/**
 * Geometría de la rueda, sin DOM.
 *
 * Portada tal cual de
 * `apps/desktop/src/lib/surfaces/overlay/pill/wheelGeometry.ts`: el ángulo
 * arranca arriba y va en sentido horario; el gajo se recorta con un polígono
 * en píxeles (no en porcentajes, que se deformaba con la relación de aspecto);
 * los separadores llegan hasta el borde según su propio ángulo.
 */

export interface Point {
  x: number;
  y: number;
}

/** El ángulo del nodo `index`, en radianes. Cero arriba, horario. */
export function nodeAngle(index: number, count: number): number {
  return (index / count) * Math.PI * 2 - Math.PI / 2;
}

/** Dónde se dibuja el icono del nodo `index`. */
export function nodePosition(
  index: number,
  count: number,
  size: { width: number; height: number },
  ringRadius: number,
): Point {
  const angle = nodeAngle(index, count);
  return {
    x: size.width / 2 + Math.cos(angle) * ringRadius,
    y: size.height / 2 + Math.sin(angle) * ringRadius,
  };
}

/**
 * Centro de la gota viva que cuelga bajo el anillo.
 */
export function liveDropPosition(
  size: { width: number; height: number },
  ringRadius: number,
  nodeDiameter: number,
  dropDiameter: number,
  gap: number,
): Point {
  return {
    x: size.width / 2,
    y: size.height / 2 + ringRadius + nodeDiameter / 2 + gap + dropDiameter / 2,
  };
}

/** El sector que activa un nodo, como polígono en píxeles. */
export function wedgeClip(
  index: number,
  count: number,
  size: { width: number; height: number },
): string {
  const half = Math.PI / count;
  const center = nodeAngle(index, count);
  const cx = size.width / 2;
  const cy = size.height / 2;
  const radius = Math.hypot(size.width, size.height);
  const points = [-1, -0.5, 0, 0.5, 1].map((t) => {
    const a = center + t * half;
    const x = cx + radius * Math.cos(a);
    const y = cy + radius * Math.sin(a);
    return `${x.toFixed(1)}px ${y.toFixed(1)}px`;
  });
  return `polygon(${cx.toFixed(1)}px ${cy.toFixed(1)}px, ${points.join(", ")})`;
}

/** Las fronteras entre gajos, del núcleo hacia afuera. */
export function separators(
  count: number,
  size: { width: number; height: number },
  innerRadius: number,
): { deg: number; len: number }[] {
  return Array.from({ length: count }, (_, index) => {
    const deg = -90 + (360 / count) * index + 180 / count;
    const rad = (deg * Math.PI) / 180;
    const cos = Math.abs(Math.cos(rad));
    const sin = Math.abs(Math.sin(rad));
    const toEdge = Math.min(
      cos < 1e-6 ? Infinity : size.width / 2 / cos,
      sin < 1e-6 ? Infinity : size.height / 2 / sin,
    );
    return { deg, len: Math.max(0, toEdge - innerRadius) };
  });
}
