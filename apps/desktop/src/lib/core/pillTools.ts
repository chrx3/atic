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

/** Lo que puede ocupar un gajo: una herramienta, o la puerta al submenú. */
export type PillWheelId = ToolId | typeof PILL_MORE_ID;

/** Lo mismo, más el «atrás» que la tira necesita y la rueda no. */
export type PillStripId = PillWheelId | typeof PILL_BACK_ID;

export type PillLayout = {
  /** Primer anillo, sin contar el gajo «Más». */
  ring: ToolDef[];
  /** Segundo anillo. Vacío = no hay «Más» que mostrar. */
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
  if (ring.length === 0) {
    return { ring: more, more: [], hidden: hiddenFrom(more) };
  }

  return { ring, more, hidden: hiddenFrom([...ring, ...more]) };
}

function hiddenFrom(shown: readonly ToolDef[]): ToolDef[] {
  const ids = new Set(shown.map((tool) => tool.id));
  return WHEEL_TOOLS.filter((tool) => !ids.has(tool.id));
}

/**
 * Qué muestra la tira acoplada al borde, paso por paso.
 *
 * Repite el mismo escalón que la rueda en vez de aplanar las dos listas. No es
 * por ahorrar largo —contra un canto de pantalla sobra sitio—: es que si «Más»
 * existe en una silueta y no en la otra, el usuario tiene que aprender dos
 * mapas para la misma preferencia. La diferencia es de dónde sale la vuelta:
 * la rueda tiene núcleo y la tira no, así que acá el «atrás» es un botón más.
 */
export function pillStripPage(
  layout: PillLayout,
  page: "ring" | "more" = "ring",
): PillStripId[] {
  if (page === "more") {
    return [PILL_BACK_ID, ...layout.more.map((tool) => tool.id)];
  }
  const ids: PillStripId[] = layout.ring.map((tool) => tool.id);
  if (layout.more.length > 0) ids.push(PILL_MORE_ID);
  return ids;
}
