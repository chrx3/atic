/**
 * Recuerdo y etiquetas de modelos de agentes.
 *
 * El catálogo vivo lo da el backend (`agent_list_models` / `active.models`):
 * cada CLI informa los suyos. Acá solo se guarda la última elección por
 * backend en `localStorage` y se resuelve una etiqueta legible.
 */

export interface ModelChoice {
  /** Lo que se le pasa al CLI. */
  id: string;
  label: string;
  /** Para qué sirve. */
  note: string;
}

/** Modelo con esfuerzos opcionales (forma del backend). */
export interface ModelWithEfforts {
  id: string;
  name?: string;
  label?: string;
  description?: string;
  note?: string;
  efforts?: { id: string; description: string }[];
  defaultEffort?: string;
  supportsFast?: boolean;
}

const STORAGE_KEY = "atic-agent-models";
const FILTER_STORAGE_KEY = "atic-agent-model-filter";
const EFFORT_STORAGE_KEY = "atic-agent-efforts";
const FAST_STORAGE_KEY = "atic-agent-fast";
const MODE_STORAGE_KEY = "atic-agent-modes";
const BACKEND_STORAGE_KEY = "atic-agent-backend";
const CWD_STORAGE_KEY = "atic-agent-cwds";

/** Modos de permiso que la UI ofrece (ids estables). */
const KNOWN_MODES = new Set([
  "manual",
  "acceptEdits",
  "plan",
  "bypassPermissions",
]);

/** Backends cuyo catálogo de modelos se puede filtrar en el selector. */
export const FILTERABLE_BACKENDS = ["cursor", "opencode"] as const;

export function isFilterableBackend(id: string): boolean {
  return (FILTERABLE_BACKENDS as readonly string[]).includes(id);
}

/** Etiqueta de un id en la lista disponible, o el propio id. */
export function modelLabelFor(
  id: string,
  available: { id: string; label?: string; name?: string }[] = [],
): string {
  const hit = available.find((m) => m.id === id);
  if (!hit) return id || "Modelo";
  return hit.label ?? hit.name ?? id;
}

/** Label corta del nivel: low → Low, xhigh → Extra High. */
export function effortShortLabel(effortId: string): string {
  switch (effortId) {
    case "default":
      return "Default";
    case "none":
      return "None";
    case "low":
      return "Low";
    case "medium":
      return "Medium";
    case "high":
      return "High";
    case "xhigh":
      return "Extra High";
    case "max":
      return "Max";
    case "minimal":
      return "Minimal";
    case "low-thinking":
      return "Low Thinking";
    case "medium-thinking":
      return "Medium Thinking";
    case "high-thinking":
      return "High Thinking";
    case "xhigh-thinking":
      return "Extra High Thinking";
    case "max-thinking":
      return "Max Thinking";
    case "none-thinking":
      return "None Thinking";
    default:
      return effortId
        .split("-")
        .map((w) => (w ? w[0]!.toUpperCase() + w.slice(1) : w))
        .join(" ");
  }
}

/**
 * Resuelve un id (grupo o slug wire) a `{ modelId, effortId, fast }`.
 *
 * Cursor agrupa variantes bajo un id base; el effort es el nivel lógico y
 * Fast es un switch aparte.
 */
export function resolveModelChoice(
  models: ModelWithEfforts[],
  wireOrGroup: string,
): { modelId: string; effortId: string; fast: boolean } {
  if (!wireOrGroup) return { modelId: "", effortId: "", fast: false };

  for (const m of models) {
    if (m.id === wireOrGroup) {
      return {
        modelId: m.id,
        effortId: m.defaultEffort ?? m.efforts?.[0]?.id ?? "",
        fast: false,
      };
    }
  }

  // Slug wire: partir sufijos conocidos en el cliente.
  const { base, level, fast } = splitWireClient(wireOrGroup);
  const group = models.find((m) => m.id === base);
  if (group) {
    const effortId = group.efforts?.some((e) => e.id === level)
      ? level
      : (group.defaultEffort ?? group.efforts?.[0]?.id ?? level);
    return { modelId: group.id, effortId, fast: !!group.supportsFast && fast };
  }

  return { modelId: wireOrGroup, effortId: "", fast: false };
}

