/**
 * Agrupar por día lo que se lista por fecha.
 *
 * Las listas de la app son cronológicas y largas, y una fecha suelta por fila
 * obliga a leerlas todas para ubicarse. El encabezado de día da ese salto de
 * un vistazo, y por eso la etiqueta es relativa mientras lo relativo sirve
 * —hoy, ayer, el día de la semana— y absoluta en cuanto deja de servir.
 *
 * Agrupa por tramos consecutivos y no por clave: el orden lo decidió el store
 * y reordenarlo acá cambiaría lo que el usuario ve arriba de todo.
 */
import { formatLocale, intlTag } from "./format";
import { translate } from "./i18n/translate";

export type DayGroup<T> = {
  /** Único dentro del resultado. Es lo que va como key en un `{#each}`. */
  key: string;
  /** El día en hora local, `2026-08-28`. Estable aunque cambie el idioma. */
  day: string;
  label: string;
  items: T[];
};

function midnight(value: Date): number {
  return new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime();
}

export function dayKey(epochSecs: number): string {
  const value = new Date(epochSecs * 1000);
  if (Number.isNaN(value.getTime())) return "";
  const month = `${value.getMonth() + 1}`.padStart(2, "0");
  const day = `${value.getDate()}`.padStart(2, "0");
  return `${value.getFullYear()}-${month}-${day}`;
}

export function dayLabel(epochSecs: number, now: Date = new Date()): string {
  const value = new Date(epochSecs * 1000);
  if (Number.isNaN(value.getTime())) return "";
  const locale = formatLocale();
  const dayDiff = Math.round((midnight(now) - midnight(value)) / 86_400_000);

  if (dayDiff === 0) return translate(locale, "format.dayToday");
  if (dayDiff === 1) return translate(locale, "format.dayYesterday");
  if (dayDiff > 1 && dayDiff < 7) {
    // Dentro de la semana el nombre del día ubica mejor que el número.
    const weekday = new Intl.DateTimeFormat(intlTag(), { weekday: "long" }).format(
      value,
    );
    return weekday.charAt(0).toUpperCase() + weekday.slice(1);
  }

  // El año solo cuando no es el corriente: repetirlo es ruido en la mayoría.
  const sameYear = value.getFullYear() === now.getFullYear();
  return new Intl.DateTimeFormat(intlTag(), {
    day: "numeric",
    month: "long",
    ...(sameYear ? {} : { year: "numeric" as const }),
  }).format(value);
}

export function groupByDay<T>(
  items: readonly T[],
  epochSecsOf: (item: T) => number,
): DayGroup<T>[] {
  const groups: DayGroup<T>[] = [];
  let current: DayGroup<T> | null = null;

  for (const item of items) {
    const secs = epochSecsOf(item);
    const day = dayKey(secs);
    if (!current || current.day !== day) {
      // El índice va en la key porque una lista desordenada puede volver a un
      // día ya visto, y dos keys iguales en un `{#each}` es un error de Svelte.
      current = {
        key: `${day}#${groups.length}`,
        day,
        label: dayLabel(secs),
        items: [],
      };
      groups.push(current);
    }
    current.items.push(item);
  }

  return groups;
}
