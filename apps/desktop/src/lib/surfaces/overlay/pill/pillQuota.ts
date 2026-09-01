/**
 * De los cupos crudos a las filas que dibuja el hover de la pill.
 *
 * La decisión vive acá y no en el componente por lo mismo que `pillAgentChip`:
 * lo que puede salir mal son los casos de borde —un proveedor que no publica
 * cupo, un dato leído del disco que es de hace seis horas, una ventana con un
 * largo que no cae en ninguna etiqueta conocida— y eso no se comprueba mirando
 * la pill.
 *
 * Acá no se traduce nada: cada barra sale con un `window` que es un id, y el
 * componente lo pasa por i18n. Mezclar idioma con lógica obligaría a que los
 * tests supieran español.
 */
import type { AgentQuota, QuotaOverview } from "$core/types";

/** Etiqueta de ventana ya decidida. `custom` = usar `minutes`. */
export type WindowLabel =
  | "now"
  | "5h"
  | "week"
  | "month"
  | "opus"
  | "sonnet"
  | "auto"
  | "api"
  | "model"
  | "custom";

/**
 * Tres tonos y no un degradado continuo: el color acá no mide, avisa. Lo que
 * el usuario decide con esto es «¿arranco algo grande o no?», y esa respuesta
 * no cambia entre 41% y 44%.
 */
export type QuotaTone = "ok" | "warn" | "hot";

export type QuotaBar = {
  window: WindowLabel;
  /** Solo se usa con `window === "custom"`. */
  minutes: number | null;
  /**
   * Solo con `window === "model"`: el modelo de esa semanal.
   *
   * La API de Claude suma una ventana por modelo nuevo —Opus, Sonnet, ahora
   * Fable—. Traer el nombre en el dato, y no una etiqueta por modelo, es lo
   * que hace que el proximo aparezca sin tocar nada.
   */
  model: string | null;
  /** 0..=100, ya recortado. */
  percent: number;
  tone: QuotaTone;
  /** Epoch ms, o null si el proveedor no lo dice. */
  resetsAt: number | null;
};

export type QuotaRow = {
  agent: string;
  /** Nombre de marca. No se traduce: «Claude Code» es «Claude Code». */
  name: string;
  bars: QuotaBar[];
  /** Presente solo cuando hay on-demand (Cursor) o el fallback sin cupo. */
  spend: { cents: number; periodEnd: number | null } | null;
  plan: string | null;
  error: string | null;
  /**
   * Epoch ms del dato cuando ya está viejo; `null` si es reciente.
   *
   * Solo Codex puede traerlo: se lee de su rollout en disco, que se escribe
   * cuando tú usas Codex y no cuando Atic pregunta.
   */
  staleAt: number | null;
};

/**
 * Orden de las filas. Fijo, no por porcentaje.
 *
 * Ordenarlas por cuál va más apretado haría que salten de lugar entre un
 * hover y el siguiente, y entonces habría que leer los nombres en vez de
 * apuntar a una posición aprendida. Cursor va última: sus barras (Auto / API)
 * no coinciden con las ventanas de tiempo de los otros tres.
 */
const ORDER = ["claude", "codex", "opencode", "agy", "cursor-agent"] as const;

const NAMES: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  opencode: "OpenCode",
  agy: "Antigravity",
  "cursor-agent": "Cursor",
};

/** Sobre esto la barra avisa; sobre `HOT` grita. */
const WARN_AT = 60;
const HOT_AT = 85;

/** A partir de acá el dato de disco se marca como viejo. */
const STALE_AFTER_MS = 15 * 60 * 1000;

/** Largos de ventana con nombre propio, en minutos. */
const KNOWN_MINUTES: Record<number, WindowLabel> = {
  300: "5h",
  10_080: "week",
  43_200: "month",
};

/** Ids de ventana que ya vienen con nombre desde el proveedor. */
const KNOWN_KINDS: Record<string, WindowLabel> = {
  "5h": "5h",
  "7d": "week",
  "7dOpus": "opus",
  "7dSonnet": "sonnet",
  rolling: "now",
  weekly: "week",
  monthly: "month",
  auto: "auto",
  api: "api",
};

export function toneFor(percent: number): QuotaTone {
  if (percent >= HOT_AT) return "hot";
  if (percent >= WARN_AT) return "warn";
  return "ok";
}