function splitWireClient(id: string): {
  base: string;
  level: string;
  fast: boolean;
} {
  let rest = id;
  let fast = false;
  if (rest.endsWith("-fast") && rest.length > 5) {
    rest = rest.slice(0, -5);
    fast = true;
  }
  const levels = [
    "extra-high-thinking",
    "xhigh-thinking",
    "medium-thinking",
    "none-thinking",
    "low-thinking",
    "high-thinking",
    "max-thinking",
    "extra-high",
    "xhigh",
    "medium",
    "none",
    "low",
    "high",
    "max",
    "minimal",
  ];
  for (const suf of levels) {
    const needle = `-${suf}`;
    if (rest.endsWith(needle) && rest.length > needle.length) {
      let level = suf;
      if (suf === "extra-high") level = "xhigh";
      if (suf === "extra-high-thinking") level = "xhigh-thinking";
      return {
        base: rest.slice(0, -needle.length),
        level,
        fast,
      };
    }
  }
  return { base: rest, level: "default", fast };
}

/** Etiqueta legible del esfuerzo para el chip. */
export function effortLabelFor(
  models: ModelWithEfforts[],
  modelId: string,
  effortId: string,
): string {
  if (!effortId) return "Esfuerzo";
  return effortShortLabel(effortId);
}

/**
 * Expande ids de filtro: un id de grupo implica todas sus variantes wire,
 * y un id wire viejo se mapea a su grupo.
 */
function expandFilterIds(
  models: ModelWithEfforts[],
  ids: string[],
): Set<string> {
  const allowed = new Set<string>();
  for (const id of ids) {
    const { modelId } = resolveModelChoice(models, id);
    if (models.some((m) => m.id === modelId)) {
      allowed.add(modelId);
    } else {
      allowed.add(id);
    }
  }
  return allowed;
}

function readMap(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "string" && v) out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

function readEffortMap(): Record<string, string> {
  try {
    const raw = localStorage.getItem(EFFORT_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "string" && v) out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

function readFastMap(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(FAST_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, boolean> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "boolean") out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

function readFilterMap(): Record<string, string[]> {
  try {
    const raw = localStorage.getItem(FILTER_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string[]> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (!Array.isArray(v)) continue;
      const ids = v.filter((x): x is string => typeof x === "string" && !!x);
      if (ids.length > 0) out[k] = ids;
    }
    return out;
  } catch {
    return {};
  }
}

function readStringMap(key: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof v === "string" && v) out[k] = v;
    }
    return out;
  } catch {
    return {};
  }
}

function writeStringMap(key: string, map: Record<string, string>): void {
  try {
    localStorage.setItem(key, JSON.stringify(map));
  } catch {
    // ignore
  }
}

/** Último modelo recordado para este backend, si sigue en la lista. */
export function rememberedModel(
  backendId: string,
  available: ModelWithEfforts[],
): string {
  if (!backendId || available.length === 0) return "";
  const remembered = readMap()[backendId];
  if (remembered) {
    const { modelId } = resolveModelChoice(available, remembered);
    if (available.some((m) => m.id === modelId)) return modelId;
  }
  return available[0]?.id ?? "";
}

/** Último esfuerzo recordado para este backend+modelo, si sigue válido. */
export function rememberedEffort(
  backendId: string,
  modelId: string,
  available: ModelWithEfforts[],
): string {
  if (!backendId || !modelId) return "";
  const m = available.find((x) => x.id === modelId);
  if (!m?.efforts?.length) return "";
  const key = `${backendId}:${modelId}`;
  const remembered = readEffortMap()[key];
  if (remembered && m.efforts.some((e) => e.id === remembered)) {
    return remembered;
  }
  return m.defaultEffort ?? m.efforts[0]?.id ?? "";
}

/** Último fast recordado para este backend+modelo. */
export function rememberedFast(
  backendId: string,
  modelId: string,
  available: ModelWithEfforts[],
): boolean {
  if (!backendId || !modelId) return false;
  const m = available.find((x) => x.id === modelId);
  if (!m?.supportsFast) return false;
  const key = `${backendId}:${modelId}`;
  return readFastMap()[key] ?? false;
}

