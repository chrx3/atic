/**
 * Qué herramientas viven en la pill, y cuáles detrás de «Más».
 *
 * En una rueda radial cada gajo que se suma achica a todos los demás, así que
 * elegir cuáles están no es una preferencia cosmética: es lo que decide si se
 * le puede apuntar a una herramienta sin mirar. De ahí las dos salidas — se
 * esconde lo que no se usa nunca, y lo que se usa poco baja a un segundo
 * anillo en vez de robarle ángulo al primero.
 *
 * La regla vive acá y no en la pill porque lo que importa son los casos
 * raros —listas vacías, ids de herramientas que ya no existen, una misma
 * herramienta en las dos listas—, y eso no se comprueba mirando la pill.
 */
import { WHEEL_TOOLS, type ToolDef, type ToolId } from "./tools";

/** El gajo que abre el segundo anillo. No es una herramienta. */
export const PILL_MORE_ID = "more";

/** La vuelta al primer paso. En la rueda la hace el núcleo; la tira no tiene. */
export const PILL_BACK_ID = "back";

/**
 * Abre la ventana principal. Tampoco es una herramienta: no tiene slot ni
 * acción en `toolActions`. Vive siempre al final de «Más».
 */
export const PILL_WINDOW_ID = "window";

/**
 * Abre el editor de la pill. Solo en la tira del canto, al final de «Más»:
 * es donde se edita (ver `PillCustomize`), y la rueda no tiene dónde.
 */
export const PILL_CUSTOMIZE_ID = "customize";

/** Lo que puede ocupar un gajo: una herramienta, la puerta al submenú, o Ventana. */
export type PillWheelId = ToolId | typeof PILL_MORE_ID | typeof PILL_WINDOW_ID;

/** Lo mismo, más el «atrás» que la tira necesita y la rueda no. */
export type PillStripId = PillWheelId | typeof PILL_BACK_ID | typeof PILL_CUSTOMIZE_ID;

export type PillLayout = {
  /** Primer anillo, sin contar el gajo «Más». */
  ring: ToolDef[];
  /** Segundo anillo de herramientas. «Más» y Ventana se muestran igual. */
  more: ToolDef[];
  /** Fuera de la pill. Siguen en la ventana principal y en sus atajos. */
  hidden: ToolDef[];
};

/** Los ids conocidos, en el orden pedido, sin repetir. */
function pick(ids: readonly string[], taken: ReadonlySet<string>): ToolDef[] {
  const out: ToolDef[] = [];
  const seen = new Set(taken);
  for (const id of ids) {
    if (seen.has(id)) continue;
    const tool = WHEEL_TOOLS.find((item) => item.id === id);
    if (!tool) continue;
    seen.add(id);
    out.push(tool);
  }
  return out;
}

export function pillLayout(
  ringIds: readonly string[] = [],
  moreIds: readonly string[] = [],
): PillLayout {
  const ring = pick(ringIds, new Set());
  const more = pick(moreIds, new Set(ring.map((tool) => tool.id)));

  // Sin nada configurado —o con una config vieja de la que no sobrevivió
  // ningún id— la pill vuelve a traerlas todas. Una rueda vacía no es una
  // preferencia, es una pill que no sirve para nada.
  if (ring.length === 0 && more.length === 0) {
    return { ring: [...WHEEL_TOOLS], more: [], hidden: [] };
  }

  // Todo en «Más» y nada en el anillo dejaría un único gajo que solo abre
  // otro anillo: dos pasos para llegar a cualquier cosa. Sube el submenú.
  let ringOut = ring;
  let moreOut = more;
  if (ringOut.length === 0) {
    ringOut = moreOut;
    moreOut = [];
  }

  // Tools nuevas: una rueda ya armada no las esconde. Entran al anillo
  // para que se vean; Ajustes → Pill las mueve o las saca.
  const extra = newcomers(ringOut, moreOut);
  ringOut = extra.length === 0 ? ringOut : [...ringOut, ...extra];
  return { ring: ringOut, more: moreOut, hidden: hiddenFrom([...ringOut, ...moreOut]) };
}

/** Dónde puede quedar una herramienta: primer anillo, «Más» o fuera de la pill. */
export type PillBucket = "ring" | "more" | "hidden";

/** Las dos listas que se guardan en la config (`pill_tools`, `pill_more_tools`). */
export type PillToolIds = { ring: ToolId[]; more: ToolId[] };

/**
 * Coloca `id` en `to`, en la posición `index` de esa lista ya sin él (al final
 * si falta). Es la única regla de edición: la usan Ajustes y el editor de la
 * pill, para que las dos no puedan decidir distinto.
 *
 * La rueda no puede quedar vacía: sacar la última devuelve null. `pillLayout`
 * ya se defiende de eso, pero lo haría deshaciendo en silencio lo que el
 * usuario acaba de hacer.
 */
export function placePillTool(
  layout: PillLayout,
  id: ToolId,
  to: PillBucket,
  index?: number,
): PillToolIds | null {
  const ring = layout.ring.map((tool) => tool.id).filter((other) => other !== id);
  const more = layout.more.map((tool) => tool.id).filter((other) => other !== id);
  if (to !== "ring" && ring.length === 0) return null;
  const insert = (list: ToolId[]) => {
    const at = index === undefined ? list.length : Math.max(0, Math.min(index, list.length));
    list.splice(at, 0, id);
  };
  if (to === "ring") insert(ring);
  else if (to === "more") insert(more);
  return { ring, more };
}

/** Catálogo que se ofreció después de que ya había ruedas personalizadas. */
const OFFER_IN_RING: readonly ToolId[] = ["system"];

function newcomers(ring: ToolDef[], more: ToolDef[]): ToolDef[] {
  const taken = new Set([...ring, ...more].map((tool) => tool.id));
  return WHEEL_TOOLS.filter(
    (tool) => OFFER_IN_RING.includes(tool.id) && !taken.has(tool.id),
  );
}

function hiddenFrom(shown: readonly ToolDef[]): ToolDef[] {
  const ids = new Set(shown.map((tool) => tool.id));
  return WHEEL_TOOLS.filter((tool) => !ids.has(tool.id));
}

/**
 * Qué muestra la tira acoplada al borde, paso por paso.
 *
 * Las herramientas siguen el mismo escalón que la rueda. Ventana no: en el
 * canto (`windowOnFirst`) va en el primer paso, porque la isla vive del hover
 * y el segundo anillo no se alcanza a pulsar. La rueda no pasa esa opción y
 * Ventana sigue detrás de «Más».
 */
export function pillStripPage(
  layout: PillLayout,
  page: "ring" | "more" = "ring",
  opts: { windowOnFirst?: boolean } = {},
): PillStripId[] {
  const onFirst = opts.windowOnFirst === true;
  if (page === "more") {
    const ids: PillStripId[] = [PILL_BACK_ID, ...layout.more.map((tool) => tool.id)];
    ids.push(onFirst ? PILL_CUSTOMIZE_ID : PILL_WINDOW_ID);
    return ids;
  }
  const ids: PillStripId[] = layout.ring.map((tool) => tool.id);
  if (layout.more.length > 0 || !onFirst) ids.push(PILL_MORE_ID);
  if (onFirst) ids.push(PILL_WINDOW_ID);
  return ids;
}
