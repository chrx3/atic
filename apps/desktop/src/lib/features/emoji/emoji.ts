/**
 * Búsqueda de emojis del launcher (modo `:`).
 *
 * Todo local: el catálogo es `emojiData.json` (CLDR vía emojibase, generado por
 * `scripts/gen-emoji-data.mjs`) y se carga con `import()` recién al entrar al
 * modo, así no pesa en el arranque del overlay.
 */
import type { ShortcutOs } from "$core/hotkeys";

export type EmojiRow = [string, number, string, string, string, number, string[]?];

export interface Emoji {
  char: string;
  group: number;
  nameEs: string;
  nameEn: string;
  /** Cinco variantes de tono uniforme (claro → oscuro), si el emoji las tiene. */
  skins?: string[];
  /** Nombres y palabras clave ya normalizados, para no recalcular por tecla. */
  names: string[];
  words: string[];
}

/** Orden de las categorías en la grilla; el 2 (componentes) no se genera. */
export const EMOJI_GROUPS = [0, 1, 3, 4, 5, 6, 7, 8, 9] as const;

/** Icono de cada categoría en la barra de chips. */
export const EMOJI_GROUP_ICON: Record<number, string> = {
  0: "😀",
  1: "👋",
  3: "🐶",
  4: "🍎",
  5: "✈️",
  6: "⚽",
  7: "💡",
  8: "❤️",
  9: "🏁",
};

/**
 * Última versión de Emoji que dibuja la fuente del sistema. Segoe UI Emoji
 * (Windows 11) llega a 15.0: lo más nuevo saldría como cuadrito vacío.
 */
const MAX_VERSION: Record<ShortcutOs, number> = {
  windows: 15,
  macos: Number.POSITIVE_INFINITY,
  other: 15,
};

export function normalizeEmojiText(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase().trim();
}

/** Banderas de país = dos indicadores regionales; Windows las dibuja como letras. */
function isCountryFlag(char: string): boolean {
  const first = char.codePointAt(0) ?? 0;
  return first >= 0x1f1e6 && first <= 0x1f1ff;
}

export function parseEmojiRows(rows: EmojiRow[], os: ShortcutOs): Emoji[] {
  const maxVersion = MAX_VERSION[os];
  return rows
    .filter(([char, , , , , version]) => {
      if (version > maxVersion) return false;
      return !(os === "windows" && isCountryFlag(char));
    })
    .map(([char, group, nameEs, nameEn, keywords, , skins]) => ({
      char,
      group,
      nameEs,
      nameEn,
      skins,
      names: [normalizeEmojiText(nameEs), normalizeEmojiText(nameEn)],
      words: normalizeEmojiText(keywords).split(/\s+/).filter(Boolean),
    }));
}

function tokenScore(token: string, emoji: Emoji): number {
  let best = 0;
  for (const name of emoji.names) {
    if (name === token) return 100;
    if (name.startsWith(token)) best = Math.max(best, 80);
    else if (name.split(/[\s:,-]+/).some((word) => word.startsWith(token))) {
      best = Math.max(best, 60);
    } else if (token.length >= 3 && name.includes(token)) best = Math.max(best, 20);
  }
  if (best < 60) {
    if (emoji.words.includes(token)) best = Math.max(best, 50);
    else if (emoji.words.some((word) => word.startsWith(token)))
      best = Math.max(best, 40);
  }
  return best;
}

/**
 * Cada palabra de la query tiene que calzar (nombre o palabra clave). Ante un
 * empate manda el orden de CLDR, que ya agrupa lo parecido.
 */
export function searchEmojis(list: Emoji[], query: string, limit = 120): Emoji[] {
  const tokens = normalizeEmojiText(query).split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return [];
  const scored: { emoji: Emoji; score: number; index: number }[] = [];
  list.forEach((emoji, index) => {
    let score = 0;
    for (const token of tokens) {
      const s = tokenScore(token, emoji);
      if (s === 0) return;
      score += s;
    }
    scored.push({ emoji, score, index });
  });
  scored.sort((a, b) => b.score - a.score || a.index - b.index);
  return scored.slice(0, limit).map((hit) => hit.emoji);
}

