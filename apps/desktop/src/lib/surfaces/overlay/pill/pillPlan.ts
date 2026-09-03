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
export function islandCueLong(n: number): number {
  const marks = Math.max(1, Math.floor(n) || 1);
  const cues = marks * PILL.islandCueBtn + (marks - 1) * PILL.islandGap;
  const inner = PILL.islandMark + PILL.islandGap + cues + 12;
  return Math.max(PILL.islandLong, inner);
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

/** Extra de caja para la gota viva (diámetro + cuello). */
export function liveHang(
  activity: Activity,
  surface: Surface = "none",
): number {
  // Solo la rueda cuelga. Acoplada y flotando, la actividad vive DENTRO de la
  // silueta: la cara de la marca dice cuál es y el stop es un chip más.
  //
  // Colgar una gota hacía que la forma cambiara con el estado, y era lo que
  // hacía que la pestaña cerrada, la tira abierta y la cápsula flotante no se
  // leyeran como la misma cosa. La rueda es su propio escenario cuadrado, así
  // que ahí la gota no rompe ninguna continuidad.
  if (surface !== "wheel") return 0;
  if (activity === "recording" || activity === "dictating") {
    return PILL.recDrop + PILL.recDropGap;
  }
  return 0;
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
): Size {
  if (surface === "wheel") {
    const side = PILL.wheel - PILL.pad * 2;
    const hang = liveHang(activity, "wheel") > 0 ? PILL.wheelLiveHang : 0;
    return { w: side, h: side + hang };
  }
  if (surface === "edge" && dock) {
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
        islandCue ? islandCueLong(islandCueCount) : PILL.islandLong,
      );
      return dockAxis(dock.edge) === "x"
        ? { w: PILL.islandTool, h: long }
        : { w: long, h: PILL.islandTool };
    }
    // En reposo, una pestaña: fina contra el borde y larga a lo largo de él.
    // Grabando se alarga, no engorda: el estado entró a la cara de la marca.
    const thick = islandCue ? PILL.islandCueThick : PILL.islandThick;
    const long = islandCue
      ? islandCueLong(islandCueCount)
      : PILL.islandLong;
    return dockAxis(dock.edge) === "x"
      ? { w: thick, h: long }
      : { w: long, h: thick };
  }
  return { w: Math.max(barW, PILL.bar), h: PILL.bar };
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
    ),
  );
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
