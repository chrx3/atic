import type { InkStroke, NoteBlock } from "$core/types";

export const MARGEN = 24;
/**
 * Piso del zoom.
 *
 * Con la celda en 900 de alto, una ventana baja necesita alejar bastante para
 * que la página entre entera; en 0.45 el encuadre quedaba corto y el papel se
 * veía recortado contra el borde.
 */
export const ZOOM_MIN = 0.3;
export const ZOOM_MAX = 2.4;
/** Arrastre interno del cajón (historial, textos o capturas) hacia el papel. */
export const FLIP_MIME = "application/x-atic-flip";

export type FlipFuente = "clip" | "snip" | "cap" | "meet";

export function payloadFlip(tipo: FlipFuente, id: string): string {
  return `${tipo}:${id}`;
}

export function leerPayloadFlip(raw: string): { tipo: FlipFuente; id: string } | null {
  const i = raw.indexOf(":");
  if (i < 1) return null;
  const tipo = raw.slice(0, i);
  const id = raw.slice(i + 1);
  if ((tipo !== "clip" && tipo !== "snip" && tipo !== "cap" && tipo !== "meet") || !id)
    return null;
  return { tipo, id };
}

export const BORRAR_RADIO = 16;
/** Tamaños del borrador (radio en px de papel): chico, mediano y grande. */
export const BORRAR_RADIOS = [8, 16, 32] as const;

export const TEXTO_W = 280;
export const TEXTO_H = 96;
export const LISTA_W = 260;
export const LISTA_H = 120;

/**
 * Tipografía del texto del tablero, junta en un solo lugar.
 *
 * El `textarea` la toma por CSS (`font: inherit` → `--rb-font`) y el lienzo que
 * mide para ajustar el tamaño usa exactamente la misma: si una cambia y la otra
 * no, el ajuste miente y el texto se corta. El export lee de acá también.
 */
export const FUENTE_TABLERO =
  '"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif';
export const TEXTO_FUENTE = 13.5;
/** Piso de legibilidad: por debajo de esto no se encoge aunque no entre. */
export const TEXTO_FUENTE_MIN = 8.5;
export const TEXTO_INTERLINEADO = 1.5;
/** Padding interno del recuadro de texto: espeja `.objeto textarea`. */
export const TEXTO_PAD_X = 24;
export const TEXTO_PAD_Y = 28;
/** Alto de una caja de una línea sin encoger: un encabezado cabe con aire. */
export const TEXTO_ALTO_MIN = 48;
/** Ancho de una caja recién creada o casi vacía: entra el placeholder. */
export const TEXTO_ANCHO_MIN = 160;
/** Hasta acá se ensancha sola; de ahí en más el texto baja de renglón. */
export const TEXTO_ANCHO_MAX = 560;
/** Aire para el cursor al final del renglón más largo: sin él, envuelve antes. */
const TEXTO_CURSOR = 6;

export type Herramienta =
  "select" | "hand" | "draw" | "highlight" | "eraser" | "text" | "check";

/** Varios elementos elegidos a la vez: objetos por id, trazos por índice. */
export interface Grupo {
  ids: string[];
  trazos: number[];
}

/**
 * Lo que toca un recuadro de selección.
 *
 * Un objeto entra si su marco se cruza con el recuadro, aunque sea en parte;
 * un trazo, si alguno de sus puntos cae adentro. Así no hace falta encerrar
 * todo entero, y una caja grande casi vacía no se lleva trazos lejanos.
 */
export function enRecuadro(bloques: NoteBlock[], r: Marco): Grupo {
  const x1 = r.x + r.w;
  const y1 = r.y + r.h;
  const ids: string[] = [];
  const trazos: number[] = [];
  for (const bloque of bloques) {
    if (bloque.kind === "ink") {
      bloque.strokes.forEach((trazo, i) => {
        if (trazo.points.some(([x, y]) => x >= r.x && x <= x1 && y >= r.y && y <= y1)) {
          trazos.push(i);
        }
      });
      continue;
    }
    const m = marcoDe(bloque);
    if (m.x < x1 && m.x + m.w > r.x && m.y < y1 && m.y + m.h > r.y) ids.push(bloque.id);
  }
  return { ids, trazos };
}

