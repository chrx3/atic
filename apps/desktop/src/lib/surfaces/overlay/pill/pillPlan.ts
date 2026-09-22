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

/**
 * Largo de la pestaña con `n` avisos, contando que la marca sigue ahí.
 *
 * Antes el aviso se pintaba ENCIMA de la marca —`.p-island-cues` iba en
 * `inset: 0`— y la pestaña solo tenía que abrigar los logos. Ahora conviven
 * a lo largo del borde, así que el largo es marca + un botón por aviso.
 *
 * Con varios logos de agente el botón es uno solo (los logos se apilan
 * adentro), así que la cuenta sobra unos píxeles. Sobrar no recorta nada;
 * quedarse corto sí.
 */
export function islandCueLong(n: number, msg = false): number {
  const marks = Math.max(1, Math.floor(n) || 1);
  const cues = marks * PILL.islandCueBtn + (marks - 1) * PILL.islandGap;
  const inner =
    PILL.islandMark + PILL.islandGap + cues + (msg ? PILL.islandCueMsgW : 0) + 12;
  return Math.max(PILL.islandLong, inner);
}

/** Alto extra de la barra flotante cuando hay más de un aviso de consola. */
export function agentStackHang(n: number): number {
  const rows = Math.max(0, Math.floor(n) || 0);
  if (rows <= 1) return 0;
  return (rows - 1) * PILL.agentStackRow;
}

/** Qué hay desplegado. Textos y agentes pueden ser cara, como clipboard. */
export type Surface = "none" | "wheel" | "edge";

/**
 * Cara de la isla acoplada. `tab` es la pestaña/tira; `agent` es el permiso
 * pendiente; el resto son paneles de herramienta (un solo blob, cualquier canto).
 */
export type IslandFace =
  | "tab"
  | "agent"
  | "dictation"
  | "live"
  | "clipboard"
  | "snippets"
  | "system"
  | "agents"
  | "customize";

export function isIslandPanelFace(face: IslandFace): boolean {
  return (
    face === "clipboard" ||
    face === "snippets" ||
    face === "system" ||
    face === "agents" ||
    face === "customize"
  );
}

/**
 * Acoplada a un borde, y si el puntero la tiene abierta.
 *
 * Va aparte de `Surface` en vez de multiplicar los estados (`edge-left`,
 * `edge-left-open`, …): el borde no cambia lo que la pill *es*, solo contra
 * qué lado se aplana y hacia dónde crece.
 */
export type Dock = { edge: DockEdge; expanded: boolean };

export type Activity = "idle" | "recording" | "dictating";

/**
 * Hueco extra en la tira acoplada.
 *
 * La grabación ya no roba un slot: cuelga como gota bajo el cuerpo, fundida
 * al líquido. Se deja la función para no reescribir a los tests de geometría
 * de la tira: el extra ahora es `liveHang`, no un botón más.
 */
export function islandLiveSlots(_activity: Activity): number {
  return 0;
}

/**
 * Extra de caja para la gota viva (diámetro + cuello).
 *
 * Cuelga en dos casos: la rueda (grabando o dictando, su propio escenario) y
 * la barra FLOTANTE en dictado, donde la onda baja a una gota debajo de la
 * caja. Grabar no cuelga en la barra: su onda vive adentro.
 *
 * Acoplada no cuelga: colgar una gota hacía que la forma cambiara con el
 * estado, y era lo que hacía que la pestaña cerrada, la tira abierta y la
 * cápsula flotante no se leyeran como la misma cosa (ahí el dictado además
 * desacopla solo).
 */
export function liveHang(activity: Activity, surface: Surface = "none"): number {
  if (activity !== "recording" && activity !== "dictating") return 0;
  if (surface === "wheel") return PILL.recDrop + PILL.recDropGap;
  if (surface === "none" && activity === "dictating") {
    return PILL.recDrop + PILL.recDropGap;
  }
  return 0;
}

