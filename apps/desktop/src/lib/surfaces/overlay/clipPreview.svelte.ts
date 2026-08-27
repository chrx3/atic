/**
 * Preview al pasar el mouse por un ítem del portapapeles.
 *
 * # Por qué no es un `tip`
 *
 * Un tooltip muestra una frase. Acá hace falta el CONTENIDO: el texto completo
 * de un ítem que la fila recorta a una línea, o la imagen en un tamaño donde se
 * distinga algo —la miniatura de la fila mide 28 px—. Formas y tamaños que no
 * caben en un globo de texto, y que no valen la pena metiéndoselos a un
 * primitivo que usan cincuenta anclas.
 *
 * Lo que sí se copia de `tip.svelte.ts` es la mecánica de hover, porque ya está
 * resuelta: demora en frío, ventana "tibia" para recorrer la lista sin esperar
 * cada vez, un solo dueño, y oyentes globales que lo bajan cuando el suelo se
 * mueve.
 *
 * # Dónde se dibuja
 *
 * En `ClipPreviewHost.svelte`, montado una vez por ventana en
 * `routes/+layout.svelte` —igual que `TipHost`— y no dentro de la lista. El
 * float del portapapeles es un `div` con `overflow: hidden`: un panel hijo
 * quedaría recortado contra sus 312 px, que es justo lo que este preview viene
 * a evitar.
 */

/** Caja del ancla en px CSS del viewport, copiada (el DOMRect vive poco). */
export type ClipPreviewAnchor = { x: number; y: number; w: number; h: number };

export type ClipPreviewInput =
  | { kind: "text"; text: string; hint?: string }
  | { kind: "image"; src: string; label?: string; hint?: string }
  | null
  | undefined;

class ClipPreviewState {
  kind = $state<"text" | "image">("text");
  text = $state("");
  src = $state("");
  label = $state("");
  hint = $state("");
  anchor = $state<ClipPreviewAnchor | null>(null);
  open = $state(false);

  show(input: NonNullable<ClipPreviewInput>, anchor: ClipPreviewAnchor) {
    this.kind = input.kind;
    this.text = input.kind === "text" ? input.text : "";
    this.src = input.kind === "image" ? input.src : "";
    this.label = input.kind === "image" ? (input.label ?? "") : "";
    this.hint = input.hint ?? "";
    this.anchor = anchor;
    this.open = true;
  }

  hide() {
    this.open = false;
    this.anchor = null;
  }
}

export const clipPreviewState = new ClipPreviewState();

/**
 * Más lento que el tooltip (450 ms).
 *
 * Un panel con una imagen es mucho más intrusivo que un globo de texto, y
 * recorrer la lista buscando un ítem no debería ir dejando previews por el
 * camino. Quien se detiene sobre una fila lo quiere; quien la cruza, no.
 */
const SHOW_DELAY_MS = 600;
/** Ya abierto, moverse a la fila de al lado lo cambia al toque. */
const WARM_MS = 400;

let warmUntil = 0;
let timer = 0;
/** Quién pidió el preview visible. Solo ese puede bajarlo. */
let owner: HTMLElement | null = null;
let globalsInstalled = false;

function clearTimer() {
  if (!timer) return;
  clearTimeout(timer);
  timer = 0;
}

function hideNow() {
  clearTimer();
  if (clipPreviewState.open) warmUntil = Date.now() + WARM_MS;
  owner = null;
  clipPreviewState.hide();
}

/**
 * Un solo juego de oyentes para toda la ventana.
 *
 * En captura, y `scroll` incluido, por el mismo motivo que en `tip`: la lista
 * está virtualizada, así que al hacer scroll la fila anclada puede dejar de
 * existir y el panel quedaría flotando sobre el hueco de otra cosa.
 */
function installGlobals() {
  if (globalsInstalled || typeof window === "undefined") return;
  globalsInstalled = true;
  window.addEventListener("pointerdown", hideNow, true);
  window.addEventListener("wheel", hideNow, true);
  window.addEventListener("scroll", hideNow, true);
  window.addEventListener("blur", hideNow);
}

export function clipPreview(node: HTMLElement, input: ClipPreviewInput) {
  installGlobals();
  let current = input;

  const open = () => {
    if (!current) return;
    clearTimer();
    const run = () => {
      timer = 0;
      if (!current) return;
      owner = node;
      const r = node.getBoundingClientRect();
      clipPreviewState.show(current, { x: r.left, y: r.top, w: r.width, h: r.height });
    };
    if (clipPreviewState.open || Date.now() < warmUntil) run();
    else timer = window.setTimeout(run, SHOW_DELAY_MS);
  };

  const close = () => {
    clearTimer();
    if (owner === node) hideNow();
  };

  node.addEventListener("pointerenter", open);
  node.addEventListener("pointerleave", close);

  return {
    update(next: ClipPreviewInput) {
      current = next;
      if (!next) {
        close();
        return;
      }
      if (owner === node && clipPreviewState.open) {
        const r = node.getBoundingClientRect();
        clipPreviewState.show(next, { x: r.left, y: r.top, w: r.width, h: r.height });
      }
    },
    destroy() {
      node.removeEventListener("pointerenter", open);
      node.removeEventListener("pointerleave", close);
      // La fila se va del DOM —scroll de la lista virtualizada, un borrado— y
      // el panel no puede quedar colgado sobre su hueco.
      close();
    },
  };
}
