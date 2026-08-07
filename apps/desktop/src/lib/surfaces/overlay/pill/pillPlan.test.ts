import { describe, expect, it } from "vitest";
import { PILL } from "../pillStage";
import {
  blocksBrowserChrome,
  consoleSideFor,
  contentFor,
  discJoinsTail,
  isDiscOnly,
  morphsInPlace,
  pivotFor,
  stepWheel,
  targetFor,
  stackMarkVisible,
  wheelChromeActive,
  wheelKeyAction,
} from "./pillPlan";

describe("contentFor", () => {
  it("en reposo mide la barra, nunca menos que el disco", () => {
    expect(contentFor("none", 20)).toEqual({ w: PILL.bar, h: PILL.bar });
    expect(contentFor("none", 180)).toEqual({ w: 180, h: PILL.bar });
  });

  it("la rueda es cuadrada y deja el aire del sobrepaso", () => {
    const side = PILL.wheel - PILL.pad * 2;
    expect(contentFor("wheel", 999)).toEqual({ w: side, h: side });
  });

  it("el destino agrega el respiro de los dos lados", () => {
    expect(targetFor("none", PILL.bar)).toEqual({
      w: PILL.bar + PILL.pad * 2,
      h: PILL.bar + PILL.pad * 2,
    });
  });
});

describe("pivotFor", () => {
  const base = { collapsingFrom: null } as const;

  it("la rueda abre y cierra in-situ desde el centro (no cursor)", () => {
    // Open/close morph en el hogar; el summon al cursor es otro camino.
    expect(pivotFor({ ...base, surface: "wheel" })).toBe("center");
    expect(pivotFor({ ...base, surface: "none", collapsingFrom: "wheel" })).toBe(
      "center",
    );
  });

  it("en reposo nunca pivotea al centro", () => {
    // Con `center`, cada tic del cronómetro corría la pill media diferencia.
    expect(pivotFor({ ...base, surface: "none" })).toBe("topLeft");
  });
});

describe("wheelChromeActive", () => {
  it("cubre rueda abierta y colapso en curso", () => {
    expect(wheelChromeActive({ surface: "wheel", collapsingFrom: null })).toBe(true);
    expect(
      wheelChromeActive({ surface: "none", collapsingFrom: "wheel" }),
    ).toBe(true);
  });

  it("en reposo el chrome de la rueda no es la silueta", () => {
    expect(wheelChromeActive({ surface: "none", collapsingFrom: null })).toBe(
      false,
    );
  });
});

describe("stackMarkVisible", () => {
  it("oculta la marca del stack con la rueda abierta o colapsando", () => {
    expect(stackMarkVisible({ surface: "wheel", collapsingFrom: null })).toBe(
      false,
    );
    expect(
      stackMarkVisible({ surface: "none", collapsingFrom: "wheel" }),
    ).toBe(false);
  });

  it("muestra la marca del stack solo en reposo", () => {
    expect(stackMarkVisible({ surface: "none", collapsingFrom: null })).toBe(
      true,
    );
  });
});

describe("morphsInPlace", () => {
  const size = { w: 48, h: 48 };

  it("anima los cambios de la barra compacta", () => {
    expect(morphsInPlace({ from: size, surface: "none", collapsingFrom: null })).toBe(
      true,
    );
  });

  it("no anima el primer reencuadre", () => {
    expect(morphsInPlace({ from: null, surface: "none", collapsingFrom: null })).toBe(
      false,
    );
  });

  it("no anima los colapsos: tienen su propia coreografía", () => {
    expect(
      morphsInPlace({ from: size, surface: "none", collapsingFrom: "wheel" }),
    ).toBe(false);
  });

  it("no anima con algo desplegado", () => {
    expect(morphsInPlace({ from: size, surface: "wheel", collapsingFrom: null })).toBe(
      false,
    );
  });
});

describe("isDiscOnly", () => {
  const idle = {
    surface: "none",
    activity: "idle",
    hasQueue: false,
    agentAlert: false,
  } as const;

  it("en reposo, la barra es solo el disco", () => {
    expect(isDiscOnly(idle)).toBe(true);
  });

  it("cualquier cosa que mostrar la estira", () => {
    expect(isDiscOnly({ ...idle, activity: "recording" })).toBe(false);
    expect(isDiscOnly({ ...idle, activity: "dictating" })).toBe(false);
    expect(isDiscOnly({ ...idle, hasQueue: true })).toBe(false);
    expect(isDiscOnly({ ...idle, agentAlert: true })).toBe(false);
  });

  it("la rueda no cuenta: la barra de abajo sigue siendo el disco", () => {
    expect(isDiscOnly({ ...idle, surface: "wheel" })).toBe(true);
  });
});

