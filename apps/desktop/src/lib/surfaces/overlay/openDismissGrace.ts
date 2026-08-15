/**
 * El clic que abre un float (rueda, atajo con el mouse sobre la UI) llega
 * tarde como `overlay-dismiss`: Raw Input procesa el button-down cuando el
 * overlay ya se desarmó al colapsar la rueda, y el panel se cierra al nacer.
 */

let until = 0;

const DEFAULT_MS = 450;

export function armOpenDismissGrace(ms = DEFAULT_MS): void {
  until = performance.now() + ms;
}

export function isOpenDismissGrace(): boolean {
  return performance.now() < until;
}