/** Todo el tablero como grupo: lo que elige Ctrl+A. */
export function grupoCompleto(bloques: NoteBlock[]): Grupo {
  const ids: string[] = [];
  const trazos: number[] = [];
  for (const bloque of bloques) {
    if (bloque.kind === "ink") bloque.strokes.forEach((_, i) => trazos.push(i));
    else ids.push(bloque.id);
  }
  return { ids, trazos };
}

/** El tablero sin lo del grupo. No muta la entrada. */
export function quitarGrupo(bloques: NoteBlock[], grupo: Grupo): NoteBlock[] {
  const fuera = new Set(grupo.ids);
  const trazosFuera = new Set(grupo.trazos);
  return bloques
    .filter((b) => !fuera.has(b.id))
    .map((b) =>
      b.kind === "ink"
        ? { ...b, strokes: b.strokes.filter((_, i) => !trazosFuera.has(i)) }
        : b,
    );
}

/**
 * Página lógica del tablero, en píxeles de papel. Todo empieza en una.
 *
 * Es también la página del export (una celda = una página), así que agrandarla
 * da más lugar para trabajar sin agregar páginas. 4:3: entra una minuta entera
 * sin partirse y la tarjeta del reverso —que es 4:3— la muestra completa.
 */
export const PAGINA_W = 1200;
export const PAGINA_H = 900;

/** Cuántas celdas hay, de izquierda a derecha. */
export function cantidadPaginas(papelW: number): number {
  return Math.max(1, Math.round(papelW / PAGINA_W));
}
/** Cota contra datos corruptos (un bloque en y=1e9 no puede romper el papel). */
export const TABLERO_MAX = 12000;

/** Ancho máximo al pegar una imagen: cabe en la página con margen. */
const IMAGEN_MAX = PAGINA_W - MARGEN * 2;

/** Rectángulo del papel: origen (puede ser negativo) más tamaño. */
export interface Tablero {
  ox: number;
  oy: number;
  w: number;
  h: number;
}

export function marcoDe(bloque: NoteBlock): {
  x: number;
  y: number;
  w: number;
  h: number;
} {
  return {
    x: bloque.x ?? 0,
    y: bloque.y ?? 0,
    w: bloque.w ?? 0,
    h: bloque.h ?? 0,
  };
}

export type Marco = { x: number; y: number; w: number; h: number };

/**
 * Intersección de un marco con una celda, en coords locales de la celda.
 *
 * Vive acá y no en el export porque el tablero también necesita saber qué cae
 * en cada página: la tira de miniaturas filtra con esto.
 */
export function marcoEnPagina(
  x: number,
  y: number,
  w: number,
  h: number,
  indice: number,
): Marco | null {
  const x0 = indice * PAGINA_W;
  const izq = Math.max(x, x0);
  const der = Math.min(x + w, x0 + PAGINA_W);
  const arr = Math.max(y, 0);
  const aba = Math.min(y + h, PAGINA_H);
  if (der - izq < 1 || aba - arr < 1) return null;
  return { x: izq - x0, y: arr, w: der - izq, h: aba - arr };
}

export interface MiniaturaPagina {
  indice: number;
  piezas: {
    id: string;
    tipo: NoteBlock["kind"];
    x: number;
    y: number;
    w: number;
    h: number;
    filas: number;
    asset: string;
    renglones: number;
  }[];
  trazos: { puntos: string; color: string; ancho: number }[];
}

/**
 * Lo mínimo para dibujar cada miniatura: cajas en % de la celda.
 *
 * Texto y listas van como barritas y no como texto: a 84px de ancho no se lee
 * nada, y lo que hace reconocible una página es la foto y el dibujo.
 */
