import { describe, expect, it } from "vitest";
import { PILL } from "../pillStage";
import { WHEEL_TOOLS } from "$core/tools";
  import {
  blocksBrowserChrome,
  consoleSideFor,
  contentFor,
  discJoinsTail,
  FLIGHT_SKIP_PX,
  isDiscOnly,
  islandLiveSlots,
  islandStripLong,
  morphsInPlace,
  pivotFor,
  stepWheel,
  targetFor,
  undockForSummon,
  stackMarkVisible,
  wheelChromeActive,
  wheelKeyAction,
  wheelOpenFlight,
} from "./pillPlan";

describe("FLIGHT_SKIP_PX", () => {
  it("omite vuelos menores que ~un disco", () => {
    expect(FLIGHT_SKIP_PX).toBe(48);
  });
});

describe("contentFor", () => {
  it("en reposo mide la barra, nunca menos que el disco", () => {
    expect(contentFor("none", 20)).toEqual({ w: PILL.bar, h: PILL.bar });
    expect(contentFor("none", 180)).toEqual({ w: 180, h: PILL.bar });
  });

  it("la rueda es cuadrada y deja el aire del sobrepaso", () => {
    const side = PILL.wheel - PILL.pad * 2;
    expect(contentFor("wheel", 999)).toEqual({ w: side, h: side });
  });

  it("grabando, la rueda deja alto para la gota colgada", () => {
    const idle = contentFor("wheel", 999);
    const rec = contentFor("wheel", 999, null, "recording");
    expect(rec.w).toBe(idle.w);
    expect(rec.h).toBe(idle.h + PILL.wheelLiveHang);
    expect(contentFor("wheel", 999, null, "dictating").h).toBe(rec.h);
  });

  it("el destino agrega el respiro de los dos lados", () => {
    expect(targetFor("none", PILL.bar)).toEqual({
      w: PILL.bar + PILL.pad * 2,
      h: PILL.bar + PILL.pad * 2,
    });
  });

  it("acoplada en reposo es una pestaña, fina contra su borde", () => {
    // Izquierda/derecha aplastan en x; arriba/abajo en y.
    expect(contentFor("edge", 180, { edge: "left", expanded: false })).toEqual({
      w: PILL.islandThick,
      h: PILL.islandLong,
    });
    expect(contentFor("edge", 180, { edge: "bottom", expanded: false })).toEqual({
      w: PILL.islandLong,
      h: PILL.islandThick,
    });
  });

  it("acoplada y abierta es la tira de herramientas, a lo largo del borde", () => {
    // Acoplada la pill deja de ser un indicador y pasa a ser el acceso. Y se
    // despliega a lo largo del canto, que es donde crecer no tapa pantalla.
    const long = islandStripLong(WHEEL_TOOLS.length);
    expect(contentFor("edge", 180, { edge: "left", expanded: true })).toEqual({
      w: PILL.islandTool,
      h: long,
    });
    expect(contentFor("edge", 180, { edge: "bottom", expanded: true })).toEqual({
      w: long,
      h: PILL.islandTool,
    });
  });

  it("la tira encoge cuando se esconden herramientas de la pill", () => {
    const dock = { edge: "left" as const, expanded: true };
    const todas = contentFor("edge", 180, dock, "idle");
    const tres = contentFor("edge", 180, dock, "idle", 3);
    expect(tres.h).toBe(islandStripLong(3));
    expect(tres.h).toBeLessThan(todas.h);
  });

  it("sin decir cuántas, mide el catálogo entero", () => {
    const dock = { edge: "bottom" as const, expanded: true };
    expect(contentFor("edge", 180, dock, "idle").w).toBe(
      islandStripLong(WHEEL_TOOLS.length),
    );
  });

  it("la tira mide los botones más los huecos entre ellos", () => {
    expect(islandStripLong(1)).toBe(PILL.islandTool);
    expect(islandStripLong(3)).toBe(PILL.islandTool * 3 + PILL.islandGap * 2);
    // Sin herramientas no colapsa a cero: quedaría una isla invisible.
    expect(islandStripLong(0)).toBe(PILL.bar);
  });

  it("el ancho de la barra no influye en la isla", () => {
    // La barra mide del DOM y crece con el timer; la tira no.
    expect(contentFor("edge", 999, { edge: "left", expanded: true })).toEqual(
      contentFor("edge", 40, { edge: "left", expanded: true }),
    );
  });

  it("sin dock, `edge` no puede decidir nada y cae a la barra", () => {
    expect(contentFor("edge", 180)).toEqual(contentFor("none", 180));
  });

  it("desacoplar para summon restaura la barra, no la pestaña", () => {
    const docked = { surface: "edge" as const, dock: { edge: "top" as const, expanded: false } };
    const next = undockForSummon(docked);
    expect(next).toEqual({ surface: "none", dock: null });
    expect(contentFor(next.surface, 180, next.dock).h).toBe(PILL.bar);
    expect(contentFor(docked.surface, 180, docked.dock).h).toBe(PILL.islandThick);
  });

  it("summon ya flotando no toca el estado", () => {
    expect(undockForSummon({ surface: "none", dock: null })).toEqual({
      surface: "none",
      dock: null,
    });
    expect(undockForSummon({ surface: "wheel", dock: null })).toEqual({
      surface: "wheel",
      dock: null,
    });
  });

  it("grabando, la tira no roba un slot: la gota cuelga del cuerpo", () => {
    expect(islandLiveSlots("idle")).toBe(0);
    expect(islandLiveSlots("recording")).toBe(0);
    expect(islandLiveSlots("dictating")).toBe(0);
    const idle = contentFor("edge", 180, { edge: "bottom", expanded: true });
    const rec = contentFor("edge", 180, { edge: "bottom", expanded: true }, "recording");
    expect(rec.w).toBe(idle.w);
    expect(rec.h).toBe(idle.h + PILL.recDrop + PILL.recDropGap);
    const shutRec = contentFor(
      "edge",
      180,
      { edge: "bottom", expanded: false },
      "recording",
    );
    const shutIdle = contentFor("edge", 180, { edge: "bottom", expanded: false });
    expect(shutRec.h).toBe(shutIdle.h + PILL.recDrop + PILL.recDropGap);
    expect(shutRec.w).toBe(shutIdle.w);
  });

  /**
   * La invariante que impide el bucle abrir/cerrar: la isla se abre con el
   * puntero encima, así que la caja abierta no puede ser más chica en ningún
   * eje que la cerrada. Si lo fuera, un cursor en el extremo quedaría afuera
   * al abrirse y el ciclo se realimentaría a 60 Hz.
   */
  it("abrir la isla nunca encoge la caja en ningún eje", () => {
    expect(PILL.islandLong).toBe(PILL.bar);
    for (const activity of ["idle", "recording", "dictating"] as const) {
      for (const edge of ["left", "right", "top", "bottom"] as const) {
        const shut = contentFor("edge", 180, { edge, expanded: false }, activity);
        const open = contentFor("edge", 180, { edge, expanded: true }, activity);
        expect(open.w).toBeGreaterThanOrEqual(shut.w);
        expect(open.h).toBeGreaterThanOrEqual(shut.h);
      }
    }
  });
});

