/**
 * Qué índice queda al recorrer una colección con el teclado.
 *
 * Las bibliotecas de la ventana principal —grabaciones, capturas, historial,
 * textos— se recorren igual y solo se diferencian en cuántas columnas tienen,
 * así que la regla vive acá y no repetida en cada `onkeydown`: una lista es
 * una grilla de una sola columna.
 *
 * Devuelve `null` cuando la tecla no navega, y ese `null` es lo que el
 * componente usa para NO llamar a `preventDefault()`. Tragarse teclas que no
 * se manejan rompe el tabulador, el escape y el tipeo dentro de un buscador.
 */

/** Teclas horizontales: solo navegan si hay más de una columna. */
const HORIZONTAL = new Set(["ArrowRight", "ArrowLeft"]);

export function nextIndex(
  key: string,
  current: number,
  length: number,
  columns = 1,
): number | null {
  if (length <= 0) return null;
  if (columns <= 1 && HORIZONTAL.has(key)) return null;

  // Sin nada elegido, la primera tecla elige un extremo en vez de moverse
  // desde un cero implícito: bajar tiene que dar el primero, no el segundo.
  if (current < 0 || current >= length) {
    switch (key) {
      case "ArrowDown":
      case "ArrowRight":
      case "Home":
        return 0;
      case "ArrowUp":
      case "ArrowLeft":
      case "End":
        return length - 1;
      default:
        return null;
    }
  }

  const clamp = (value: number) => Math.max(0, Math.min(length - 1, value));
  switch (key) {
    case "ArrowDown":
      return clamp(current + columns);
    case "ArrowUp":
      return clamp(current - columns);
    case "ArrowRight":
      return clamp(current + 1);
    case "ArrowLeft":
      return clamp(current - 1);
    case "Home":
      return 0;
    case "End":
      return length - 1;
    default:
      return null;
  }
}