describe("stepWheel", () => {
  const tools = [{ id: "a" }, { id: "b" }, { id: "c" }] as const;

  it("sin selección, entra por el extremo que corresponde", () => {
    expect(stepWheel(null, 1, tools)).toBe("a");
    expect(stepWheel(null, -1, tools)).toBe("c");
  });

  it("avanza y retrocede dando la vuelta", () => {
    expect(stepWheel("a", 1, tools)).toBe("b");
    expect(stepWheel("c", 1, tools)).toBe("a");
    expect(stepWheel("a", -1, tools)).toBe("c");
  });
});

describe("wheelKeyAction", () => {
  it("mapea flechas, tabulador y activación", () => {
    expect(wheelKeyAction("ArrowRight", false)).toBe("next");
    expect(wheelKeyAction("ArrowUp", false)).toBe("prev");
    expect(wheelKeyAction("Tab", false)).toBe("next");
    expect(wheelKeyAction("Tab", true)).toBe("prev");
    expect(wheelKeyAction("Enter", false)).toBe("activate");
    expect(wheelKeyAction(" ", false)).toBe("activate");
  });

  it("deja pasar lo que no es de la rueda", () => {
    expect(wheelKeyAction("Escape", false)).toBeNull();
    expect(wheelKeyAction("a", false)).toBeNull();
  });
});

describe("consoleSideFor", () => {
  const area = { x: 0, y: 0, w: 1000, h: 800 };

  it("cerca del borde izquierdo, la consola va a la derecha", () => {
    expect(consoleSideFor([area], { x: 20, y: 100 }, { w: 48, h: 48 })).toBe(
      "right",
    );
  });

  it("cerca del borde derecho, la consola va a la izquierda", () => {
    expect(consoleSideFor([area], { x: 920, y: 100 }, { w: 48, h: 48 })).toBe(
      "left",
    );
  });

  it("sin monitores, por defecto a la derecha", () => {
    expect(consoleSideFor([], { x: 10, y: 10 }, { w: 40, h: 40 })).toBe("right");
  });
});

describe("discJoinsTail", () => {
  it("junto a una gota chica, el disco sigue en el campo", () => {
    expect(discJoinsTail({ w: 40 }, { w: 24 })).toBe(true);
  });

  it("con la pastilla ya expandida, solo queda la gota", () => {
    // Dos formas que comparten el borde izquierdo engordan ese lado.
    expect(discJoinsTail({ w: 40 }, { w: 100 })).toBe(false);
  });

  it("sin una de las dos, no hay fusión que publicar", () => {
    expect(discJoinsTail({ w: 40 }, null)).toBe(false);
    expect(discJoinsTail(null, { w: 24 })).toBe(false);
  });
});

describe("blocksBrowserChrome", () => {
  it("se traga el chrome del WebView", () => {
    // `Ctrl+R` recarga el overlay entero y se lleva la sesión de agentes.
    expect(blocksBrowserChrome({ key: "r", ctrlKey: true, metaKey: false })).toBe(true);
    expect(blocksBrowserChrome({ key: "P", ctrlKey: true, metaKey: false })).toBe(true);
    expect(blocksBrowserChrome({ key: "F12", ctrlKey: false, metaKey: false })).toBe(
      true,
    );
  });

  it("no toca las teclas normales", () => {
    expect(blocksBrowserChrome({ key: "r", ctrlKey: false, metaKey: false })).toBe(
      false,
    );
    expect(blocksBrowserChrome({ key: "Escape", ctrlKey: false, metaKey: false })).toBe(
      false,
    );
    // Copiar y pegar tienen que seguir funcionando dentro del bloc.
    expect(blocksBrowserChrome({ key: "c", ctrlKey: true, metaKey: false })).toBe(
      false,
    );
    expect(blocksBrowserChrome({ key: "v", ctrlKey: true, metaKey: false })).toBe(
      false,
    );
  });
});
