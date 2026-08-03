export type SummarySectionKind =
  "summary" | "topics" | "decisions" | "tasks" | "general";

export interface SummaryListItem {
  text: string;
  checked: boolean | null;
}

export type SummaryBlock =
  | { type: "paragraph"; text: string }
  | { type: "list"; ordered: boolean; items: SummaryListItem[] };

export interface SummarySection {
  id: string;
  title: string;
  kind: SummarySectionKind;
  blocks: SummaryBlock[];
}

const KNOWN_SECTION =
  /^(resumen|temas tratados|temas|decisiones|acuerdos|pr[oó]ximos pasos|tareas|acciones|conclusiones|cierre)\s*:?\s*$/i;

function cleanInline(value: string): string {
  return value
    .replace(/\[([^\]]+)]\([^)]+\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

function classifySection(title: string): SummarySectionKind {
  const normalized = title.toLocaleLowerCase("es");
  if (normalized.includes("resumen") || normalized.includes("contexto")) {
    return "summary";
  }
  if (normalized.includes("tema")) return "topics";
  if (normalized.includes("decisi") || normalized.includes("acuerdo")) {
    return "decisions";
  }
  if (
    normalized.includes("tarea") ||
    normalized.includes("paso") ||
    normalized.includes("acci")
  ) {
    return "tasks";
  }
  return "general";
}

function sectionId(title: string, index: number): string {
  const slug = title
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
  return `${slug || "seccion"}-${index}`;
}

/**
 * Convierte el Markdown simple de los proveedores IA en un modelo seguro para UI.
 * Tolera streaming incompleto, encabezados legacy y texto plano sin usar innerHTML.
 */
export function parseSummaryDocument(
  source: string,
  defaultTitle = "Resumen",
): SummarySection[] {
  let lines = source.replace(/\r\n?/g, "\n").split("\n");
  if (lines[0]?.trim().startsWith("```")) lines = lines.slice(1);
  if (lines.at(-1)?.trim() === "```") lines = lines.slice(0, -1);

  const sections: SummarySection[] = [];
  let current: SummarySection | null = null;
  let paragraph: string[] = [];
  let list: Extract<SummaryBlock, { type: "list" }> | null = null;

  function ensureSection(): SummarySection {
    if (!current) {
      current = {
        id: sectionId(defaultTitle, sections.length),
        title: defaultTitle,
        kind: classifySection(defaultTitle),
        blocks: [],
      };
      sections.push(current);
    }
    return current;
  }

  function flushParagraph() {
    const text = cleanInline(paragraph.join(" "));
    if (text) ensureSection().blocks.push({ type: "paragraph", text });
    paragraph = [];
  }

  function flushList() {
    if (list?.items.length) ensureSection().blocks.push(list);
    list = null;
  }

  function startSection(rawTitle: string) {
    flushParagraph();
    flushList();
    const title = cleanInline(rawTitle.replace(/:$/, "")) || defaultTitle;
    current = {
      id: sectionId(title, sections.length),
      title,
      kind: classifySection(title),
      blocks: [],
    };
    sections.push(current);
  }

  for (const rawLine of lines) {
    const line = rawLine.trim();
    const markdownHeading = line.match(/^#{1,4}\s+(.+?)\s*#*$/);
    const boldHeading = line.match(/^\*\*([^*]+)\*\*\s*:?\s*$/);
    const legacyHeading = KNOWN_SECTION.test(line) ? line : null;
    const heading = markdownHeading?.[1] ?? boldHeading?.[1] ?? legacyHeading;

    if (heading) {
      startSection(heading);
      continue;
    }

    const item = line.match(/^\s*(?:(\d+)[.)]|[-*•])\s+(?:\[([ xX])\]\s+)?(.+?)\s*$/);
    if (item) {
      flushParagraph();
      const ordered = Boolean(item[1]);
      if (list && list.ordered !== ordered) flushList();
      list ??= { type: "list", ordered, items: [] };
      list.items.push({
        text: cleanInline(item[3]),
        checked: item[2] === undefined ? null : item[2].toLowerCase() === "x",
      });
      continue;
    }

    if (!line) {
      flushParagraph();
      flushList();
      continue;
    }

    flushList();
    paragraph.push(line);
  }

  flushParagraph();
  flushList();
  return sections;
}
