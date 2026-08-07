/**
 * El markdown que de verdad usa un agente, y nada más.
 *
 * Los agentes responden en markdown siempre. Pintarlo como texto plano es lo
 * que hacía que la consola se viera cruda: párrafos pegados, viñetas con
 * guiones sueltos y bloques de código indistinguibles de la prosa.
 *
 * Es un subconjunto a propósito —bloques de código, encabezados, listas,
 * tablas GFM, `código`, **negrita**— porque es lo que aparece. Sin HTML
 * embebido ni imágenes remotas: el riesgo de inyectar markup ajeno en la
 * ventana no compensa.
 *
 * Devuelve bloques ya tipados. La vista decide cómo se ven: acá no hay HTML,
 * así que no hay nada que sanear.
 */

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; text: string };

export type Block =
  | { kind: "p"; spans: Inline[] }
  | { kind: "h"; level: number; spans: Inline[] }
  | { kind: "li"; spans: Inline[]; ordered: boolean; marker: string }
  | { kind: "code"; lang: string; text: string }
  | { kind: "table"; headers: Inline[][]; rows: Inline[][][] }
  | { kind: "hr" };

/** ¿Línea separadora de tabla GFM (`|---|---`)? */
function isTableSeparator(line: string): boolean {
  const t = line.trim();
  if (!t.includes("-")) return false;
  return /^\|?[\t -:|]+\|[\t -:|]*$/.test(t);
}

/** Celdas de una fila con pipes; ignora pipes vacíos de borde. */
function tableCells(line: string): string[] {
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|")) s = s.slice(0, -1);
  return s.split("|").map((c) => c.trim());
}

function looksLikeTableRow(line: string): boolean {
  const t = line.trim();
  if (!t.includes("|")) return false;
  return tableCells(t).length >= 2;
}

/** Divide una línea en texto, `código` y **negrita**. */
export function inlines(line: string): Inline[] {
  const out: Inline[] = [];
  // Un solo recorrido con alternancia: anidar negrita dentro de código (o al
  // revés) no pasa en la práctica y resolverlo pediría un parser de verdad.
  const re = /`([^`]+)`|\*\*([^*]+)\*\*/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(line)) !== null) {
    if (m.index > last) out.push({ kind: "text", text: line.slice(last, m.index) });
    if (m[1] !== undefined) out.push({ kind: "code", text: m[1] });
    else out.push({ kind: "strong", text: m[2] });
    last = m.index + m[0].length;
  }
  if (last < line.length) out.push({ kind: "text", text: line.slice(last) });
  return out.length > 0 ? out : [{ kind: "text", text: line }];
}

export function parse(source: string): Block[] {
  const lines = source.split("\n");
  const blocks: Block[] = [];
  let paragraph: string[] = [];

  const flush = () => {
    if (paragraph.length === 0) return;
    // Se unen con salto y NO con espacio.
    //
    // Markdown diría que un salto simple es un espacio, pero acá el emisor es
    // un agente: no envuelve a 80 columnas, y sí manda salida por líneas
    // —`/usage`, listas de estado, tablas de texto—. Uniendo con espacio, ese
    // tipo de respuesta se convertía en un párrafo ilegible.
    blocks.push({ kind: "p", spans: inlines(paragraph.join("\n")) });
    paragraph = [];
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const fence = /^\s*```(\w*)/.exec(line);

    if (fence) {
      flush();
      const lang = fence[1] ?? "";
      const body: string[] = [];
      i += 1;
      // Un bloque sin cierre llega a la vista igual: mientras el agente
      // escribe, la última línea SIEMPRE está a medias, y esperar el cierre
      // dejaría la respuesta en blanco justo cuando se está formando.
      while (i < lines.length && !/^\s*```/.test(lines[i])) {
        body.push(lines[i]);
        i += 1;
      }
      blocks.push({ kind: "code", lang, text: body.join("\n") });
      continue;
    }

    if (/^\s*(-{3,}|_{3,}|\*{3,})\s*$/.test(line)) {
      flush();
      blocks.push({ kind: "hr" });
      continue;
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      blocks.push({
        kind: "h",
        level: heading[1].length,
        spans: inlines(heading[2]),
      });
      continue;
    }

    // Tabla GFM: cabecera + separador `|---|` + filas. Sin librería.
    if (
      looksLikeTableRow(line) &&
      i + 1 < lines.length &&
      isTableSeparator(lines[i + 1])
    ) {
      flush();
      const headers = tableCells(line).map((c) => inlines(c));
      i += 2; // salta separador
      const rows: Inline[][][] = [];
      while (i < lines.length && looksLikeTableRow(lines[i])) {
        const cells = tableCells(lines[i]).map((c) => inlines(c));
        // Rellena/recorta al ancho de la cabecera para no romper el grid.
        while (cells.length < headers.length) cells.push([{ kind: "text", text: "" }]);
        if (cells.length > headers.length) cells.length = headers.length;
        rows.push(cells);
        i += 1;
      }
      i -= 1; // el for hará +1
      blocks.push({ kind: "table", headers, rows });
      continue;
    }

    const bullet = /^\s*[-*+]\s+(.*)$/.exec(line);
    if (bullet) {
      flush();
      blocks.push({
        kind: "li",
        spans: inlines(bullet[1]),
        ordered: false,
        marker: "•",
      });
      continue;
    }

    const ordered = /^\s*(\d+)[.)]\s+(.*)$/.exec(line);
    if (ordered) {
      flush();
      blocks.push({
        kind: "li",
        spans: inlines(ordered[2]),
        ordered: true,
        marker: `${ordered[1]}.`,
      });
      continue;
    }

    if (line.trim() === "") {
      flush();
      continue;
    }

    // `trimEnd` y no `trim`: la sangría de una salida alineada es información.
    paragraph.push(line.trimEnd());
  }

  flush();
  return blocks;
}

/** Una línea de diff, ya clasificada. */
export interface DiffLine {
  sign: " " | "+" | "-";
  text: string;
}

/**
 * Diff de una edición, a partir del `input` de la herramienta.
 *
 * Se compara por líneas y sin algoritmo de similitud: `Edit` ya trae los dos
 * fragmentos exactos que cambian, así que el diff útil es «esto sale, esto
 * entra». Un LCS acá solo serviría para que un cambio de una palabra se viera
 * como una línea modificada en vez de dos, y no vale su complejidad.
 */
export function editDiff(input: unknown): DiffLine[] | null {
  if (!input || typeof input !== "object") return null;
  const o = input as Record<string, unknown>;

  const old = typeof o.old_string === "string" ? o.old_string : null;
  const next = typeof o.new_string === "string" ? o.new_string : null;
  if (old !== null && next !== null) {
    return [
      ...old.split("\n").map((text): DiffLine => ({ sign: "-", text })),
      ...next.split("\n").map((text): DiffLine => ({ sign: "+", text })),
    ];
  }

  // `Write` no tiene versión anterior: todo el contenido es alta.
  const content = typeof o.content === "string" ? o.content : null;
  if (content !== null) {
    return content.split("\n").map((text): DiffLine => ({ sign: "+", text }));
  }
  return null;
}