/** Tono 0 = amarillo por defecto; 1..5 = claro → oscuro. */
export function withSkin(emoji: Emoji, tone: number): string {
  if (tone < 1 || tone > 5 || !emoji.skins) return emoji.char;
  return emoji.skins[tone - 1] ?? emoji.char;
}

export type GridKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown";

/**
 * Mueve la selección en una grilla partida en secciones (recientes + una por
 * categoría), cada una con su última fila incompleta. Arriba/abajo conservan la
 * columna y saltan de sección; si la fila destino es más corta, cae en su
 * último emoji. `index` es plano: la suma de todas las secciones.
 */
export function moveInGrid(
  sizes: number[],
  index: number,
  key: GridKey,
  cols: number,
): number {
  const total = sizes.reduce((sum, size) => sum + size, 0);
  if (total === 0) return 0;
  if (key === "ArrowLeft") return Math.max(0, index - 1);
  if (key === "ArrowRight") return Math.min(total - 1, index + 1);

  const starts: number[] = [];
  let acc = 0;
  for (const size of sizes) {
    starts.push(acc);
    acc += size;
  }
  let section = sizes.findIndex(
    (size, i) => index >= starts[i] && index < starts[i] + size,
  );
  if (section < 0) return Math.min(index, total - 1);
  const local = index - starts[section];
  const col = local % cols;
  const row = Math.floor(local / cols);
  const rows = (size: number) => Math.ceil(size / cols);
  const at = (s: number, r: number) =>
    starts[s] + Math.min(r * cols + col, sizes[s] - 1);

  if (key === "ArrowDown") {
    if (row + 1 < rows(sizes[section])) return at(section, row + 1);
    do section += 1;
    while (section < sizes.length && sizes[section] === 0);
    return section < sizes.length ? at(section, 0) : index;
  }
  if (row > 0) return at(section, row - 1);
  do section -= 1;
  while (section >= 0 && sizes[section] === 0);
  return section >= 0 ? at(section, rows(sizes[section]) - 1) : index;
}

const RECENTS_KEY = "atic.emoji.recents";
const TONE_KEY = "atic.emoji.tone";
export const EMOJI_RECENTS_LIMIT = 16;

/** Recientes = emoji base (sin tono): el tono se aplica al mostrarlos. */
export function loadEmojiRecents(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((c) => typeof c === "string") : [];
  } catch {
    return [];
  }
}

export function pushEmojiRecent(recents: string[], char: string): string[] {
  const next = [char, ...recents.filter((c) => c !== char)].slice(
    0,
    EMOJI_RECENTS_LIMIT,
  );
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  } catch {
    /* sin storage no hay recientes, pero pegar sigue funcionando */
  }
  return next;
}

export function loadEmojiTone(): number {
  try {
    const tone = Number(localStorage.getItem(TONE_KEY));
    return Number.isInteger(tone) && tone >= 0 && tone <= 5 ? tone : 0;
  } catch {
    return 0;
  }
}

export function saveEmojiTone(tone: number): void {
  try {
    localStorage.setItem(TONE_KEY, String(tone));
  } catch {
    /* preferencia de esta sesión nomás */
  }
}

let catalog: Promise<Emoji[]> | null = null;

/** Carga (una vez) el catálogo filtrado para lo que la fuente del SO dibuja. */
export function loadEmojiCatalog(os: ShortcutOs): Promise<Emoji[]> {
  catalog ??= import("./emojiData.json")
    .then((mod) => parseEmojiRows(mod.default.rows as EmojiRow[], os))
    .catch((failure: unknown) => {
      catalog = null;
      throw failure;
    });
  return catalog;
}