/**
 * Tramo que el panel lateral le suma a la caja de la cara de agentes.
 *
 * Es UN número para los tres lugares que lo necesitan —`contentFor`, el
 * recentrado del techo y el anclaje de salida—: consola y panel son una sola
 * superficie sin hueco (`.p-row.is-side` va con `gap: 0`), así que el tramo es
 * el ancho del panel. Si algún día vuelve la separación, vuelve acá y en el CSS.
 */
export function sidePanelSpan(): number {
  return PILL.islandClipW;
}

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
  activity: Activity = "idle",
  /**
   * Cuántas herramientas muestra la tira. Por defecto, todas: el usuario puede
   * esconder gajos desde Ajustes y la caja tiene que encoger con ellos, pero
   * un llamador que no sepa de esa preferencia sigue midiendo lo de siempre.
   */
  toolCount: number = WHEEL_TOOLS.length,
  /**
   * Aviso de agente o dock achicado: la pestaña cerrada engorda un poco para
   * pintar logos. No aplica a la tira abierta (ya mide `islandTool`).
   */
  islandCue: boolean = false,
  /** Cuántas marcas hay que alinear en la pestaña. 0 o 1 no alarga. */
  islandCueCount: number = 0,
  /** Avisos de consola apilados en la barra flotante. */
  agentStack: number = 0,
  /**
   * Cara expandida de la isla. El permiso (`agent`) solo en cantos
   * horizontales; los paneles (clipboard / textos / agentes) en cualquiera.
   */
  face: IslandFace = "tab",
  /** Cara agentes: true = consola (panel alto), false = lanzador compacto. */
  agentsConsole: boolean = false,
  /** Cara agentes con el selector de carpetas abierto: la pill lo envuelve. */
  agentsBrowse: boolean = false,
  /**
   * Panel lateral junto a la consola (clipboard / textos). Solo cambia la
   * medida de la cara `agents`: la caja suma el tramo del panel (ver
   * `sidePanelSpan`), con el borde del lado acoplado fijo (ver `pivotFor`),
   * así la consola no se corre. En el eje y el alto acompaña al de la consola.
   */
  side: boolean = false,
  /**
   * El primer aviso de la pestaña lleva texto (preview / «permiso»): la
   * pestaña reserva el tramo fijo `islandCueMsgW`. Solo notches del eje y.
   */
  islandCueMsg: boolean = false,
  /** Filas de la cara ambiental de agentes. */
  liveRows: number = 0,
): Size {
  if (surface === "wheel") {
    const wheelSide = PILL.wheel - PILL.pad * 2;
    const hang = liveHang(activity, "wheel") > 0 ? PILL.wheelLiveHang : 0;
    return { w: wheelSide, h: wheelSide + hang };
  }
  if (surface === "edge" && dock) {
    // En reposo, una pestaña: fina contra el borde y larga a lo largo de él.
    // Grabando se alarga, no engorda: el estado entró a la cara de la marca.
    const thick = islandCue ? PILL.islandCueThick : PILL.islandThick;
    const long = islandCue
      ? islandCueLong(islandCueCount, islandCueMsg)
      : PILL.islandLong;
    // Cara expandida: la tarjeta cuelga de la pestaña con gap 0, un solo blob
    // que crece hacia adentro. Gana a la tira: no conviven.
    if (face === "agent" && dockAxis(dock.edge) === "y") {
      return { w: Math.max(long, PILL.islandCardW), h: thick + PILL.islandCardH };
    }
    // Cara dictado: la caja crece hacia adentro (el pivote del canto clava el
    // lado pegado), mismo blob pestaña + tarjeta. Gana a la tira abierta.
    if (face === "dictation") {
      return dockAxis(dock.edge) === "x"
        ? { w: thick + PILL.islandDictW, h: Math.max(long, PILL.islandDictH) }
        : { w: Math.max(long, PILL.islandDictW), h: thick + PILL.islandDictH };
    }
    // Cerrada: el aviso cuelga. Abierta, cede a la tira — si ganara, el hover
    // no podría desplegar las herramientas mientras un agente trabaja.
    if (face === "live" && !dock.expanded) {
      const rows = Math.max(1, Math.floor(liveRows) || 1);
      const liveH = rows * PILL.islandLiveRow;
      return dockAxis(dock.edge) === "x"
        ? { w: thick + PILL.islandDictW, h: Math.max(long, liveH) }
        : { w: Math.max(long, PILL.islandDictW), h: thick + liveH };
    }
    if (face === "agents") {
      const w = agentsBrowse
        ? PILL.islandBrowseW
        : agentsConsole
          ? PILL.islandAgentsW
          : PILL.islandAgentsSetupW;
      const h = agentsBrowse
        ? PILL.islandBrowseH
        : agentsConsole
          ? PILL.islandAgentsH
          : PILL.islandAgentsSetupH;
      // Con panel al costado la caja suma consola + panel, sin hueco. El alto
      // sale del mayor entre consola y panel (496 > 252: no cambia).
      const panelW = side ? sidePanelSpan() : 0;
      const panelH = side ? PILL.islandClipH : 0;
      return dockAxis(dock.edge) === "x"
        ? { w: thick + w + panelW, h: Math.max(long, h, panelH) }
        : { w: Math.max(long, w + panelW), h: thick + Math.max(h, panelH) };
    }
    if (face === "system") {
      // Medida propia: ver `islandSysH` en `pillStage`.
      return dockAxis(dock.edge) === "x"
        ? { w: thick + PILL.islandSysW, h: Math.max(long, PILL.islandSysH) }
        : { w: Math.max(long, PILL.islandSysW), h: thick + PILL.islandSysH };
    }
    if (face === "customize") {
      // Medida propia: ver `islandCustomH` en `pillStage`.
      return dockAxis(dock.edge) === "x"
        ? { w: thick + PILL.islandCustomW, h: Math.max(long, PILL.islandCustomH) }
        : { w: Math.max(long, PILL.islandCustomW), h: thick + PILL.islandCustomH };
    }
    if (isIslandPanelFace(face)) {
      return dockAxis(dock.edge) === "x"
        ? { w: thick + PILL.islandClipW, h: Math.max(long, PILL.islandClipH) }
        : { w: Math.max(long, PILL.islandClipW), h: thick + PILL.islandClipH };
    }
    // Abierta es la tira de herramientas: acoplada, la pill deja de ser un
    // indicador y pasa a ser el acceso. Se despliega A LO LARGO del borde, que
    // es el único eje donde hay lugar sin taparle la pantalla al usuario.
    //
    // Y SOLO a lo largo: acoplada no cuelga nada. Grabación y update son chips
    // dentro de la pestaña, contados en `islandCueCount`.
    if (dock.expanded) {
      // Nunca más corta que cerrada: la isla se abre con el puntero encima,
      // y si al abrirse encogiera, el cursor quedaría fuera y el ciclo
      // abrir/cerrar se realimentaría a 60 Hz. Con pocas herramientas a la
      // vista (se pueden esconder desde Ajustes) la tira puede quedar más
      // corta que la pestaña con avisos.
      const long = Math.max(
        islandStripLong(toolCount + islandLiveSlots(activity)),
        islandCue ? islandCueLong(islandCueCount, islandCueMsg) : PILL.islandLong,
      );
      return dockAxis(dock.edge) === "x"
        ? { w: PILL.islandTool, h: long }
        : { w: long, h: PILL.islandTool };
    }
    return dockAxis(dock.edge) === "x" ? { w: thick, h: long } : { w: long, h: thick };
  }
  return {
    w: Math.max(barW, PILL.bar),
    h: PILL.bar + agentStackHang(agentStack) + liveHang(activity, surface),
  };
}

