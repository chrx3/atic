/**
 * Las decisiones de la pill, sin estado ni DOM.
 *
 * Todo lo de acá vivía dentro del componente, mezclado con los efectos y las
 * llamadas a Rust, así que no había forma de probarlo sin montar el overlay
 * entero — y es justo la parte donde un error no se ve como un error sino como
 * «la pill quedó unos píxeles corrida».
 *
 * Son funciones puras: entra estado, sale una decisión. El componente sigue
 * siendo quien la ejecuta.
 */

import { PILL, windowFor, type Pivot, type Size } from "../pillStage";
import { dockAxis, type DockEdge } from "../edgeDock";
import { WHEEL_TOOLS } from "$core/tools";

/**
 * Largo de la tira de herramientas para `n` botones.
 *
 * `n` es parámetro y no `WHEEL_TOOLS.length` directo para poder fijar la
 * cuenta en un test sin atarlo a cuántas herramientas haya hoy.
 */
export function islandStripLong(n: number): number {
  if (n <= 0) return PILL.bar;
  return n * PILL.islandTool + (n - 1) * PILL.islandGap;
}

/** Qué hay desplegado. Clipboard/snippets ya no crecen la pill: son floats. */
export type Surface = "none" | "wheel" | "edge";

/**
 * Acoplada a un borde, y si el puntero la tiene abierta.
 *
 * Va aparte de `Surface` en vez de multiplicar los estados (`edge-left`,
 * `edge-left-open`, …): el borde no cambia lo que la pill *es*, solo contra
 * qué lado se aplana y hacia dónde crece.
 */
export type Dock = { edge: DockEdge; expanded: boolean };

/**
 * El contenido que la pill tiene que poder mostrar, en píxeles.
 *
 * `barW` se mide del DOM en vez de mantenerse en una tabla de anchos por
 * estado: esa tabla fue el origen del desajuste original, porque el ancho real
 * depende de la fuente, del texto del timer y de si entró un chip.
 */
export function contentFor(
  surface: Surface,
  barW: number,
  dock: Dock | null = null,
): Size {
  if (surface === "wheel") {
    const side = PILL.wheel - PILL.pad * 2;
    return { w: side, h: side };
  }
  if (surface === "edge" && dock) {
    // Abierta es la tira de herramientas: acoplada, la pill deja de ser un
    // indicador y pasa a ser el acceso. Se despliega A LO LARGO del borde, que
    // es el único eje donde hay lugar sin taparle la pantalla al usuario.
    if (dock.expanded) {
      const long = islandStripLong(WHEEL_TOOLS.length);
      return dockAxis(dock.edge) === "x"
        ? { w: PILL.islandTool, h: long }
        : { w: long, h: PILL.islandTool };
    }
    // En reposo, una pestaña: fina contra el borde y larga a lo largo de él.
    return dockAxis(dock.edge) === "x"
      ? { w: PILL.islandThick, h: PILL.islandLong }
      : { w: PILL.islandLong, h: PILL.islandThick };
  }
  return { w: Math.max(barW, PILL.bar), h: PILL.bar };
}

/** El tamaño de la caja para un estado dado. */
export function targetFor(
  surface: Surface,
  barW: number,
  dock: Dock | null = null,
): Size {
  return windowFor(contentFor(surface, barW, dock));
}

/**
 * Traer la pill al cursor la saca del canto.
 *
 * Acoplada, `contentFor` mide la pestaña (~10 px). Si el summon vuela sin
 * desacoplar, llega al puntero con forma de isla y se queda así: el vuelo
 * solo mueve, no cambia `surface`.
 */
export function undockForSummon(state: {
  surface: Surface;
  dock: Dock | null;
}): { surface: Surface; dock: Dock | null } {
  if (state.surface === "edge") {
    return { surface: "none", dock: null };
  }
  return { surface: state.surface, dock: state.dock };
}

/**
 * ¿El chrome de la rueda es la silueta activa?
 *
 * Abierta (`surface === "wheel"`) o colapsando (`collapsingFrom === "wheel"`):
 * en ambos casos el único "a" visible es el de ParticleWheel. El stack de la
 * barra vive anclado al top-left del root (no al centro): si publica formas o
 * pinta su AticMark mientras el root es el cuadrado grande, aparece una
 * segunda «a» fantasma arriba-izquierda — también con la rueda ya abierta, no
 * solo al cerrar. Por eso el stack se apaga (visibility + sin marca + sin
 * publish) en todo el tramo `wheelChromeActive`.
 */
export function wheelChromeActive(state: {
  surface: Surface;
  collapsingFrom: "wheel" | null;
}): boolean {
  return state.surface === "wheel" || state.collapsingFrom === "wheel";
}

