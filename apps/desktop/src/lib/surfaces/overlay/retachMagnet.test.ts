import { describe, expect, it } from "vitest";

import { RETACH_GAP_PX, awayFromPill, retachesOnDrop } from "./retachMagnet";

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