/**
 * Qué dice la etiqueta de una ventana.
 *
 * Primero el id del proveedor, que es más específico (`7dOpus` sabe que es
 * Opus; sus 10080 minutos no). Recién si el id no dice nada se cae a los
 * minutos, que es el caso de Codex: sus ventanas se llaman `primary` y
 * `secondary`, nombres que no significan nada para quien mira.
 */
export function windowLabel(kind: string, minutes: number | null): WindowLabel {
  const known = KNOWN_KINDS[kind];
  if (known) return known;
  if (modelOf(kind)) return "model";
  if (minutes != null && KNOWN_MINUTES[minutes]) return KNOWN_MINUTES[minutes];
  return "custom";
}

/**
 * El modelo de una semanal `7d:<modelo>`, presentable. `null` si no es una.
 *
 * La API manda la clave en minusculas (`fable`); la vista lo pone al lado de
 * «Opus» y «Sonnet», que van con mayuscula.
 */
export function modelOf(kind: string): string | null {
  const raw = kind.startsWith("7d:") ? kind.slice(3).trim() : "";
  if (!raw) return null;
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}

function rowFor(quota: AgentQuota, now: number): QuotaRow {
  const stale =
    quota.fetchedAt != null && now - quota.fetchedAt > STALE_AFTER_MS
      ? quota.fetchedAt
      : null;

  return {
    agent: quota.agent,
    name: NAMES[quota.agent] ?? quota.agent,
    // Una ventana cuyo reinicio ya pasó no se dibuja. No es prolijidad: ese
    // porcentaje es de la vuelta anterior. Pasa con Codex, que se lee del
    // disco: si hace dos días que no lo usas, su ventana de 5 h se reinició
    // sola y el 14% del rollout describe un ciclo que ya no existe. Sacar la
    // barra dice «no sé»; dejarla diría «te queda 86%», que es mentira.
    bars: quota.windows
      .filter((win) => win.resetsAt == null || win.resetsAt > now)
      .map((win) => {
        const percent = Math.min(Math.max(win.usedPercent, 0), 100);
        return {
          window: windowLabel(win.kind, win.minutes),
          minutes: win.minutes,
          model: modelOf(win.kind),
          percent,
          tone: toneFor(percent),
          resetsAt: win.resetsAt,
        };
      }),
    spend: quota.spend
      ? { cents: quota.spend.cents, periodEnd: quota.spend.periodEnd }
      : null,
    plan: quota.plan,
    error: quota.error,
    staleAt: stale,
  };
}

/**
 * Un lapso ya redondeado a la unidad en que se va a leer.
 *
 * Se devuelve número y unidad por separado, y no un texto, porque «2 h» y
 * «2 hrs» son la misma cuenta en dos idiomas: la cuenta se prueba acá, el
 * idioma lo pone la vista.
 */
export type Span = { value: number; unit: "min" | "h" | "d" };

/**
 * Elige la unidad por legibilidad, no por precisión.
 *
 * Para decidir si arrancar algo largo, «90 min» y «1 h» sirven igual, pero
 * «2160 min» no sirve para nada. Los cortes están donde el número empieza a
 * ser incómodo de leer, no donde cambia la unidad.
 */
export function spanFrom(ms: number): Span {
  const minutes = Math.max(0, Math.round(ms / 60_000));
  if (minutes < 90) return { value: minutes, unit: "min" };
  const hours = Math.round(minutes / 60);
  if (hours < 36) return { value: hours, unit: "h" };
  return { value: Math.round(hours / 24), unit: "d" };
}

/**
 * Las filas del panel, en orden fijo.
 *
 * Un agente que no está instalado no llega en el snapshot y no ocupa fila. Uno
 * que está pero falló sí: llega con `error`, y esconderlo haría que «no
 * aparece» signifique dos cosas distintas.
 */
export function quotaRows(
  overview: QuotaOverview | null,
  now = Date.now(),
): QuotaRow[] {
  if (!overview) return [];
  const byAgent = new Map(overview.agents.map((quota) => [quota.agent, quota]));
  const rows: QuotaRow[] = [];
  for (const agent of ORDER) {
    const quota = byAgent.get(agent);
    if (quota) {
      rows.push(rowFor(quota, now));
      byAgent.delete(agent);
    }
  }
  // Un agente que Rust conozca y esta lista no: mejor al final que invisible.
  for (const quota of byAgent.values()) rows.push(rowFor(quota, now));
  return rows;
}