export function miniaturasTablero(
  bloques: NoteBlock[],
  paginas: number,
): MiniaturaPagina[] {
  const tinta =
    bloques.find((b): b is Extract<NoteBlock, { kind: "ink" }> => b.kind === "ink") ??
    null;
  return Array.from({ length: paginas }, (_, indice) => {
    const piezas = bloques
      .filter((b) => b.kind !== "ink" && bloqueEnPagina(b, indice))
      .map((b) => {
        const m = marcoDe(b);
        const x0 = indice * PAGINA_W;
        const caja = {
          id: b.id,
          tipo: b.kind,
          x: ((m.x - x0) / PAGINA_W) * 100,
          y: (m.y / PAGINA_H) * 100,
          w: (m.w / PAGINA_W) * 100,
          h: (m.h / PAGINA_H) * 100,
        };
        if (b.kind === "check") {
          return {
            ...caja,
            filas: Math.min(4, b.items.length),
            asset: "",
            renglones: 0,
          };
        }
        if (b.kind === "image") {
          return { ...caja, filas: 0, asset: b.asset, renglones: 0 };
        }
        return {
          ...caja,
          filas: 0,
          asset: "",
          renglones: Math.min(4, Math.max(1, Math.round(m.h / 24))),
        };
      });
    const trazos =
      tinta && bloqueEnPagina(tinta, indice)
        ? tinta.strokes.map((trazo) => ({
            puntos: trazo.points.map(([x, y]) => `${x},${y}`).join(" "),
            color: trazo.color.length === 9 ? trazo.color.slice(0, 7) : trazo.color,
            ancho: trazo.width * 2,
          }))
        : [];
    return { indice, piezas, trazos };
  });
}

/** ¿Este bloque toca la celda `indice`? La tinta se mide por sus puntos. */
export function bloqueEnPagina(bloque: NoteBlock, indice: number): boolean {
  if (bloque.kind === "ink") {
    const x0 = indice * PAGINA_W;
    const x1 = x0 + PAGINA_W;
    return bloque.strokes.some((trazo) =>
      trazo.points.some(([x, y]) => x >= x0 && x < x1 && y >= 0 && y < PAGINA_H),
    );
  }
  const m = marcoDe(bloque);
  return marcoEnPagina(m.x, m.y, m.w, m.h, indice) !== null;
}

export function tieneMarco(bloque: NoteBlock): boolean {
  return (bloque.w ?? 0) > 0 && (bloque.h ?? 0) > 0;
}

export function clampZoom(valor: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, valor));
}

export function puntoEnPapel(
  papel: DOMRect,
  clientX: number,
  clientY: number,
  anchoLogico: number,
  ox = 0,
  oy = 0,
): { x: number; y: number } {
  const escala = papel.width / anchoLogico;
  if (escala <= 0) return { x: ox, y: oy };
  return {
    x: ox + (clientX - papel.left) / escala,
    y: oy + (clientY - papel.top) / escala,
  };
}

/** Líneas interiores entre páginas, en coordenadas del papel. */
export function lineasPagina(tablero: Tablero): {
  verticales: number[];
  horizontales: number[];
} {
  const verticales: number[] = [];
  for (
    let x = (Math.floor(tablero.ox / PAGINA_W) + 1) * PAGINA_W;
    x < tablero.ox + tablero.w;
    x += PAGINA_W
  ) {
    verticales.push(x - tablero.ox);
  }
  const horizontales: number[] = [];
  for (
    let y = (Math.floor(tablero.oy / PAGINA_H) + 1) * PAGINA_H;
    y < tablero.oy + tablero.h;
    y += PAGINA_H
  ) {
    horizontales.push(y - tablero.oy);
  }
  return { verticales, horizontales };
}

export function tamanoImagen(
  anchoNat: number,
  altoNat: number,
): { w: number; h: number } {
  const w = Math.min(IMAGEN_MAX, Math.max(80, anchoNat));
  const ratio = altoNat <= 0 ? 1 : altoNat / Math.max(1, anchoNat);
  return { w, h: Math.max(48, w * ratio) };
}

