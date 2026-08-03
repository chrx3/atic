/**
 * La geometría de la rueda, sin DOM.
 *
 * Son cuatro cuentas de trigonometría que deciden dónde cae cada herramienta,
 * qué zona la activa y hasta dónde llega cada separador. Estaban dentro del
 * componente, así que la única forma de comprobarlas era abrir la rueda y
 * mirar — y un error acá no se ve como un error, se ve como «el gajo de arriba
 * responde un poco corrido».
 */

/** Punto en píxeles del lienzo de la rueda. */
export interface Point {
  x: number;
  y: number;
}

/**
 * El ángulo del nodo `index`, en radianes.
 *
 * Cero apunta ARRIBA y no a la derecha: la primera herramienta tiene que caer
 * en las doce, que es donde el ojo empieza a leer una rueda. De ahí el
 * `- π/2`, porque el cero del canvas y del CSS apunta a la derecha.
 */
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
 * El sector que activa un nodo, como polígono en PÍXELES.
 *
 * En porcentajes el sector se deformaba con la relación de aspecto y quedaban
 * franjas muertas a los lados. En píxeles cubre el rectángulo entero, esquinas
 * incluidas.
 *
 * El radio es la diagonal completa —más que la media diagonal— a propósito: el
 * polígono desborda el rectángulo, y por eso dentro de él las fronteras son los
 * rayos exactos del sector y no una aproximación recortada.
 *
 * `clip-path` recorta también el hit-testing, así que el sector queda
 * clickeable sin dibujar nada.
 */
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

/**
 * Las fronteras entre gajos.
 *
 * Cada una arranca fuera del núcleo y llega hasta el borde del lienzo **según
 * su propio ángulo**, no hasta un radio fijo: la distancia de un rayo al
 * rectángulo depende de hacia dónde apunta. Con un largo único, las diagonales
 * quedaban cortas y las cardinales se pasaban.
 */
export function separators(
  count: number,
  size: { width: number; height: number },
  innerRadius: number,
): { deg: number; len: number }[] {
  return Array.from({ length: count }, (_, index) => {
    // Van en el MEDIO entre dos nodos: media vuelta de sector más allá.
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
