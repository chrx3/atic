import { describe, expect, it } from "vitest";
import { PILL } from "../pillStage";
import {
  blocksBrowserChrome,
  contentFor,
  isDiscOnly,
  morphsInPlace,
  pivotFor,
  stepWheel,
  targetFor,
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

  it("el panel ignora el ancho de la barra", () => {
    const panel = { w: PILL.panelW, h: PILL.bar + PILL.panelH };
    expect(contentFor("clipboard", 20)).toEqual(panel);
    expect(contentFor("snippets", 999)).toEqual(panel);
  });

  it("el destino agrega el respiro de los dos lados", () => {
    expect(targetFor("none", PILL.bar)).toEqual({
      w: PILL.bar + PILL.pad * 2,
      h: PILL.bar + PILL.pad * 2,
    });
  });
});

describe("pivotFor", () => {
  const base = { collapsingFrom: null, panelUp: false } as const;

  it("la rueda crece y se cierra desde su centro", () => {
    expect(pivotFor({ ...base, surface: "wheel" })).toBe("center");
    expect(pivotFor({ ...base, surface: "none", collapsingFrom: "wheel" })).toBe(
      "center",
    );
  });

  it("el panel deja que Rust decida hacia dónde abre", () => {
    expect(pivotFor({ ...base, surface: "clipboard" })).toBe("panel");
    expect(pivotFor({ ...base, surface: "snippets" })).toBe("panel");
  });

  /**
   * El «punto C»: sin distinguir de qué se cierra, el panel colapsaba hacia su
   * propio centro y recién desde ahí volaba al hogar.
   */
  it("al cerrar un panel clava la barra donde está", () => {
    expect(pivotFor({ surface: "none", collapsingFrom: "panel", panelUp: false })).toBe(
      "topLeft",
    );
    expect(pivotFor({ surface: "none", collapsingFrom: "panel", panelUp: true })).toBe(
      "bottomLeft",
    );
  });

  it("en reposo nunca pivotea al centro", () => {
    // Con `center`, cada tic del cronómetro corría la pill media diferencia.
    expect(pivotFor({ ...base, surface: "none" })).toBe("topLeft");
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
      morphsInPlace({ from: size, surface: "none", collapsingFrom: "panel" }),
    ).toBe(false);
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
    expect(isDiscOnly({ ...idle, surface: "clipboard" })).toBe(false);
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