/** Ancho del papel para que ningún objeto quede fuera de una celda. */
export function anchoParaBloques(lista: NoteBlock[]): number {
  let maxX = PAGINA_W;
  for (const bloque of lista) {
    if (bloque.kind === "ink") continue;
    const m = marcoDe(bloque);
    maxX = Math.max(maxX, m.x + m.w + MARGEN);
  }
  const celdas = Math.max(1, Math.ceil(maxX / PAGINA_W));
  return Math.min(TABLERO_MAX, celdas * PAGINA_W);
}

/** Notas viejas (todo en 0) se apilan. Las que ya tienen marco se dejan. */
export function colocarSiHaceFalta(lista: NoteBlock[]): NoteBlock[] {
  const objetos = lista.filter((b) => b.kind !== "ink");
  if (objetos.length === 0 || objetos.every(tieneMarco)) return lista;
  let y = MARGEN;
  return lista.map((bloque) => {
    if (bloque.kind === "ink" || tieneMarco(bloque)) return bloque;
    if (bloque.kind === "image") {
      const { w, h } = tamanoImagen(bloque.width, bloque.height);
      const next = { ...bloque, x: MARGEN, y, w, h };
      y += h + 16;
      return next;
    }
    if (bloque.kind === "check") {
      const next = { ...bloque, x: MARGEN, y, w: LISTA_W, h: LISTA_H };
      y += LISTA_H + 16;
      return next;
    }
    const next = { ...bloque, x: MARGEN, y, w: TEXTO_W, h: TEXTO_H };
    y += TEXTO_H + 16;
    return next;
  });
}

/**
 * Envuelve palabras al ancho dado; `medir` mide en px una cadena.
 *
 * Vive acá y no en el export porque el tablero también necesita saber cuánto
 * ocupa un texto antes de dibujarlo.
 */
export function envolver(
  texto: string,
  maxWidth: number,
  medir: (s: string) => number,
): string[] {
  const palabras = texto.split(/\s+/).filter(Boolean);
  if (palabras.length === 0) return [""];
  const lineas: string[] = [];
  let actual = palabras[0] ?? "";
  for (const palabra of palabras.slice(1)) {
    const prueba = `${actual} ${palabra}`;
    if (medir(prueba) <= maxWidth) actual = prueba;
    else {
      lineas.push(actual);
      actual = palabra;
    }
  }
  lineas.push(actual);
  return lineas;
}

/** Alto que pide el cuerpo a un tamaño de fuente dado. */
export function altoEnvolvente(
  body: string,
  ancho: number,
  tamano: number,
  medir: (s: string, tamano: number) => number,
): number {
  const util = Math.max(8, ancho - TEXTO_PAD_X);
  let lineas = 0;
  for (const cruda of body.split("\n")) {
    lineas += envolver(cruda, util, (s) => medir(s, tamano)).length;
  }
  return TEXTO_PAD_Y + lineas * tamano * TEXTO_INTERLINEADO;
}

/**
 * El tamaño que pide un texto para verse entero al tamaño de fuente base.
 *
 * Ancho: el del renglón más largo, entre el mínimo y el tope. Pasado el tope
 * envuelve. Alto: lo que ocupan los renglones a ese ancho.
 */
export function medidaTexto(
  body: string,
  medir: (s: string, tamano: number) => number,
  tamano = TEXTO_FUENTE,
): { w: number; h: number } {
  let masLargo = 0;
  for (const linea of body.split("\n")) {
    masLargo = Math.max(masLargo, medir(linea, tamano));
  }
  const w = Math.ceil(
    Math.min(
      TEXTO_ANCHO_MAX,
      Math.max(TEXTO_ANCHO_MIN, masLargo + TEXTO_PAD_X + TEXTO_CURSOR),
    ),
  );
  return { w, h: altoParaAncho(body, w, medir, tamano) };
}