/** El tamaño de la caja para un estado dado. */
export function targetFor(
  surface: Surface,
  barW: number,
  dock: Dock | null = null,
  activity: Activity = "idle",
  toolCount: number = WHEEL_TOOLS.length,
  islandCue: boolean = false,
  islandCueCount: number = 0,
  agentStack: number = 0,
  face: IslandFace = "tab",
  agentsConsole: boolean = false,
  /** Panel lateral junto a la consola (ver `contentFor`). */
  side: boolean = false,
  /** Filas de la cara ambiental de agentes (ver `contentFor`). */
  liveRows: number = 0,
): Size {
  return windowFor(
    contentFor(
      surface,
      barW,
      dock,
      activity,
      toolCount,
      islandCue,
      islandCueCount,
      agentStack,
      face,
      agentsConsole,
      false,
      side,
      false,
      liveRows,
    ),
  );
}

/**
 * ¿La isla muestra la cara de permiso del agente?
 *
 * Pura y testeable: auto-abre con un pedido nuevo (urgente, bloquea al
 * agente), no re-abre el que el usuario ya colapsó a mano, y solo en canto
 * horizontal acoplado (flotando el permiso ya tiene su tarjeta propia).
 */
export function islandFaceAgent(state: {
  surface: Surface;
  dock: Dock | null;
  authId: string | null;
  dismissedAuthId: string | null;
}): boolean {
  if (state.surface !== "edge" || !state.dock) return false;
  if (dockAxis(state.dock.edge) !== "y") return false;
  if (!state.authId) return false;
  return state.authId !== state.dismissedAuthId;
}

