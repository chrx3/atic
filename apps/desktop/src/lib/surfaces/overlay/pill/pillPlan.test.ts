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
  shouldMeasureBar,
  nextBarWidth,
  islandCueLong,
  islandLiveSlots,
  liveHang,
  islandStripLong,
  morphsInPlace,
  bloomPivot,
  pivotFor,
  stepWheel,
  targetFor,
  undockForSummon,
  shouldStayDockedOnActivate,
  shouldRecenterTopNotch,
  shouldReturnToEdgeOnActivate,
  islandHoverStay,
  islandHoverOpens,
  floatWheelHoverWatches,
  floatWheelHoverOpens,
  FLOAT_WHEEL_HOVER_OPEN_MS,
  islandFaceAgent,
  islandFaceClipboard,
  islandFacePanel,
  isIslandPanelFace,
  islandNotchRadius,
  pointerMoveDrags,
  pointerGestureWasClick,
  ISLAND_COLLAPSE_MS,
  stackMarkVisible,
  wheelChromeActive,
  dragClosesWheel,
  wheelKeyAction,
  wheelOpenFlight,
  UPDATE_ISLAND_OPEN_DELAY_MS,
} from "./pillPlan";

describe("FLIGHT_SKIP_PX", () => {
  it("omite vuelos menores que ~un disco", () => {
    expect(FLIGHT_SKIP_PX).toBe(PILL.bar + 8);
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

  it("con aviso de agente, la pestaña cerrada engorda y sigue siendo pestaña", () => {
    expect(PILL.islandCueThick).toBeGreaterThan(PILL.islandThick);
    expect(PILL.islandCueThick).toBeLessThan(PILL.islandTool);
    expect(
      contentFor("edge", 180, { edge: "left", expanded: false }, "idle", 5, true),
    ).toEqual({
      w: PILL.islandCueThick,
      h: islandCueLong(1),
    });
    expect(
      contentFor("edge", 180, { edge: "bottom", expanded: false }, "idle", 5, true),
    ).toEqual({
      w: islandCueLong(1),
      h: PILL.islandCueThick,
    });
    expect(
      contentFor("edge", 180, { edge: "bottom", expanded: false }, "idle", 5, true, 3)
        .w,
    ).toBe(islandCueLong(3));
    // Un logo cabe en el dintel de la isla; no hay que alargar. Varios sí.
    expect(islandCueLong(1)).toBe(PILL.islandLong);
    expect(islandCueLong(1)).toBeGreaterThanOrEqual(
      PILL.islandMark + PILL.islandCueBtn,
    );
    expect(islandCueLong(9)).toBeGreaterThan(islandCueLong(1));
    // Abierta no crece por el aviso: con 5 herramientas la tira ya es más
    // larga que cualquier pestaña, y el aviso vive en el botón de agentes.
    expect(
      contentFor("edge", 180, { edge: "left", expanded: true }, "idle", 5, true),
    ).toEqual(
      contentFor("edge", 180, { edge: "left", expanded: true }, "idle", 5, false),
    );
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

  it("cara agent: tarjeta colgada de la pestaña, solo en canto horizontal", () => {
    const face = "agent" as const;
    // Arriba/abajo: la tarjeta gana a la tira y ensancha a la tarjeta.
    expect(
      contentFor(
        "edge",
        180,
        { edge: "top", expanded: false },
        "idle",
        5,
        false,
        0,
        0,
        face,
      ),
    ).toEqual({ w: PILL.islandCardW, h: PILL.islandThick + PILL.islandCardH });
    expect(
      contentFor(
        "edge",
        180,
        { edge: "bottom", expanded: true },
        "idle",
        5,
        false,
        0,
        0,
        face,
      ),
    ).toEqual({ w: PILL.islandCardW, h: PILL.islandThick + PILL.islandCardH });
    // Con aviso, la pestaña engorda y la tarjeta hereda el ancho si es mayor.
    const cue = contentFor(
      "edge",
      180,
      { edge: "top", expanded: false },
      "idle",
      5,
      true,
      1,
      0,
      face,
    );
    expect(cue.h).toBe(PILL.islandCueThick + PILL.islandCardH);
    expect(cue.w).toBe(Math.max(islandCueLong(1), PILL.islandCardW));
    // En laterales se degrada a pestaña/tira: la tarjeta no cabe.
    const leftDock = { edge: "left" as const, expanded: false };
    expect(contentFor("edge", 180, leftDock, "idle", 5, false, 0, 0, face)).toEqual(
      contentFor("edge", 180, leftDock, "idle", 5),
    );
    const leftOpen = { edge: "left" as const, expanded: true };
    expect(contentFor("edge", 180, leftOpen, "idle", 5, false, 0, 0, face)).toEqual(
      contentFor("edge", 180, leftOpen, "idle", 5),
    );
    // Flotando y en rueda la cara no existe: el permiso ya tiene su tarjeta.
    expect(contentFor("none", 180, null, "idle", 5, false, 0, 0, face)).toEqual(
      contentFor("none", 180),
    );
    expect(contentFor("wheel", 999, null, "idle", 5, false, 0, 0, face)).toEqual(
      contentFor("wheel", 999),
    );
  });

  it("isla alta usa radio de panel, no de pastilla", () => {
    expect(islandNotchRadius({ w: 124, h: 40 })).toBe(20);
    expect(islandNotchRadius({ w: 280, h: 292 })).toBe(PILL.islandClipR);
  });

  it("cara clipboard: lista colgada de la pestaña, en cualquier canto", () => {
    const face = "clipboard" as const;
    expect(
      contentFor(
        "edge",
        180,
        { edge: "top", expanded: false },
        "idle",
        5,
        false,
        0,
        0,
        face,
      ),
    ).toEqual({ w: PILL.islandClipW, h: PILL.islandThick + PILL.islandClipH });
    expect(
      contentFor(
        "edge",
        180,
        { edge: "left", expanded: false },
        "idle",
        5,
        false,
        0,
        0,
        face,
      ),
    ).toEqual({ w: PILL.islandThick + PILL.islandClipW, h: PILL.islandClipH });
  });

  it("textos usan el mismo panel que clipboard; agentes, uno más grande", () => {
    const top = { edge: "top" as const, expanded: false };
    const left = { edge: "left" as const, expanded: false };
    expect(isIslandPanelFace("snippets")).toBe(true);
    expect(contentFor("edge", 180, top, "idle", 5, false, 0, 0, "snippets")).toEqual(
      contentFor("edge", 180, top, "idle", 5, false, 0, 0, "clipboard"),
    );
    expect(contentFor("edge", 180, top, "idle", 5, false, 0, 0, "agents")).toEqual({
      w: PILL.islandAgentsSetupW,
      h: PILL.islandThick + PILL.islandAgentsSetupH,
    });
    expect(contentFor("edge", 180, left, "idle", 5, false, 0, 0, "agents")).toEqual({
      w: PILL.islandThick + PILL.islandAgentsSetupW,
      h: PILL.islandAgentsSetupH,
    });
    expect(
      contentFor("edge", 180, top, "idle", 5, false, 0, 0, "agents", true),
    ).toEqual({ w: PILL.islandAgentsW, h: PILL.islandThick + PILL.islandAgentsH });
  });

  it("islandFaceClipboard: acoplada y pedida, también en laterales", () => {
    const dock = { edge: "top" as const, expanded: false };
    expect(islandFaceClipboard({ surface: "edge", dock, requested: true })).toBe(true);
    expect(islandFaceClipboard({ surface: "edge", dock, requested: false })).toBe(
      false,
    );
    expect(
      islandFaceClipboard({
        surface: "edge",
        dock: { edge: "left", expanded: false },
        requested: true,
      }),
    ).toBe(true);
    expect(islandFaceClipboard({ surface: "none", dock, requested: true })).toBe(false);
    expect(islandFacePanel({ surface: "edge", dock, requested: true })).toBe(true);
  });

  it("islandFaceAgent: auto-abre con pedido nuevo, no re-abre el colapsado", () => {
    const dock = { edge: "top" as const, expanded: false };
    const base = {
      surface: "edge" as const,
      dock,
      authId: "p1",
      dismissedAuthId: null,
    };
    expect(islandFaceAgent(base)).toBe(true);
    expect(islandFaceAgent({ ...base, dismissedAuthId: "p1" })).toBe(false);
    // Un pedido NUEVO sí re-abre aunque se haya colapsado el anterior.
    expect(islandFaceAgent({ ...base, authId: "p2", dismissedAuthId: "p1" })).toBe(
      true,
    );
    // Sin pedido, flotando, sin dock o en lateral: pestaña.
    expect(islandFaceAgent({ ...base, authId: null })).toBe(false);
    expect(islandFaceAgent({ ...base, surface: "none" as const })).toBe(false);
    expect(islandFaceAgent({ ...base, dock: null })).toBe(false);
    expect(
      islandFaceAgent({ ...base, dock: { edge: "left" as const, expanded: false } }),
    ).toBe(false);
  });

  it("desacoplar para summon restaura la barra, no la pestaña", () => {
    const docked = {
      surface: "edge" as const,
      dock: { edge: "top" as const, expanded: false },
    };
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

  it("recentrar el notch solo en isla de techo, en reposo", () => {
    const docked = {
      surface: "edge" as const,
      dock: { edge: "top" as const, expanded: false },
    };
    expect(shouldRecenterTopNotch(docked)).toBe(true);
    expect(shouldRecenterTopNotch({ ...docked, flying: true })).toBe(false);
    expect(shouldRecenterTopNotch({ ...docked, opening: true })).toBe(false);
    expect(shouldRecenterTopNotch({ surface: "wheel", dock: docked.dock })).toBe(false);
    expect(shouldRecenterTopNotch({ surface: "none", dock: docked.dock })).toBe(false);
    expect(
      shouldRecenterTopNotch({
        surface: "edge",
        dock: { edge: "left", expanded: false },
      }),
    ).toBe(false);
  });

  it("activar desde el canto se queda acoplada; el summon no", () => {
    expect(shouldStayDockedOnActivate("edge")).toBe(true);
    expect(shouldStayDockedOnActivate("none")).toBe(false);
    expect(shouldStayDockedOnActivate("wheel")).toBe(false);
    const docked = {
      surface: "edge" as const,
      dock: { edge: "left" as const, expanded: false },
    };
    expect(undockForSummon(docked).surface).toBe("none");
    expect(shouldStayDockedOnActivate(docked.surface)).toBe(true);
  });

  it("abrir una tool desde la rueda o el canto vuelve al hogar", () => {
    const dock = { edge: "top" as const, expanded: false };
    expect(shouldReturnToEdgeOnActivate("edge", dock)).toBe(true);
    expect(shouldReturnToEdgeOnActivate("wheel", dock)).toBe(true);
    expect(shouldReturnToEdgeOnActivate("wheel", null)).toBe(true);
    expect(shouldReturnToEdgeOnActivate("none", null)).toBe(false);
  });

  it("un frame fuera del hit no cierra la isla; el linger sí", () => {
    const stay = islandHoverStay({
      over: false,
      now: 1000,
      leftAt: null,
      lingerMs: ISLAND_COLLAPSE_MS,
    });
    expect(stay.open).toBe(true);
    expect(stay.leftAt).toBe(1000);
    expect(
      islandHoverStay({
        over: false,
        now: 1000 + ISLAND_COLLAPSE_MS - 1,
        leftAt: stay.leftAt,
        lingerMs: ISLAND_COLLAPSE_MS,
      }).open,
    ).toBe(true);
    expect(
      islandHoverStay({
        over: false,
        now: 1000 + ISLAND_COLLAPSE_MS,
        leftAt: stay.leftAt,
        lingerMs: ISLAND_COLLAPSE_MS,
      }).open,
    ).toBe(false);
    expect(
      islandHoverStay({
        over: true,
        now: 2000,
        leftAt: 1000,
        lingerMs: ISLAND_COLLAPSE_MS,
      }),
    ).toEqual({ open: true, leftAt: null });
  });

  it("con aviso de update, la tira espera un toque antes de abrir", () => {
    const closed = {
      over: true,
      expanded: false,
      hasUpdate: true,
      hoveredMs: 0,
    };
    expect(islandHoverOpens(closed)).toBe(false);
    expect(
      islandHoverOpens({ ...closed, hoveredMs: UPDATE_ISLAND_OPEN_DELAY_MS - 1 }),
    ).toBe(false);
    expect(
      islandHoverOpens({ ...closed, hoveredMs: UPDATE_ISLAND_OPEN_DELAY_MS }),
    ).toBe(true);
    expect(islandHoverOpens({ ...closed, hasUpdate: false })).toBe(true);
    expect(islandHoverOpens({ ...closed, expanded: true })).toBe(true);
    expect(islandHoverOpens({ ...closed, over: false })).toBe(false);
  });

  it("el disco flotante mira el hover; el atajo no se cierra al alejar", () => {
    expect(
      floatWheelHoverWatches({
        surface: "none",
        discOnly: true,
        heldByHover: false,
        collapsingFrom: null,
      }),
    ).toBe(true);
    expect(
      floatWheelHoverWatches({
        surface: "none",
        discOnly: false,
        heldByHover: false,
        collapsingFrom: null,
      }),
    ).toBe(false);
    expect(
      floatWheelHoverWatches({
        surface: "edge",
        discOnly: true,
        heldByHover: false,
        collapsingFrom: null,
      }),
    ).toBe(false);
    expect(
      floatWheelHoverWatches({
        surface: "wheel",
        discOnly: true,
        heldByHover: true,
        collapsingFrom: null,
      }),
    ).toBe(true);
    expect(
      floatWheelHoverWatches({
        surface: "wheel",
        discOnly: true,
        heldByHover: false,
        collapsingFrom: null,
      }),
    ).toBe(false);
    expect(
      floatWheelHoverWatches({
        surface: "none",
        discOnly: true,
        heldByHover: true,
        collapsingFrom: "wheel",
      }),
    ).toBe(false);
  });

  it("el disco flotante espera un toque antes de abrir la rueda", () => {
    const closed = { over: true, alreadyOpen: false, hoveredMs: 0 };
    expect(floatWheelHoverOpens(closed)).toBe(false);
    expect(
      floatWheelHoverOpens({
        ...closed,
        hoveredMs: FLOAT_WHEEL_HOVER_OPEN_MS - 1,
      }),
    ).toBe(false);
    expect(
      floatWheelHoverOpens({
        ...closed,
        hoveredMs: FLOAT_WHEEL_HOVER_OPEN_MS,
      }),
    ).toBe(true);
    expect(floatWheelHoverOpens({ over: true, alreadyOpen: true, hoveredMs: 0 })).toBe(
      true,
    );
    expect(
      floatWheelHoverOpens({ over: false, alreadyOpen: true, hoveredMs: 500 }),
    ).toBe(false);
  });

  it("el pointermove del hover sintético no cuenta como arrastre", () => {
    expect(pointerMoveDrags(0)).toBe(false);
    expect(pointerMoveDrags(1)).toBe(true);
  });

  it("el clic se mide contra el down original, no contra un origen re-sembrado", () => {
    const press = { x: 100, y: 100 };
    expect(pointerGestureWasClick(press, { x: 102, y: 101 }, 4)).toBe(true);
    expect(pointerGestureWasClick(press, { x: 110, y: 100 }, 4)).toBe(false);
    expect(pointerGestureWasClick(null, { x: 100, y: 100 }, 4)).toBe(false);
  });

  it("acoplada, la actividad no cuelga: la caja no crece hacia adentro", () => {
    expect(islandLiveSlots("idle")).toBe(0);
    expect(islandLiveSlots("recording")).toBe(0);
    expect(islandLiveSlots("dictating")).toBe(0);
    // La gota que colgaba cambiaba la silueta con el estado. Ahora el estado
    // vive en la cara de la marca y el stop es un chip: la caja es la misma.
    for (const edge of ["top", "bottom", "left", "right"] as const) {
      for (const expanded of [false, true]) {
        const dock = { edge, expanded };
        const idle = contentFor("edge", 180, dock, "idle", 5);
        expect(contentFor("edge", 180, dock, "recording", 5)).toEqual(idle);
        expect(contentFor("edge", 180, dock, "dictating", 5)).toEqual(idle);
      }
    }
  });

  it("la rueda sí cuelga: es su propio escenario, no rompe continuidad", () => {
    expect(liveHang("recording", "wheel")).toBe(PILL.recDrop + PILL.recDropGap);
    expect(liveHang("dictating", "wheel")).toBe(PILL.recDrop + PILL.recDropGap);
    expect(liveHang("idle", "wheel")).toBe(0);
    expect(liveHang("recording", "edge")).toBe(0);
    expect(liveHang("recording", "none")).toBe(0);
    const idle = contentFor("wheel", 999);
    expect(contentFor("wheel", 999, null, "recording").h).toBe(
      idle.h + PILL.wheelLiveHang,
    );
  });

  it("el aviso de update alarga la pestaña, no la hace colgar", () => {
    const dockShut = { edge: "bottom" as const, expanded: false };
    // Cuenta como una marca más: entra a `islandCueCount`, no a un hang.
    // Uno o dos logos caben en el dintel; con varios la isla se alarga
    // a lo largo del borde y el grosor no cambia.
    const unaMarca = contentFor("edge", 180, dockShut, "idle", 5, true, 1);
    const muchas = contentFor("edge", 180, dockShut, "idle", 5, true, 4);
    expect(muchas.h).toBe(unaMarca.h);
    expect(muchas.w).toBeGreaterThan(unaMarca.w);
    expect(muchas.w).toBe(islandCueLong(4));
  });

  /**
   * La invariante que impide el bucle abrir/cerrar: la isla se abre con el
   * puntero encima, así que la caja abierta no puede ser más chica en ningún
   * eje que la cerrada. Si lo fuera, un cursor en el extremo quedaría afuera
   * al abrirse y el ciclo se realimentaría a 60 Hz.
   */
  it("abrir la isla nunca encoge la caja en ningún eje", () => {
    expect(PILL.islandLong).toBeGreaterThan(PILL.islandThick);
    // `toolCount` barre hasta 1: las herramientas se esconden desde Ajustes, y
    // con pocas la tira abierta puede quedar más corta que una pestaña con
    // avisos. Ahí es donde el piso de `contentFor` hace falta.
    for (const toolCount of [1, 2, 3, WHEEL_TOOLS.length]) {
      for (const cue of [false, true]) {
        for (const cueCount of [0, 1, 3]) {
          for (const activity of ["idle", "recording", "dictating"] as const) {
            for (const edge of ["left", "right", "top", "bottom"] as const) {
              const shut = contentFor(
                "edge",
                180,
                { edge, expanded: false },
                activity,
                toolCount,
                cue,
                cueCount,
              );
              const open = contentFor(
                "edge",
                180,
                { edge, expanded: true },
                activity,
                toolCount,
                cue,
                cueCount,
              );
              expect(open.w).toBeGreaterThanOrEqual(shut.w);
              expect(open.h).toBeGreaterThanOrEqual(shut.h);
            }
          }
        }
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

describe("bloomPivot", () => {
  it("sin canto la rueda nace del centro", () => {
    expect(bloomPivot(null)).toBe("center");
    expect(bloomPivot(undefined)).toBe("center");
  });

  it("desde la isla crece hacia adentro, clavada al muro", () => {
    expect(bloomPivot("top")).toBe("dockTop");
    expect(bloomPivot("bottom")).toBe("dockBottom");
    expect(bloomPivot("left")).toBe("dockLeft");
    expect(bloomPivot("right")).toBe("dockRight");
  });
});

describe("wheelChromeActive", () => {
  it("cubre rueda abierta y colapso en curso", () => {
    expect(wheelChromeActive({ surface: "wheel", collapsingFrom: null })).toBe(true);
    expect(wheelChromeActive({ surface: "none", collapsingFrom: "wheel" })).toBe(true);
  });

  it("en reposo el chrome de la rueda no es la silueta", () => {
    expect(wheelChromeActive({ surface: "none", collapsingFrom: null })).toBe(false);
  });
});

describe("dragClosesWheel", () => {
  it("solo la rueda abierta se cierra al arrastrar", () => {
    expect(dragClosesWheel("wheel")).toBe(true);
    expect(dragClosesWheel("none")).toBe(false);
    expect(dragClosesWheel("edge")).toBe(false);
  });
});

describe("stackMarkVisible", () => {
  it("oculta la marca del stack con la rueda abierta o colapsando", () => {
    expect(stackMarkVisible({ surface: "wheel", collapsingFrom: null })).toBe(false);
    expect(stackMarkVisible({ surface: "none", collapsingFrom: "wheel" })).toBe(false);
  });

  it("muestra la marca del stack solo en reposo", () => {
    expect(stackMarkVisible({ surface: "none", collapsingFrom: null })).toBe(true);
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
    expect(isDiscOnly({ ...idle, hasUpdate: true })).toBe(false);
  });

  it("la rueda no cuenta: la barra de abajo sigue siendo el disco", () => {
    expect(isDiscOnly({ ...idle, surface: "wheel" })).toBe(true);
  });
});

describe("shouldMeasureBar", () => {
  it("en la barra flotante, en reposo", () => {
    expect(shouldMeasureBar("none", false)).toBe(true);
  });

  it("no durante un arrastre: el timer pelearía con el gesto", () => {
    expect(shouldMeasureBar("none", true)).toBe(false);
  });

  it("acoplada o en rueda no mide la barra compacta", () => {
    expect(shouldMeasureBar("edge", false)).toBe(false);
    expect(shouldMeasureBar("wheel", false)).toBe(false);
  });
});

describe("nextBarWidth", () => {
  it("ignora el ruido de 1 px", () => {
    expect(nextBarWidth(180, 180.4, true)).toBe(180);
    expect(nextBarWidth(180, 181, true)).toBe(180);
  });

  it("grabando no encoge con las ondas", () => {
    expect(nextBarWidth(200, 160, true)).toBe(200);
  });

  it("en idle sí encoge al contenido", () => {
    expect(nextBarWidth(200, 52, false)).toBe(PILL.bar);
  });

  it("crece cuando el timer o un chip lo piden", () => {
    expect(nextBarWidth(180, 210, true)).toBe(210);
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
    expect(consoleSideFor([area], { x: 20, y: 100 }, { w: 48, h: 48 })).toBe("right");
  });

  it("cerca del borde derecho, la consola va a la izquierda", () => {
    expect(consoleSideFor([area], { x: 920, y: 100 }, { w: 48, h: 48 })).toBe("left");
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

  it("sin cursor (clic en la pill) no vuela: encaja donde está", () => {
    const dest = wheelOpenFlight({
      cursor: null,
      pill: { x: 400, y: 300, w: 48, h: 48 },
      wheel,
      areas: [area],
      skipIfNear: FLIGHT_SKIP_PX,
    });
    expect(dest).toEqual({ x: 400, y: 300 });
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