/** Alto que pide el texto a un ancho dado, sin bajar del mínimo de una caja. */
export function altoParaAncho(
  body: string,
  ancho: number,
  medir: (s: string, tamano: number) => number,
  tamano = TEXTO_FUENTE,
): number {
  return Math.max(
    TEXTO_ALTO_MIN,
    Math.ceil(altoEnvolvente(body, ancho, tamano, medir)),
  );
}

/**
 * El tamaño de fuente más grande que hace entrar el cuerpo en la caja.
 *
 * Nunca sube del base ni baja del mínimo. Sirve para que encoger una nota no
 * esconda lo que dice: antes el `textarea` scrolleaba sin barra y con
 * `pointer-events: none` —o sea, el texto simplemente desaparecía.
 */
export function ajustarFuente(
  body: string,
  ancho: number,
  alto: number,
  medir: (s: string, tamano: number) => number,
  base = TEXTO_FUENTE,
  minimo = TEXTO_FUENTE_MIN,
): number {
  if (!body.trim() || ancho <= 0 || alto <= 0) return base;
  const entra = (t: number) => altoEnvolvente(body, ancho, t, medir) <= alto;
  if (entra(base)) return base;
  let chico = minimo;
  let grande = base;
  for (let i = 0; i < 12; i++) {
    const medio = (chico + grande) / 2;
    if (entra(medio)) chico = medio;
    else grande = medio;
  }
  return Math.max(minimo, Math.round(chico * 10) / 10);
}

/**
 * Borra por donde pasa el borrador: quita los puntos dentro del radio y parte
 * cada trazo en los tramos que quedan. Los tramos de un solo punto se van.
 */
export function borrarPuntos(
  trazos: InkStroke[],
  x: number,
  y: number,
  radio: number,
): InkStroke[] {
  const salida: InkStroke[] = [];
  for (const trazo of trazos) {
    let tramo: [number, number][] = [];
    const cerrar = () => {
      if (tramo.length >= 2) salida.push({ ...trazo, points: tramo });
      tramo = [];
    };
    for (const [px, py] of trazo.points) {
      if (Math.hypot(px - x, py - y) <= radio) cerrar();
      else tramo.push([px, py]);
    }
    cerrar();
  }
  return salida;
}

/** Distancia de un punto a un segmento. */
function distanciaSegmento(
  x: number,
  y: number,
  [ax, ay]: [number, number],
  [bx, by]: [number, number],
): number {
  const dx = bx - ax;
  const dy = by - ay;
  const largo2 = dx * dx + dy * dy;
  const t =
    largo2 === 0
      ? 0
      : Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / largo2));
  return Math.hypot(x - (ax + t * dx), y - (ay + t * dy));
}

/**
 * El trazo que está bajo el punto, o -1.
 *
 * Mide contra los segmentos y no contra los puntos: un trazo recto y rápido
 * tiene pocos puntos y entre ellos también es trazo. Gana el de más arriba
 * (el último dibujado), como con los objetos.
 */
export function trazoEnPunto(
  trazos: InkStroke[],
  x: number,
  y: number,
  tolerancia: number,
): number {
  for (let i = trazos.length - 1; i >= 0; i--) {
    const trazo = trazos[i];
    if (!trazo) continue;
    const alcance = tolerancia + trazo.width / 2;
    const puntos = trazo.points;
    if (puntos.length === 1 && puntos[0]) {
      if (Math.hypot(x - puntos[0][0], y - puntos[0][1]) <= alcance) return i;
      continue;
    }
    for (let j = 1; j < puntos.length; j++) {
      const a = puntos[j - 1];
      const b = puntos[j];
      if (a && b && distanciaSegmento(x, y, a, b) <= alcance) return i;
    }
  }
  return -1;
}

/** Caja de un trazo, contando su grosor. */
export function cajaTrazo(trazo: InkStroke): Marco {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const [x, y] of trazo.points) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  if (!Number.isFinite(x0)) return { x: 0, y: 0, w: 0, h: 0 };
  const pad = trazo.width / 2;
  return { x: x0 - pad, y: y0 - pad, w: x1 - x0 + pad * 2, h: y1 - y0 + pad * 2 };
}