/** Guarda la elección para la próxima apertura. */
export function rememberModel(backendId: string, modelId: string): void {
  if (!backendId || !modelId) return;
  try {
    const map = readMap();
    map[backendId] = modelId;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
}

/** Guarda el esfuerzo elegido para ese backend+modelo. */
export function rememberEffort(
  backendId: string,
  modelId: string,
  effortId: string,
): void {
  if (!backendId || !modelId || !effortId) return;
  try {
    const map = readEffortMap();
    map[`${backendId}:${modelId}`] = effortId;
    localStorage.setItem(EFFORT_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
}

/** Guarda el switch Fast para ese backend+modelo. */
export function rememberFast(
  backendId: string,
  modelId: string,
  fast: boolean,
): void {
  if (!backendId || !modelId) return;
  try {
    const map = readFastMap();
    map[`${backendId}:${modelId}`] = fast;
    localStorage.setItem(FAST_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
}

/** Último modo de permisos recordado para este backend. */
export function rememberedMode(backendId: string): string {
  if (!backendId) return "manual";
  const mode = readStringMap(MODE_STORAGE_KEY)[backendId];
  return mode && KNOWN_MODES.has(mode) ? mode : "manual";
}

/** Guarda el modo de permisos (escudo) para ese backend. */
export function rememberMode(backendId: string, mode: string): void {
  if (!backendId || !KNOWN_MODES.has(mode)) return;
  const map = readStringMap(MODE_STORAGE_KEY);
  map[backendId] = mode;
  writeStringMap(MODE_STORAGE_KEY, map);
}

/** Último backend (pestaña) recordado, si sigue disponible. */
export function rememberedBackend(
  backends: { id: string; available?: boolean }[],
): string {
  if (backends.length === 0) return "";
  try {
    const id = localStorage.getItem(BACKEND_STORAGE_KEY) ?? "";
    if (id && backends.some((b) => b.id === id)) return id;
  } catch {
    // ignore
  }
  return backends.find((b) => b.available)?.id ?? backends[0]?.id ?? "";
}

/** Guarda la pestaña de agente elegida. */
export function rememberBackend(backendId: string): void {
  if (!backendId) return;
  try {
    localStorage.setItem(BACKEND_STORAGE_KEY, backendId);
  } catch {
    // ignore
  }
}

/** Última carpeta de trabajo recordada para este backend. */
export function rememberedCwd(backendId: string): string {
  if (!backendId) return "";
  return readStringMap(CWD_STORAGE_KEY)[backendId] ?? "";
}

/** Guarda la carpeta de trabajo para ese backend. */
export function rememberCwd(backendId: string, cwd: string): void {
  if (!backendId) return;
  const map = readStringMap(CWD_STORAGE_KEY);
  if (!cwd.trim()) {
    delete map[backendId];
  } else {
    map[backendId] = cwd;
  }
  writeStringMap(CWD_STORAGE_KEY, map);
}

/**
 * Ids visibles configurados para un backend.
 * `null` = mostrar todos (sin filtro guardado).
 */
export function visibleModelIds(backendId: string): string[] | null {
  if (!backendId || !isFilterableBackend(backendId)) return null;
  const ids = readFilterMap()[backendId];
  return ids && ids.length > 0 ? ids : null;
}

/**
 * Guarda la lista blanca de modelos visibles.
 * Un array vacío borra el filtro (mostrar todos).
 */
export function setVisibleModelIds(backendId: string, ids: string[]): void {
  if (!backendId || !isFilterableBackend(backendId)) return;
  try {
    const map = readFilterMap();
    if (ids.length === 0) {
      delete map[backendId];
    } else {
      map[backendId] = [...new Set(ids.filter(Boolean))];
    }
    localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
}

/**
 * Aplica el filtro de visibilidad. Sin filtro, o si ninguno de los ids
 * guardados sigue en el catálogo (filtro obsoleto), devuelve todos.
 *
 * Ids viejos de variantes (`…-low`) se mapean al grupo base.
 */
export function filterVisibleModels<T extends ModelWithEfforts>(
  backendId: string,
  models: T[],
): T[] {
  const ids = visibleModelIds(backendId);
  if (!ids) return models;
  const allowed = expandFilterIds(models, ids);
  const filtered = models.filter((m) => allowed.has(m.id));
  if (filtered.length === 0) return models;
  return filtered;
}
