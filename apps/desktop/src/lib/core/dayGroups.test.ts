import { describe, expect, it } from "vitest";
import { dayKey, dayLabel, groupByDay } from "./dayGroups";
import { setFormatLocale } from "./format";

/** Un jueves al mediodía: lejos de los bordes de día y de mes. */
const NOW = new Date(2026, 7, 27, 12, 0, 0);

function secs(date: Date): number {
  return Math.floor(date.getTime() / 1000);
}

function daysBefore(days: number, hour = 10): Date {
  return new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate() - days, hour);
}

describe("dayKey", () => {
  it("es el día local, con ceros a la izquierda", () => {
    expect(dayKey(secs(new Date(2026, 0, 5, 23, 30)))).toBe("2026-01-05");
  });

  it("no inventa una fecha para una entrada inválida", () => {
    expect(dayKey(Number.NaN)).toBe("");
  });

  it("separa dos horas del mismo día del día siguiente", () => {
    expect(dayKey(secs(new Date(2026, 7, 27, 0, 1)))).toBe(
      dayKey(secs(new Date(2026, 7, 27, 23, 59))),
    );
    expect(dayKey(secs(new Date(2026, 7, 28, 0, 1)))).not.toBe(
      dayKey(secs(new Date(2026, 7, 27, 23, 59))),
    );
  });
});

describe("dayLabel", () => {
  it("usa palabras mientras lo relativo sirve", () => {
    setFormatLocale("es");
    expect(dayLabel(secs(daysBefore(0)), NOW)).toBe("Hoy");
    expect(dayLabel(secs(daysBefore(1)), NOW)).toBe("Ayer");
  });

  it("sigue el idioma de la interfaz", () => {
    setFormatLocale("en");
    expect(dayLabel(secs(daysBefore(0)), NOW)).toBe("Today");
    expect(dayLabel(secs(daysBefore(1)), NOW)).toBe("Yesterday");
    setFormatLocale("es");
  });

  it("dentro de la semana nombra el día, capitalizado", () => {
    setFormatLocale("es");
    // Tres días antes de un jueves es lunes.
    expect(dayLabel(secs(daysBefore(3)), NOW)).toBe("Lunes");
  });

  it("más atrás pasa a fecha, y agrega el año solo si es otro", () => {
    setFormatLocale("es");
    const older = dayLabel(secs(daysBefore(20)), NOW);
    expect(older).toContain("agosto");
    expect(older).not.toContain("2026");
    expect(dayLabel(secs(new Date(2025, 7, 7, 10)), NOW)).toContain("2025");
  });
});

/**
 * `groupByDay` etiqueta contra el reloj real, así que estas fixtures cuelgan
 * de hoy y no de `NOW`: fijar la fecha acá haría fallar el test cada día.
 */
function agoFromToday(days: number, hour = 10): number {
  const today = new Date();
  return secs(
    new Date(today.getFullYear(), today.getMonth(), today.getDate() - days, hour),
  );
}

describe("groupByDay", () => {
  it("corta por tramos consecutivos y respeta el orden recibido", () => {
    setFormatLocale("es");
    const items = [
      { id: "a", at: agoFromToday(0, 18) },
      { id: "b", at: agoFromToday(0, 9) },
      { id: "c", at: agoFromToday(1, 17) },
      { id: "d", at: agoFromToday(3) },
    ];
    const groups = groupByDay(items, (item) => item.at);

    expect(groups).toHaveLength(3);
    expect(groups[0].label).toBe("Hoy");
    expect(groups[1].label).toBe("Ayer");
    expect(groups[0].items.map((item) => item.id)).toEqual(["a", "b"]);
    expect(groups[1].items.map((item) => item.id)).toEqual(["c"]);
    expect(groups[2].items.map((item) => item.id)).toEqual(["d"]);
  });

  it("no funde dos tramos separados del mismo día, y les da keys distintas", () => {
    const items = [
      { at: agoFromToday(0, 18) },
      { at: agoFromToday(5) },
      { at: agoFromToday(0, 9) },
    ];
    const groups = groupByDay(items, (item) => item.at);

    expect(groups).toHaveLength(3);
    expect(new Set(groups.map((group) => group.key)).size).toBe(3);
  });

  it("de una lista vacía no salen grupos", () => {
    expect(groupByDay([], () => 0)).toEqual([]);
  });
});
