/**
 * Los cupos de agentes, puestos en palabras.
 *
 * Lo comparten el vistazo flotante (`PillPeekHost`) y el que vive dentro de la
 * isla (`AgentsPeek`): dos copias de estas cuentas terminarían diciendo lo
 * mismo con palabras distintas. La decisión de qué barras hay sigue en
 * `pillQuota`, que no traduce.
 */
import { t } from "$domain/i18n.svelte";
import { spanFrom, type QuotaBar, type QuotaRow, type QuotaTone } from "./pillQuota";

export function spanText(ms: number): string {
  const span = spanFrom(ms);
  return `${span.value} ${t(`pill.quota.unit.${span.unit}`)}`;
}

export function windowText(bar: QuotaBar): string {
  if (bar.window === "model") {
    return t("pill.quota.window.modelWeek", { model: bar.model ?? "" });
  }
  if (bar.window !== "custom") return t(`pill.quota.window.${bar.window}`);
  if (bar.minutes == null) return t("pill.quota.window.unknown");
  return spanText(bar.minutes * 60_000);
}

/** Plan tal como lo guarda el proveedor (`max 20x`, `pro_plus`, `plus`). */
export function planText(plan: string | null): string {
  return plan ? plan.replace(/_/g, " ") : "";
}

/** Centavos → «1.213» con la separación de miles del idioma activo. */
function moneyText(cents: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(
    cents / 100,
  );
}

export function spendText(row: QuotaRow, now: number): string {
  if (!row.spend) return "";
  const amount = t("pill.quota.spend", { amount: moneyText(row.spend.cents) });
  if (row.spend.periodEnd == null || row.spend.periodEnd <= now) return amount;
  return `${amount} · ${t("pill.quota.periodEnds", {
    when: spanText(row.spend.periodEnd - now),
  })}`;
}

/** La ventana más apretada del agente: el anillo resume eso. */
export function headline(row: QuotaRow): { percent: number; tone: QuotaTone } {
  const first = row.bars[0];
  if (!first) return { percent: 0, tone: "ok" };
  return row.bars.reduce(
    (best, bar) =>
      bar.percent > best.percent ? { percent: bar.percent, tone: bar.tone } : best,
    { percent: first.percent, tone: first.tone },
  );
}

/** Sin hover, el agente más apretado; con hover, el apuntado. */
export function detailRowFor(rows: QuotaRow[], hover: string | null): QuotaRow | null {
  const pointed = hover ? rows.find((r) => r.agent === hover) : null;
  if (pointed) return pointed;
  const withBars = rows.filter((row) => row.bars.length > 0);
  if (withBars.length === 0) return rows[0] ?? null;
  return withBars.reduce((a, b) => (headline(b).percent > headline(a).percent ? b : a));
}