/**
 * ¿Puede el stack montar su AticMark?
 *
 * Es el inverso de `wheelChromeActive`: con la rueda al mando la marca del
 * stack no debe existir en el DOM. Sirve de contrato explícito para tests.
 */
export function stackMarkVisible(state: {
  surface: Surface;
  collapsingFrom: "wheel" | null;
}): boolean {
  return !wheelChromeActive(state);
}

/**
 * Qué punto se conserva en el próximo reencuadre.
 *
 * Abrir y cerrar la rueda pivotean al `center` (morph in-situ desde el hogar).
 * Al cerrar hay que saber **de qué** se está cerrando, no adónde se va: para
 * cuando esto corre, `surface` ya vale `"none"`. Sin `collapsingFrom` el
 * colapso caería en `topLeft` y la barra se correría.
 *
 * En reposo el pivote es `topLeft`, nunca `center`: el ancho de la barra
 * cambia solo —entra el timer, tictaquea de 0:09 a 0:10, aparece el badge de la
 * cola— y con pivote al centro CADA cambio corría la pill media diferencia. Al
 * arrancar, el primer encogimiento la movía 53 px.
 *
 * El vuelo al cursor (abrir rueda / summon) no pasa por acá: usa `flyTo` en
 * la superficie; este pivote solo decide el morph de tamaño in-situ.
 */
export function pivotFor(state: {
  surface: Surface;
  collapsingFrom: "wheel" | null;
  dock?: Dock | null;
}): Pivot {
  if (state.surface === "wheel") return "center";
  if (state.collapsingFrom === "wheel") return "center";
  // Acoplada: el lado pegado al canto es el punto fijo. Con `topLeft`, abrir
  // la isla de la derecha la empujaría fuera de la pantalla.
  if (state.surface === "edge" && state.dock) {
    switch (state.dock.edge) {
      case "left":
        return "dockLeft";
      case "right":
        return "dockRight";
      case "top":
        return "dockTop";
      case "bottom":
        return "dockBottom";
    }
  }
  return "topLeft";
}

/**
 * ¿Este reencuadre se anima en el sitio?
 *
 * Solo los cambios de la barra compacta: disco ↔ dictado ↔ grabación ↔ cola,
 * donde el salto se leía como un parpadeo. El colapso de la rueda tiene su
 * propia coreografía —caja + gotas con `.is-sizing`— y animar acá largaría un
 * tween que el vuelo siguiente cancelaría a mitad de camino.
 *
 * El primer reencuadre tampoco: al arrancar no hay «estado anterior» desde el
 * cual transicionar, solo la ventana acomodándose.
 */
export function morphsInPlace(state: {
  from: Size | null;
  surface: Surface;
  collapsingFrom: "wheel" | null;
}): boolean {
  // También la isla: abrirse y cerrarse contra el canto ES un cambio de la
  // barra compacta, y sin animar el salto de pestaña a barra se lee como un
  // parpadeo. La rueda sigue afuera: tiene su propia coreografía.
  return (
    state.from !== null &&
    state.collapsingFrom === null &&
    (state.surface === "none" || state.surface === "edge")
  );
}

/**
 * Si el cursor ya está casi sobre la pill, el vuelo de apertura no aporta
 * significado —solo latencia. Umbral ≈ diámetro del disco.
 */
export const FLIGHT_SKIP_PX = 48;

/**
 * La pill está en reposo: la barra es SOLO el disco.
 *
 * Lo mira el CSS para la forma y lo mira la piel para saber si monta la gota,
 * así que tiene que ser una sola definición.
 */
export function isDiscOnly(state: {
  surface: Surface;
  activity: "idle" | "recording" | "dictating";
  hasQueue: boolean;
  agentAlert: boolean;
}): boolean {
  return (
    state.activity === "idle" &&
    !state.hasQueue &&
    !state.agentAlert
  );
}

/**
 * ¿Publicar el disco junto a la gota en el campo líquido?
 *
 * Solo mientras la gota todavía no lo cubre. Cuando ambas comparten el borde
 * izquierdo (disco de 40 px + pastilla ya expandida), el `smin` engorda ese
 * lado y la silueta queda con aire muerto a la izquierda del contenido.
 */
export function discJoinsTail(
  bar: { w: number } | null | undefined,
  tail: { w: number } | null | undefined,
): boolean {
  if (!bar || !tail) return false;
  return tail.w < bar.w * 0.95;
}

/** Área útil de un monitor, en el mismo espacio que la pill. */
export type WorkArea = { x: number; y: number; w: number; h: number };

/**
 * Encaja un rectángulo en el monitor cuyo centro lo contiene.
 * Sin áreas, deja `p` igual.
 */
