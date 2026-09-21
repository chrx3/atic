import { describe, expect, it } from "vitest";
import { barWidth, formatBytes, formatPercent } from "./systemFormat";

describe("systemFormat", () => {
  it("formatea bytes en unidades legibles", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2 KB");
    expect(formatBytes(1_572_864)).toBe("1.5 MB");
  });

  it("acota el porcentaje", () => {
    expect(formatPercent(12.4)).toBe("12%");
    expect(formatPercent(140)).toBe("100%");
    expect(formatPercent(-4)).toBe("0%");
  });

  it("el ancho de barra no se sale", () => {
    expect(barWidth(50, 100)).toBe("50.0%");
    expect(barWidth(200, 100)).toBe("100.0%");
    expect(barWidth(10, 0)).toBe("0%");
  });
});
