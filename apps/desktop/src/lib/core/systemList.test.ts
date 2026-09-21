import { describe, expect, it } from "vitest";

import { hasBackground, matchesQuery, visibleApps } from "./systemList";

type SystemApp = {
  id: string;
  name: string;
  pid: number;
  cpu: number;
  ram_bytes: number;
  can_close: boolean;
  can_force: boolean;
  can_focus: boolean;
  background: boolean;
};

function app(over: Partial<SystemApp> & { id: string }): SystemApp {
  return {
    name: over.id,
    pid: 1,
    cpu: 0,
    ram_bytes: 0,
    can_close: true,
    can_force: true,
    can_focus: true,
    background: false,
    ...over,
  };
}

const filas = [
  app({ id: "safari", name: "Safari", cpu: 12, ram_bytes: 900 }),
  app({ id: "node", name: "node", cpu: 80, ram_bytes: 300, background: true }),
  app({ id: "chrome", name: "Google Chrome", cpu: 4, ram_bytes: 4000 }),
];

describe("visibleApps", () => {
  it("esconde el segundo plano por defecto", () => {
    const vistas = visibleApps(filas, { sort: "cpu", query: "", background: false });
    expect(vistas.map((a) => a.id)).toEqual(["safari", "chrome"]);
  });

  it("lo muestra con el interruptor, y ordena por CPU", () => {
    const vistas = visibleApps(filas, { sort: "cpu", query: "", background: true });
    expect(vistas.map((a) => a.id)).toEqual(["node", "safari", "chrome"]);
  });

  it("buscar trae el segundo plano aunque el interruptor esté apagado", () => {
    // Si escribiste "node" es porque lo estás buscando.
    const vistas = visibleApps(filas, {
      sort: "cpu",
      query: "node",
      background: false,
    });
    expect(vistas.map((a) => a.id)).toEqual(["node"]);
  });

  it("ordena por RAM cuando se pide", () => {
    const vistas = visibleApps(filas, { sort: "ram", query: "", background: false });
    expect(vistas.map((a) => a.id)).toEqual(["chrome", "safari"]);
  });
});

describe("matchesQuery", () => {
  it("busca por nombre visible y por clave", () => {
    expect(matchesQuery(filas[2], "chrome")).toBe(true);
    expect(matchesQuery(filas[2], "Google")).toBe(true);
    expect(matchesQuery(filas[2], "firefox")).toBe(false);
  });

  it("ignora acentos y mayúsculas", () => {
    expect(matchesQuery(app({ id: "musica", name: "Música" }), "MUSICA")).toBe(true);
  });
});

describe("hasBackground", () => {
  it("solo hay interruptor si hay algo detrás", () => {
    expect(hasBackground(filas)).toBe(true);
    expect(hasBackground([filas[0]])).toBe(false);
  });
});