/**
 * Radio de la isla en un canto: pestaña = pastilla; panel alto = rectángulo
 * redondeado. Si el radio siguiera a `min(w,h)/2`, el clipboard se lee gota.
 */
export function islandNotchRadius(size: { w: number; h: number }): number {
  const cap = Math.min(size.w, size.h) / 2;
  return cap > PILL.islandClipR + 8 ? PILL.islandClipR : cap;
}

/** Panel de herramienta en la isla, en cualquier canto acoplado. */
export function islandFacePanel(state: {
  surface: Surface;
  dock: Dock | null;
  requested: boolean;
}): boolean {
  if (!state.requested) return false;
  return state.surface === "edge" && state.dock != null;
}

/**
 * Dictar acoplada muestra una cara de la isla: la onda (o el estado) cuelga
 * de la pestaña en vez de desacoplar la pill a una gota. Prioridad: gana a
 * las caras de herramienta (el dictado es transitorio y el usuario lo pidió)
 * y pierde contra el permiso de agente, que es urgente.
 */
export function islandFaceDictation(state: {
  surface: Surface;
  dock: Dock | null;
  dictating: boolean;
}): boolean {
  return state.surface === "edge" && state.dock != null && state.dictating;
}

/** Cara ambiental: una fila compacta por cada agente activo del notch. */
export function islandFaceLive(state: {
  surface: Surface;
  dock: Dock | null;
  live: boolean;
}): boolean {
  if (state.surface !== "edge" || !state.dock || !state.live) return false;
  // El hover abre la tira: si la cara live siguiera ganando, las
  // herramientas no existirían mientras un agente trabaja.
  return !state.dock.expanded;
}

/**
 * ¿Esta cara bloquea el hover que abre la tira?
 *
 * Clipboard, permiso y dictado son paneles que el usuario está usando: el
 * hover no los sustituye. `live` es un aviso, no un panel: el hover tiene
 * que poder abrir las herramientas.
 */
export function islandFaceBlocksHover(face: IslandFace): boolean {
  return face !== "tab" && face !== "live";
}

/**
 * ¿El cursor sigue dentro de un hit recordado (CSS del overlay)?
 *
 * Al abrir la tira la cara live encoge hacia el canto. Sin este hold, el
 * cursor queda en el vacío y el ciclo abrir/cerrar se realimenta.
 */
export function pointInRect(
  point: { x: number; y: number } | null | undefined,
  rect: { x: number; y: number; w: number; h: number } | null | undefined,
): boolean {
  if (!point || !rect || rect.w <= 0 || rect.h <= 0) return false;
  return (
    point.x >= rect.x &&
    point.x < rect.x + rect.w &&
    point.y >= rect.y &&
    point.y < rect.y + rect.h
  );
}

