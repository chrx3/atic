import { describe, expect, it } from "vitest";
import type { AgentQuota, QuotaOverview } from "$core/types";
import { quotaRows, spanFrom, toneFor, windowLabel } from "./pillQuota";

const NOW = 1_788_000_000_000;

function quota(partial: Partial<AgentQuota> & { agent: string }): AgentQuota {
  return {
    plan: null,
    windows: [],
    spend: null,
    fetchedAt: NOW,
    error: null,
    ...partial,
  };
}

function overview(agents: AgentQuota[]): QuotaOverview {
  return { agents, fetchedAt: NOW };
}

describe("windowLabel", () => {
  it("prefiere el id del proveedor sobre los minutos", () => {
    // 7dOpus y 7d duran lo mismo; solo el id sabe cuál es cuál.
    expect(windowLabel("7dOpus", 10_080)).toBe("opus");
    expect(windowLabel("7d", 10_080)).toBe("week");
  });

  it("cae a los minutos cuando el id no dice nada", () => {
    // Codex llama a sus ventanas primary/secondary, que no significan nada.
    expect(windowLabel("primary", 300)).toBe("5h");
    expect(windowLabel("secondary", 10_080)).toBe("week");
  });

  it("un largo desconocido queda como custom y no como una etiqueta falsa", () => {
    expect(windowLabel("primary", 720)).toBe("custom");
    expect(windowLabel("primary", null)).toBe("custom");
  });
});

describe("toneFor", () => {
  it("avisa a los 60 y grita a los 85", () => {
    expect(toneFor(0)).toBe("ok");
    expect(toneFor(59.9)).toBe("ok");
    expect(toneFor(60)).toBe("warn");
    expect(toneFor(84.9)).toBe("warn");
    expect(toneFor(85)).toBe("hot");
    expect(toneFor(100)).toBe("hot");
  });
});

describe("spanFrom", () => {
  it("usa minutos hasta que el número empieza a estorbar", () => {
    expect(spanFrom(0)).toEqual({ value: 0, unit: "min" });
    expect(spanFrom(12 * 60_000)).toEqual({ value: 12, unit: "min" });
    expect(spanFrom(89 * 60_000)).toEqual({ value: 89, unit: "min" });
  });

  it("pasa a horas y después a días", () => {
    expect(spanFrom(90 * 60_000)).toEqual({ value: 2, unit: "h" });
    expect(spanFrom(5 * 3_600_000)).toEqual({ value: 5, unit: "h" });
    expect(spanFrom(35 * 3_600_000)).toEqual({ value: 35, unit: "h" });
    expect(spanFrom(36 * 3_600_000)).toEqual({ value: 2, unit: "d" });
    expect(spanFrom(7 * 24 * 3_600_000)).toEqual({ value: 7, unit: "d" });
  });

  it("un lapso ya vencido es cero y no un negativo", () => {
    expect(spanFrom(-5_000)).toEqual({ value: 0, unit: "min" });
  });
});

describe("quotaRows", () => {
  it("sin snapshot no hay filas", () => {
    expect(quotaRows(null, NOW)).toEqual([]);
  });

  it("mantiene el orden fijo y no el del snapshot", () => {
    const rows = quotaRows(
      overview([
        quota({ agent: "cursor-agent", spend: { cents: 100, periodEnd: null } }),
        quota({ agent: "codex" }),
        quota({ agent: "claude" }),
      ]),
      NOW,
    );
    expect(rows.map((r) => r.agent)).toEqual(["claude", "codex", "cursor-agent"]);
  });

  it("un agente que no llega en el snapshot no ocupa fila", () => {
    const rows = quotaRows(overview([quota({ agent: "claude" })]), NOW);
    expect(rows).toHaveLength(1);
  });

  it("un agente con error sí ocupa fila, con su motivo", () => {
    const rows = quotaRows(
      overview([quota({ agent: "codex", error: "sin sesiones", fetchedAt: null })]),
      NOW,
    );
    expect(rows[0].error).toBe("sin sesiones");
    expect(rows[0].bars).toEqual([]);
    expect(rows[0].staleAt).toBeNull();
  });

  it("recorta porcentajes fuera de rango en vez de dibujar barras rotas", () => {
    const rows = quotaRows(
      overview([
        quota({
          agent: "claude",
          windows: [
            { kind: "5h", minutes: 300, usedPercent: 140, resetsAt: null },
            { kind: "7d", minutes: 10_080, usedPercent: -3, resetsAt: null },
          ],
        }),
      ]),
      NOW,
    );
    expect(rows[0].bars.map((b) => b.percent)).toEqual([100, 0]);
    expect(rows[0].bars[0].tone).toBe("hot");
  });

  it("no dibuja una ventana cuyo reinicio ya pasó", () => {
    // El caso real de Codex leído del disco: dos días sin usarlo, la ventana
    // de 5 h ya dio la vuelta y su porcentaje es del ciclo anterior.
    const rows = quotaRows(
      overview([
        quota({
          agent: "codex",
          fetchedAt: NOW - 2 * 24 * 3_600_000,
          windows: [
            {
              kind: "primary",
              minutes: 300,
              usedPercent: 14,
              resetsAt: NOW - 3_600_000,
            },
            {
              kind: "secondary",
              minutes: 10_080,
              usedPercent: 13,
              resetsAt: NOW + 3 * 24 * 3_600_000,
            },
          ],
        }),
      ]),
      NOW,
    );
    expect(rows[0].bars).toHaveLength(1);
    expect(rows[0].bars[0].window).toBe("week");
  });

  it("una ventana sin fecha de reinicio se dibuja igual", () => {
    const rows = quotaRows(
      overview([
        quota({
          agent: "claude",
          windows: [{ kind: "5h", minutes: 300, usedPercent: 20, resetsAt: null }],
        }),
      ]),
      NOW,
    );
    expect(rows[0].bars).toHaveLength(1);
  });

  it("marca viejo el dato de disco y no el recién consultado", () => {
    const rows = quotaRows(
      overview([
        quota({ agent: "claude", fetchedAt: NOW - 60_000 }),
        quota({ agent: "codex", fetchedAt: NOW - 6 * 60 * 60 * 1000 }),
      ]),
      NOW,
    );
    expect(rows[0].staleAt).toBeNull();
    expect(rows[1].staleAt).toBe(NOW - 6 * 60 * 60 * 1000);
  });

  it("Cursor llega con consumo y sin barras", () => {
    const rows = quotaRows(
      overview([
        quota({
          agent: "cursor-agent",
          plan: "pro_plus",
          spend: { cents: 121_312, periodEnd: NOW + 86_400_000 },
        }),
      ]),
      NOW,
    );
    expect(rows[0].bars).toEqual([]);
    expect(rows[0].spend?.cents).toBe(121_312);
    expect(rows[0].name).toBe("Cursor");
  });

  it("un agente desconocido va al final en vez de desaparecer", () => {
    const rows = quotaRows(
      overview([quota({ agent: "gemini" }), quota({ agent: "claude" })]),
      NOW,
    );
    expect(rows.map((r) => r.agent)).toEqual(["claude", "gemini"]);
    expect(rows[1].name).toBe("gemini");
  });
});
