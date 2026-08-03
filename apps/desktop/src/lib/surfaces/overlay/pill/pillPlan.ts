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

/** Qué hay desplegado. Es uno de los tres ejes ortogonales de la pill. */
export type Surface = "none" | "wheel" | "clipboard" | "snippets";

/** Superficies que se despliegan bajo la barra. La rueda no es una: crece. */
export type PanelKind = "clipboard" | "snippets";

export function isPanel(surface: Surface): surface is PanelKind {
  return surface === "clipboard" || surface === "snippets";
}

/**
 * El contenido que la pill tiene que poder mostrar, en píxeles.
 *
 * `barW` se mide del DOM en vez de mantenerse en una tabla de anchos por
 * estado: esa tabla fue el origen del desajuste original, porque el ancho real
 * depende de la fuente, del texto del timer y de si entró un chip.
 */
export function contentFor(surface: Surface, barW: number): Size {
  if (surface === "wheel") {
    const side = PILL.wheel - PILL.pad * 2;
    return { w: side, h: side };
  }
  if (isPanel(surface)) return { w: PILL.panelW, h: PILL.bar + PILL.panelH };
  return { w: Math.max(barW, PILL.bar), h: PILL.bar };
}

/** El tamaño de la caja para un estado dado. */
export function targetFor(surface: Surface, barW: number): Size {
  return windowFor(contentFor(surface, barW));
}

/**
 * Qué punto se conserva en el próximo reencuadre.
 *
 * Es la función más delicada de la pill y la razón principal de que este
 * archivo exista.
 *
 * Al abrir es simétrico al cierre. Al cerrar hay que saber **de qué** se está
 * cerrando, no adónde se va: para cuando esto corre, `surface` ya vale `"none"`
 * y los dos colapsos son indistinguibles. Sin `collapsingFrom` los dos usaban
 * `center`, y ese era el «punto C»: el panel se encogía hacia su propio centro
 * —unos 130 px arriba y 80 a la izquierda de la barra— y recién desde ahí
 * arrancaba el vuelo al hogar.
 *
 * Y en reposo el pivote es `topLeft`, nunca `center`: el ancho de la barra
 * cambia solo —entra el timer, tictaquea de 0:09 a 0:10, aparece el badge de la
 * cola— y con pivote al centro CADA cambio corría la pill media diferencia. Al
 * arrancar, el primer encogimiento la movía 53 px.
 */
export function pivotFor(state: {
  surface: Surface;
  collapsingFrom: "wheel" | "panel" | null;
  /** El panel había abierto hacia arriba. */
  panelUp: boolean;
}): Pivot {
  if (state.surface === "wheel") return "center";
  if (isPanel(state.surface)) return "panel";
  if (state.collapsingFrom === "panel") return state.panelUp ? "bottomLeft" : "topLeft";
  if (state.collapsingFrom === "wheel") return "center";
  return "topLeft";
}

/**
 * ¿Este reencuadre se anima en el sitio?
 *
 * Solo los cambios de la barra compacta: disco ↔ dictado ↔ grabación ↔ cola,
 * donde el salto se leía como un parpadeo. Los colapsos de panel y de rueda
 * tienen su propia coreografía —encoger y después volar, o el morph continuo— y
 * animar acá largaría un tween que el vuelo siguiente cancelaría a mitad de
 * camino.
 *
 * El primer reencuadre tampoco: al arrancar no hay «estado anterior» desde el
 * cual transicionar, solo la ventana acomodándose.
 */
export function morphsInPlace(state: {
  from: Size | null;
  surface: Surface;
  collapsingFrom: "wheel" | "panel" | null;
}): boolean {
  return (
    state.from !== null && state.collapsingFrom === null && state.surface === "none"
  );
}

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
    !isPanel(state.surface) &&
    state.activity === "idle" &&
    !state.hasQueue &&
    !state.agentAlert
  );
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