/** Historial en la isla. Mismo predicado que los otros paneles. */
export function islandFaceClipboard(state: {
  surface: Surface;
  dock: Dock | null;
  requested: boolean;
}): boolean {
  return islandFacePanel(state);
}

/**
 * Traer la pill al cursor la saca del canto.
 *
 * Acoplada, `contentFor` mide la pestaña (~10 px). Si el summon vuela sin
 * desacoplar, llega al puntero con forma de isla y se queda así: el vuelo
 * solo mueve, no cambia `surface`.
 */
export function undockForSummon(state: { surface: Surface; dock: Dock | null }): {
  surface: Surface;
  dock: Dock | null;
} {
  if (state.surface === "edge") {
    return { surface: "none", dock: null };
  }
  return { surface: state.surface, dock: state.dock };
}

/**
 * Abrir una tool desde el canto no desacopla.
 *
 * El summon al cursor (`undockForSummon`) sí: tiene que volar. Activar
 * clipboard/agentes/etc. ancla el float a la isla, no convierte la pestaña
 * en el disco flotante.
 */
export function shouldStayDockedOnActivate(surface: Surface): boolean {
  return surface === "edge";
}

/**
 * Recentrar al medio del techo solo si la pill ES la isla, en reposo.
 *
 * Si corre con la rueda abriéndose, el `dock.edge === "top"` que quedó
 * del notch tira el cuadrado de 252 px otra vez al canto: el clic y el
 * atajo desde el notch parecen no abrir nada, y al abortar queda el
 * disco flotante (modo pill), donde el atajo sí anda.
 */
export function shouldRecenterTopNotch(state: {
  surface: Surface;
  dock: Dock | null;
  flying?: boolean;
  opening?: boolean;
}): boolean {
  if (state.surface !== "edge") return false;
  if (state.dock?.edge !== "top") return false;
  if (state.flying || state.opening) return false;
  return true;
}

/**
 * Tamaño de la «unidad» que hay que centrar en el canto de arriba.
 *
 * `side` mira la CAJA que se está aplicando, no el layout: con panel, la caja
 * es consola + panel y centrarla entera correría la consola media diferencia.
 * Lo que va al medio del techo es la consola, así que la unidad le descuenta
 * el tramo del panel. Sin panel la unidad es la caja tal cual —que
 * es el caso del cierre, cuando la caja ya encogió y el layout todavía no—.
 */
export function notchRecenterSize(next: Size, side: boolean): Size {
  if (!side) return next;
  return { w: next.w - sidePanelSpan(), h: next.h };
}

/**
 * X con la que sale del layout con panel una caja en canto horizontal.
 *
 * Modela SOLO la salida (cierre total incluido): `from` es la caja APLICADA
 * con panel y `next` la que entra sin él. El pivote del canto centra la caja
 * ancha, así que al encoger la pestaña quedaría corrida; anclando por la
 * unidad —la columna de la consola— la pestaña cae donde estaba la consola.
 * En el cierre a consola es un no-op: `from.w - sidePanelSpan() === next.w`.
 *
 * No sirve para el eje x: ahí el canto ya clava el borde y no hay corrimiento.
 */
export function sideExitX(state: {
  at: { x: number };
  from: Size;
  next: Size;
}): number {
  const unitW = state.from.w - sidePanelSpan();
  return state.at.x + (unitW - state.next.w) / 2;
}

/**
 * ¿Hay que volver a un canto al abrir una tool?
 *
 * Desde la rueda (Ctrl+Q o clic) la pill siempre vuelve a su lugar: el
 * hogar se guardó antes del vuelo. Desde el canto, igual. Flotando a
 * mano, sin rueda, se queda donde está.
 */
export function shouldReturnToEdgeOnActivate(
  surface: Surface,
  dock: Dock | null,
): boolean {
  return surface === "edge" || surface === "wheel" || dock != null;
}

