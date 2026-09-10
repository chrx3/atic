/**
 * Un resumen de reunión → bloques del tablero.
 *
 * Las secciones de tareas y decisiones (y cualquier lista con checkbox) salen
 * como checklist. El resto, como texto, en columna de documento: una minuta en
 * tiras de 280px se parte en demasiadas cajas.
 *
 * Dos números de acá son la razón de bugs que ya pasaron:
 * - El alto declarado tiene que ser el alto REAL del bloque (padding + ítems +
 *   el botón «Añadir ítem»). Si queda corto, `.objeto { overflow: hidden }`
 *   recorta la última fila y el texto desaparece.
 * - El corte de página se mide contra la celda donde cae el resumen, no contra
 *   el origen del tablero: si no, el rebalse aparecía una celda más a la
 *   derecha de donde soltaste y el papel crecía solo.
 */
import { parseSummaryDocument, type SummarySection } from "$core/summary-format";
import type { NoteBlock } from "$core/types";
import {
  MARGEN,
  PAGINA_H,
  PAGINA_W,
  TEXTO_ALTO_MIN,
  TEXTO_FUENTE,
  TEXTO_INTERLINEADO,
  TEXTO_PAD_X,
  TEXTO_PAD_Y,
} from "./flipLayout";

/** Ancho de la columna del resumen: el doble de una nota suelta. */
export const RESUMEN_W = 560;

/** Separación entre bloques del resumen. */
const GAP = 16;

/** Padding de `.objeto.lista`: 18 arriba + 6 abajo. */
const PAD_LISTA_Y = 24;
/** Alto de una fila: `.objeto li { min-height: 28px }`. */
const ALTO_ITEM = 28;
/** El botón «Añadir ítem» (`.mas`) se dibuja siempre: 4 de margen + 28. */
const ALTO_BOTON_CHECK = 32;
/** Ancho medio de carácter a 13.5px, para estimar líneas sin canvas. */
const ANCHO_CARACTER = 7.2;

export function etiquetaResumen(startedAt: string): string {
  const d = new Date(startedAt);
  if (Number.isNaN(d.getTime())) return "Resumen reunión";
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yy = String(d.getFullYear()).slice(-2);
  return `Resumen reunión ${dd}-${mm}-${yy}`;
}

/**
 * Alto real de un bloque de texto.
 *
 * Estima las líneas por ancho de carácter: acá no hay canvas para medir de
 * verdad, y pasarse un poco es mejor que quedarse corto y recortar.
 */
export function altoTextoResumen(body: string, ancho = RESUMEN_W): number {
  const porLinea = Math.max(8, Math.floor((ancho - TEXTO_PAD_X) / ANCHO_CARACTER));
  let lineas = 0;
  for (const cruda of body.split("\n")) {
    lineas += Math.max(1, Math.ceil(cruda.length / porLinea));
  }
  const alto = TEXTO_PAD_Y + lineas * TEXTO_FUENTE * TEXTO_INTERLINEADO;
  return Math.min(PAGINA_H - MARGEN * 2, Math.max(TEXTO_ALTO_MIN, alto));
}

/** Alto real de una checklist, con el botón de añadir adentro. */
export function altoListaResumen(items: number): number {
  if (items <= 0) return 0;
  return PAD_LISTA_Y + items * ALTO_ITEM + ALTO_BOTON_CHECK;
}

type Pieza =
  | { tipo: "texto"; body: string; w: number; h: number }
  | {
      tipo: "check";
      items: { text: string; done: boolean }[];
      w: number;
      h: number;
    };

function esAccion(section: SummarySection): boolean {
  return section.kind === "tasks" || section.kind === "decisions";
}

/** Las piezas del resumen con su alto ya resuelto, todavía sin coordenadas. */
function piezasDe(etiqueta: string, sections: SummarySection[]): Pieza[] {
  const out: Pieza[] = [];

  function texto(body: string) {
    const recorte = body.trim();
    if (!recorte) return;
    out.push({
      tipo: "texto",
      body: recorte,
      w: RESUMEN_W,
      h: altoTextoResumen(recorte),
    });
  }

  function lista(items: { text: string; done: boolean }[]) {
    if (items.length === 0) return;
    out.push({
      tipo: "check",
      items,
      w: RESUMEN_W,
      h: altoListaResumen(items.length),
    });
  }

  texto(etiqueta);

  for (const section of sections) {
    let tituloPuesto = section.kind === "summary";
    for (const bloque of section.blocks) {
      if (bloque.type === "list") {
        const comoCheck =
          esAccion(section) || bloque.items.some((i) => i.checked !== null);
        if (comoCheck) {
          if (!tituloPuesto) {
            texto(section.title);
            tituloPuesto = true;
          }
          lista(
            bloque.items.map((item) => ({
              text: item.text,
              done: item.checked === true,
            })),
          );
          continue;
        }
        const lineas = bloque.items.map((item, i) =>
          bloque.ordered ? `${i + 1}. ${item.text}` : `• ${item.text}`,
        );
        texto(
          tituloPuesto ? lineas.join("\n") : `${section.title}\n${lineas.join("\n")}`,
        );
        tituloPuesto = true;
        continue;
      }
      texto(tituloPuesto ? bloque.text : `${section.title}\n${bloque.text}`);
      tituloPuesto = true;
    }
  }

  return out;
}

/**
 * Dónde arranca el resumen, dentro de la celda donde lo soltaste.
 *
 * Se alinea al margen de esa celda y, si entero no entra debajo del punto de
 * soltado, sube al techo en vez de partirse a la celda de al lado.
 */
function origenDe(
  origen: { x: number; y: number },
  alto: number,
): { x: number; y: number } {
  const celdaX = Math.floor(origen.x / PAGINA_W) * PAGINA_W;
  const y = Math.max(MARGEN, origen.y);
  return {
    x: celdaX + MARGEN,
    y: y + alto <= PAGINA_H - MARGEN ? y : MARGEN,
  };
}

export function bloquesDeResumen(
  body: string,
  etiqueta: string,
  nuevoId: () => string,
  origen: { x: number; y: number } = { x: MARGEN, y: MARGEN },
): NoteBlock[] {
  const piezas = piezasDe(etiqueta, parseSummaryDocument(body, etiqueta));
  if (piezas.length === 0) return [];

  const altoTotal =
    piezas.reduce((total, pieza) => total + pieza.h, 0) + GAP * (piezas.length - 1);
  const desde = origenDe(origen, altoTotal);

  const out: NoteBlock[] = [];
  let celda = 0;
  let y = desde.y;
  for (const pieza of piezas) {
    // Sólo se pasa a la celda de al lado cuando de verdad no entra abajo.
    if (y > desde.y && y + pieza.h > PAGINA_H - MARGEN) {
      celda += 1;
      y = MARGEN;
    }
    const x = desde.x + celda * PAGINA_W;
    if (pieza.tipo === "texto") {
      out.push({
        kind: "text",
        id: nuevoId(),
        body: pieza.body,
        x,
        y,
        w: pieza.w,
        h: pieza.h,
      });
    } else {
      out.push({
        kind: "check",
        id: nuevoId(),
        items: pieza.items.map((item) => ({ id: nuevoId(), ...item })),
        x,
        y,
        w: pieza.w,
        h: pieza.h,
      });
    }
    y += pieza.h + GAP;
  }

  return out;
}