describe("pivotFor", () => {
  const base = { collapsingFrom: null } as const;

  it("la rueda morflea desde el centro (el vuelo al cursor es aparte)", () => {
    // pivotFor solo el morph de tamaño; el flyTo al cursor vive en PillSurface.
    expect(pivotFor({ ...base, surface: "wheel" })).toBe("center");
    expect(pivotFor({ ...base, surface: "none", collapsingFrom: "wheel" })).toBe(
      "center",
    );
  });

  it("en reposo nunca pivotea al centro", () => {
    // Con `center`, cada tic del cronómetro corría la pill media diferencia.
    expect(pivotFor({ ...base, surface: "none" })).toBe("topLeft");
  });

  it("acoplada clava el lado pegado al canto", () => {
    // Es lo que hace que crezca HACIA ADENTRO: con `topLeft`, abrir la isla
    // de la derecha la empujaría fuera de la pantalla.
    const dock = (edge: "left" | "right" | "top" | "bottom") => ({
      ...base,
      surface: "edge" as const,
      dock: { edge, expanded: false },
    });
    expect(pivotFor(dock("left"))).toBe("dockLeft");
    expect(pivotFor(dock("right"))).toBe("dockRight");
    expect(pivotFor(dock("top"))).toBe("dockTop");
    expect(pivotFor(dock("bottom"))).toBe("dockBottom");
  });

  it("la rueda manda sobre el acople: sale del canto a volar", () => {
    expect(
      pivotFor({
        ...base,
        surface: "wheel",
        dock: { edge: "right", expanded: true },
      }),
    ).toBe("center");
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

describe("wheelOpenFlight", () => {
  const area = { x: 0, y: 0, w: 1000, h: 800 };
  const wheel = { w: 252, h: 252 };
  const pill = { x: 0, y: 0, w: 48, h: 48 };

  it("con el clic sobre la pill, encaja la rueda en el monitor", () => {
    const dest = wheelOpenFlight({
      cursor: { x: 24, y: 24 },
      pill,
      wheel,
      areas: [area],
      skipIfNear: FLIGHT_SKIP_PX,
    });
    expect(dest).toEqual({ x: 102, y: 102 });
  });

  it("con el atajo lejos, centra en el cursor si cabe", () => {
    const dest = wheelOpenFlight({
      cursor: { x: 500, y: 400 },
      pill,
      wheel,
      areas: [area],
      skipIfNear: FLIGHT_SKIP_PX,
    });
    expect(dest).toEqual({ x: 500 - 24, y: 400 - 24 });
  });

  it("con el atajo pegado al borde, recorre la rueda hacia adentro", () => {
    const dest = wheelOpenFlight({
      cursor: { x: 10, y: 10 },
      pill: { x: 400, y: 300, w: 48, h: 48 },
      wheel,
      areas: [area],
      skipIfNear: FLIGHT_SKIP_PX,
    });
    expect(dest).toEqual({ x: 102, y: 102 });
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