/** Tras salir del hit, la isla espera esto antes de volverse pestaña. */
export const ISLAND_COLLAPSE_MS = 400;
/** En «Más» el morph cambia el hit: hace falta un poco más. */
export const ISLAND_COLLAPSE_MORE_MS = 700;
/**
 * Con aviso de update, la isla no abre en el mismo cuadro del hover.
 *
 * El icono está EN la pestaña cerrada. Abrir al instante lo desmonta y el
 * clic cae en una herramienta. Un toque corto alcanza a apretar el aviso;
 * pasado el delay, la tira abre y el aviso cuelga.
 */
export const UPDATE_ISLAND_OPEN_DELAY_MS = 180;
/**
 * Disco flotante: espera un toque antes de abrir la rueda al hover.
 *
 * En el canto el hover es deliberado (hay que ir al borde). En medio de la
 * pantalla cruzar el disco no debería desplegar 252 px. El delay cubre un
 * sondeo de la isla (~100 ms) y un poco más.
 */
export const FLOAT_WHEEL_HOVER_OPEN_MS = 180;

/**
 * ¿Este `pointermove` cuenta como arrastre de la pill?
 *
 * El hover sintético de macOS dispara `pointermove` con `buttons === 0`. Si se
 * mezcla con el origen del gesto (coords del DOM), un clic en la isla —notch
 * de techo o de canto— se lee como arrastre y la herramienta no se activa.
 * La rueda no pasa por este gesto: sus gajos usan `onclick`.
 */
export function pointerMoveDrags(buttons: number): boolean {
  return buttons !== 0;
}

/**
 * ¿El gesto fue un clic? Se mide contra el `pointerdown` original.
 *
 * El origen del arrastre se re-siembra al pasar de DOM a Rust, y el hover
 * sintético mezcla coordenadas: con eso un clic en el disco flotante se
 * leía como arrastre y no abría la rueda (el atajo sí, no pasa por acá).
 */
export function pointerGestureWasClick(
  press: { x: number; y: number } | null,
  release: { x: number; y: number } | null,
  thresholdPx: number,
): boolean {
  if (!press || !release) return false;
  return Math.hypot(release.x - press.x, release.y - press.y) <= thresholdPx;
}

/**
 * ¿El hover ya cuenta para abrir la tira?
 *
 * Sin update, el primer sondeo abre. Con update y todavía cerrada, espera
 * `UPDATE_ISLAND_OPEN_DELAY_MS` de cursor encima.
 */
export function islandHoverOpens(input: {
  over: boolean;
  expanded: boolean;
  hasUpdate: boolean;
  hoveredMs: number;
}): boolean {
  if (!input.over) return false;
  if (input.expanded || !input.hasUpdate) return true;
  return input.hoveredMs >= UPDATE_ISLAND_OPEN_DELAY_MS;
}

/**
 * ¿La isla sigue abierta con este sondeo?
 *
 * `over` es el hit de Rust. Un false un cuadro no cierra: durante el morph
 * y el tooltip el cursor “sale” un instante. `leftAt` marca cuándo se fue.
 */
export function islandHoverStay(input: {
  over: boolean;
  now: number;
  leftAt: number | null;
  lingerMs: number;
}): { open: boolean; leftAt: number | null } {
  if (input.over) return { open: true, leftAt: null };
  const leftAt = input.leftAt ?? input.now;
  return { open: input.now - leftAt < input.lingerMs, leftAt };
}

/**
 * ¿El disco flotante mira el cursor para abrir/cerrar la rueda?
 *
 * Mismo contrato que la isla: Rust es la fuente, no `pointerenter`. En macOS
 * el primer clic sobre el disco se lo queda AppKit; el hover no pasa por eso.
 * Solo el disco en reposo, o la rueda que este hover abrió (un atajo no se
 * cierra al alejar el mouse).
 */
export function floatWheelHoverWatches(input: {
  surface: Surface;
  discOnly: boolean;
  heldByHover: boolean;
  collapsingFrom: "wheel" | null;
  /**
   * Cápsula idle con aviso (agente / update): el disco sigue siendo la
   * puerta a la rueda. Grabar y la cola no: ahí el hover no despliega.
   */
  idleCapsule?: boolean;
}): boolean {
  if (input.collapsingFrom === "wheel") return false;
  if (input.surface === "none" && (input.discOnly || input.idleCapsule)) return true;
  return input.surface === "wheel" && input.heldByHover;
}