/** El trazo corrido `dx, dy`. */
export function moverTrazo(trazo: InkStroke, dx: number, dy: number): InkStroke {
  return { ...trazo, points: trazo.points.map(([x, y]) => [x + dx, y + dy]) };
}

/**
 * El trazo escalado por `factor` desde la esquina de arriba a la izquierda de
 * su caja, que queda quieta. El grosor escala con el dibujo: así se ve igual,
 * solo más grande o más chico.
 */
export function escalarTrazo(trazo: InkStroke, factor: number): InkStroke {
  const caja = cajaTrazo(trazo);
  const f = Math.max(0.05, factor);
  const pad = trazo.width / 2;
  const ox = caja.x + pad;
  const oy = caja.y + pad;
  return {
    ...trazo,
    width: Math.max(0.5, trazo.width * f),
    points: trazo.points.map(([x, y]) => [ox + (x - ox) * f, oy + (y - oy) * f]),
  };
}

/**
 * La página de un objeto o trazo: la de su centro.
 *
 * Lo que cruza el borde entre dos páginas es de una sola, así que eliminar
 * una página nunca lo parte a la mitad.
 */
function paginaDe(marco: Marco): number {
  return Math.floor((marco.x + marco.w / 2) / PAGINA_W);
}

/** Cuánto hay en la página `indice`: lo que se perdería al eliminarla. */
export function contenidoDePagina(
  bloques: NoteBlock[],
  indice: number,
): { objetos: number; trazos: number } {
  let objetos = 0;
  let trazos = 0;
  for (const bloque of bloques) {
    if (bloque.kind === "ink") {
      trazos += bloque.strokes.filter((t) => paginaDe(cajaTrazo(t)) === indice).length;
    } else if (paginaDe(marcoDe(bloque)) === indice) {
      objetos++;
    }
  }
  return { objetos, trazos };
}

/**
 * El tablero sin la página `indice`: se va lo que había en ella y lo de las
 * páginas de la derecha se corre una página a la izquierda, para que no
 * quede un hueco. No muta la entrada.
 */
export function quitarPaginaDe(bloques: NoteBlock[], indice: number): NoteBlock[] {
  const salida: NoteBlock[] = [];
  for (const bloque of bloques) {
    if (bloque.kind === "ink") {
      const strokes = bloque.strokes
        .filter((t) => paginaDe(cajaTrazo(t)) !== indice)
        .map((t) =>
          paginaDe(cajaTrazo(t)) > indice ? moverTrazo(t, -PAGINA_W, 0) : t,
        );
      salida.push({ ...bloque, strokes });
      continue;
    }
    const pagina = paginaDe(marcoDe(bloque));
    if (pagina === indice) continue;
    salida.push(
      pagina > indice ? { ...bloque, x: (bloque.x ?? 0) - PAGINA_W } : bloque,
    );
  }
  return salida;
}

/** Tiñe un hex `#rrggbb` con alfa para el resaltador; lo demás pasa igual. */
export function conAlfa(hex: string, alfaHex = "66"): string {
  return /^#[0-9a-fA-F]{6}$/.test(hex) ? `${hex}${alfaHex}` : hex;
}

/** Borra el trazo entero más cercano al punto; no toca nada si está lejos. */
export function borrarLineaCercana(
  trazos: InkStroke[],
  x: number,
  y: number,
  radio: number,
): { trazos: InkStroke[]; borro: boolean } {
  let mejor: InkStroke | null = null;
  let mejorD = Infinity;
  for (const trazo of trazos) {
    for (const [px, py] of trazo.points) {
      const d = Math.hypot(px - x, py - y);
      if (d < mejorD) {
        mejorD = d;
        mejor = trazo;
      }
    }
  }
  if (!mejor || mejorD > radio) return { trazos, borro: false };
  return { trazos: trazos.filter((t) => t !== mejor), borro: true };
}
