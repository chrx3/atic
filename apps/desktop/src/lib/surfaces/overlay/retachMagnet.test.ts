import { describe, expect, it } from "vitest";

import {
  RETACH_GAP_PX,
  awayFromPill,
  createRetachGesture,
  cursorOnPill,
  retachReady,
  retachesOnDrop,
  trackRetach,
} from "./retachMagnet";

/** Pestaña del notch, arriba al centro. */
const pill = { x: 800, y: 0, w: 220, h: 34 };
/** El panel recién despegado: nace en el rect de la cara, pegado al notch. */
const justDetached = { x: 800, y: 34, w: 280, h: 252 };
const farAway = { x: 400, y: 600, w: 280, h: 252 };

describe("awayFromPill", () => {
  it("pegado al notch NO arma el imán", () => {
    expect(awayFromPill(pill, justDetached)).toBe(false);
  });

  it("apenas pasado el gap, arma", () => {
    const out = { ...justDetached, y: pill.h + RETACH_GAP_PX + 1 };
    expect(awayFromPill(pill, out)).toBe(true);
    expect(awayFromPill(pill, { ...justDetached, y: pill.h + RETACH_GAP_PX })).toBe(
      false,
    );
  });

  it("sin medidas se asume lejos: nada que reabsorber", () => {
    expect(awayFromPill(null, justDetached)).toBe(true);
    expect(awayFromPill(pill, null)).toBe(true);
  });
});

describe("retachesOnDrop", () => {
  it("regresión: el gesto del despegue no se re-acopla solo", () => {
    // Nace pegado (imán sin armar) y se suelta ahí mismo: queda flotando.
    const armed = awayFromPill(pill, justDetached);
    expect(retachesOnDrop(armed, pill, justDetached)).toBe(false);
  });

  it("salió y volvió: se coloca", () => {
    let armed = awayFromPill(pill, justDetached);
    armed ||= awayFromPill(pill, farAway); // el arrastre salió de la zona
    expect(retachesOnDrop(armed, pill, justDetached)).toBe(true);
  });

  it("armado pero soltado lejos: sigue flotando", () => {
    expect(retachesOnDrop(true, pill, farAway)).toBe(false);
  });

  it("sin pill medida no se coloca", () => {
    expect(retachesOnDrop(true, null, justDetached)).toBe(false);
  });
});

describe("retachReady", () => {
  const onPill = { x: 900, y: 10 };
  const offPill = { x: 900, y: 200 };

  it("cursor sobre la pill coloca aunque el marco nunca saliera", () => {
    const g = createRetachGesture();
    trackRetach(g, pill, justDetached, offPill);
    trackRetach(g, pill, justDetached, onPill);
    expect(retachReady(g, pill, justDetached, onPill)).toBe(true);
  });

  it("regresión: el despegue con la mano sobre la pill no se re-acopla", () => {
    // El gesto nace con el cursor encima y nunca sale: sin armar.
    const g = createRetachGesture();
    trackRetach(g, pill, justDetached, onPill);
    expect(retachReady(g, pill, justDetached, onPill)).toBe(false);
  });

  it("marco armado y en rango coloca con el cursor en cualquier lado", () => {
    const g = createRetachGesture();
    trackRetach(g, pill, farAway, offPill);
    expect(retachReady(g, pill, justDetached, offPill)).toBe(true);
  });

  it("lejos y sin cursor en la pill, sigue flotando", () => {
    const g = createRetachGesture();
    trackRetach(g, pill, farAway, offPill);
    expect(retachReady(g, pill, farAway, offPill)).toBe(false);
  });

  it("cursorOnPill sin medidas es false", () => {
    expect(cursorOnPill(null, onPill)).toBe(false);
    expect(cursorOnPill(pill, null)).toBe(false);
  });
});