/**
 * ¿El hover ya cuenta para abrir la rueda del disco flotante?
 *
 * Cerrada, espera `FLOAT_WHEEL_HOVER_OPEN_MS`. Abierta, el primer sondeo
 * encima la mantiene: no re-aplicar el delay a mitad de uso.
 */
export function floatWheelHoverOpens(input: {
  over: boolean;
  alreadyOpen: boolean;
  hoveredMs: number;
}): boolean {
  if (!input.over) return false;
  if (input.alreadyOpen) return true;
  return input.hoveredMs >= FLOAT_WHEEL_HOVER_OPEN_MS;
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
 * Arrastrar la rueda abierta la cierra: si no, no es isla y no se acopla
 * a un canto. El clic sin mover sigue cerrando por el núcleo.
 */
export function dragClosesWheel(surface: Surface): boolean {
  return surface === "wheel";
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
/** Pivote al abrir la rueda desde una isla: crece hacia el escritorio. */
export function bloomPivot(edge: DockEdge | null | undefined): Pivot {
  if (!edge) return "center";
  switch (edge) {
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

export function pivotFor(state: {
  surface: Surface;
  collapsingFrom: "wheel" | null;
  dock?: Dock | null;
  /**
   * El layout aplicado lleva panel al costado. En los cantos horizontales el
   * pivote deja de recentrar el eje libre: el borde del lado de la consola
   * queda fijo y la caja crece (y encoge) hacia el panel. En laterales no
   * cambia nada: su canto ya está clavado.
   */
  side?: boolean;
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
        return state.side ? "topLeft" : "dockTop";
      case "bottom":
        return state.side ? "bottomLeft" : "dockBottom";
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
export const FLIGHT_SKIP_PX = PILL.bar + 8;

/**
 * La pill está en reposo: la barra es SOLO el disco.
 *
 * Lo mira el CSS para la forma y lo mira la piel para saber si monta la gota,
 * así que tiene que ser una sola definición.
 */
export function isDiscOnly(state: {
  surface: Surface;
  activity: Activity;
  hasQueue: boolean;
  agentAlert: boolean;
  /** Chip de update al lado del disco: si no, el texto se recorta al disco. */
  hasUpdate?: boolean;
}): boolean {
  return (
    state.activity === "idle" &&
    !state.hasQueue &&
    !state.agentAlert &&
    !state.hasUpdate
  );
}

/**
 * ¿Medir el ancho de la barra compacta ahora?
 *
 * El timer y las ondas de grabación disparan `ResizeObserver` a cada tick.
 * Si se mide durante un arrastre, el reconciliador pelea con el gesto y la
 * pill salta.
 */
export function shouldMeasureBar(surface: Surface, dragging: boolean): boolean {
  return surface === "none" && !dragging;
}

/**
 * El ancho medido de la barra, con histéresis.
 *
 * Las ondas de grabación cambian `height` de hijos a ~60 Hz. En WebView2 eso
 * dispara `ResizeObserver` con ±1 px y el reconciliador reencuadra la caja
 * desde `topLeft`: la pastilla parece caminar. Con actividad viva (grabar /
 * dictar) solo se permite crecer; al volver a idle sí encoge.
 */
export function nextBarWidth(current: number, measured: number, live: boolean): number {
  const n = Math.max(PILL.bar, Math.ceil(measured));
  if (Math.abs(n - current) < 2) return current;
  if (live && n < current) return current;
  return n;
}

/**
 * ¿Publicar el disco junto a la gota en el campo líquido?
 *
 * Solo mientras la gota todavía no lo cubre. Cuando ambas comparten el borde
 * izquierdo (disco en reposo + pastilla ya expandida), el `smin` engorda ese
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
    areas.find((a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h) ??
    areas[0];
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