export function clampRect(
  areas: readonly WorkArea[],
  p: { x: number; y: number },
  size: Size,
): { x: number; y: number } {
  if (areas.length === 0) return p;
  const cx = p.x + size.w / 2;
  const cy = p.y + size.h / 2;
  const hit = areas.find(
    (a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h,
  );
  const area = hit ?? areas[0];
  if (!area) return p;
  const maxX = Math.max(area.x + area.w - size.w, area.x);
  const maxY = Math.max(area.y + area.h - size.h, area.y);
  return {
    x: Math.min(Math.max(p.x, area.x), maxX),
    y: Math.min(Math.max(p.y, area.y), maxY),
  };
}

/**
 * A dónde volar la pastilla compacta ANTES de revelar la rueda.
 *
 * El cursor manda si está lejos (atajo radial). Si el clic es sobre la pill,
 * el destino es el centro actual, clampeado para que el cuadrado de la rueda
 * quepa entero — si no, el morph in-situ en un rincón recorta las gotas.
 *
 * Devuelve el top-left de la caja CHICA cuyo centro coincide con el de la
 * rueda ya clampeada, para que el resize con pivot `center` no la desplace.
 */
export function wheelOpenFlight(opts: {
  cursor: { x: number; y: number } | null;
  pill: { x: number; y: number; w: number; h: number };
  wheel: Size;
  areas: readonly WorkArea[];
  skipIfNear: number;
}): { x: number; y: number } {
  const pillCx = opts.pill.x + opts.pill.w / 2;
  const pillCy = opts.pill.y + opts.pill.h / 2;
  let cx = pillCx;
  let cy = pillCy;
  if (opts.cursor) {
    const dist = Math.hypot(opts.cursor.x - pillCx, opts.cursor.y - pillCy);
    if (dist >= opts.skipIfNear) {
      cx = opts.cursor.x;
      cy = opts.cursor.y;
    }
  }
  const desired = { x: cx - opts.wheel.w / 2, y: cy - opts.wheel.h / 2 };
  const clamped = clampRect(opts.areas, desired, opts.wheel);
  return {
    x: clamped.x + opts.wheel.w / 2 - opts.pill.w / 2,
    y: clamped.y + opts.wheel.h / 2 - opts.pill.h / 2,
  };
}

/**
 * En qué lado de la pastilla va el control de consola.
 *
 * Regla: al lado **opuesto** al borde horizontal más cercano del monitor.
 * Cerca del borde izquierdo → consola a la derecha (no se pega al canto);
 * cerca del derecho → consola a la izquierda.
 */
export function consoleSideFor(
  areas: readonly WorkArea[],
  pill: { x: number; y: number },
  size: { w: number; h: number },
): "left" | "right" {
  if (areas.length === 0) return "right";
  const cx = pill.x + size.w / 2;
  const cy = pill.y + size.h / 2;
  const area =
    areas.find(
      (a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h,
    ) ?? areas[0];
  if (!area) return "right";
  const distLeft = cx - area.x;
  const distRight = area.x + area.w - cx;
  return distLeft <= distRight ? "right" : "left";
}

/**
 * La siguiente herramienta de la rueda.
 *
 * `null` como punto de partida no es un descuido: la rueda abre sin selección a
 * propósito, para que un toque accidental del atajo no dispare nada al soltar.
 * La primera flecha entra por el extremo que corresponde al sentido.
 */
export function stepWheel<T extends string>(
  current: T | null,
  direction: 1 | -1,
  tools: readonly { id: T }[],
): T {
  const index = tools.findIndex((tool) => tool.id === current);
  if (index < 0) return direction === 1 ? tools[0].id : tools[tools.length - 1].id;
  return tools[(index + direction + tools.length) % tools.length].id;
}

/** Qué hace una tecla con la rueda abierta. `null` = no es de la rueda. */
export function wheelKeyAction(
  key: string,
  shiftKey: boolean,
): "next" | "prev" | "activate" | null {
  if (key === "ArrowRight" || key === "ArrowDown") return "next";
  if (key === "ArrowLeft" || key === "ArrowUp") return "prev";
  if (key === "Tab") return shiftKey ? "prev" : "next";
  if (key === "Enter" || key === " ") return "activate";
  return null;
}

/**
 * Teclas que hay que tragarse para que no las agarre el WebView.
 *
 * Son el chrome del navegador —imprimir, buscar, DevTools, recargar, zoom— que
 * dentro de una pill flotante no significan nada y que además pueden dejarla
 * inutilizable: `Ctrl+R` recarga el overlay entero y se lleva puesta la sesión
 * de agentes.
 */
export function blocksBrowserChrome(event: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
}): boolean {
  const mod = event.ctrlKey || event.metaKey;
  const key = event.key.toLowerCase();
  if (mod && ["p", "f", "g", "u", "j", "i", "r", "=", "+", "-", "0"].includes(key)) {
    return true;
  }
  return event.key === "F3" || event.key === "F5" || event.key === "F12";
}
