import { describe, expect, it } from "vitest";
import { createSession, type DomainStore } from "./store";

/** Un store falso que anota en qué orden le pidieron las cosas. */
function fake(log: string[], name: string): DomainStore & { stops: number } {
  return {
    stops: 0,
    hydrate() {
      log.push(`${name}:hydrate`);
      return Promise.resolve();
    },
    listen() {
      log.push(`${name}:listen`);
      return Promise.resolve(() => {
        this.stops++;
        log.push(`${name}:stop`);
      });
    },
  };
}

describe("createSession", () => {
  it("se suscribe antes de leer", () => {
    const log: string[] = [];
    const s = createSession({ a: fake(log, "a") });
    return s.start(["a"]).then(() => {
      expect(log).toEqual(["a:listen", "a:hydrate"]);
    });
  });

  it("monta solo lo que se le pide", async () => {
    const log: string[] = [];
    const s = createSession({ a: fake(log, "a"), b: fake(log, "b") });
    await s.start(["a"]);
    expect(s.mounted()).toEqual(["a"]);
    expect(log.some((l) => l.startsWith("b:"))).toBe(false);
  });

  /**
   * El caso que motiva todo esto: dos superficies de la misma ventana piden el
   * mismo dominio. Antes cada una montaba el suyo y quedaban dos relojes.
   */
  it("monta una sola vez aunque se lo pidan dos veces", async () => {
    const log: string[] = [];
    const store = fake(log, "a");
    const s = createSession({ a: store });

    const stop1 = await s.start(["a"]);
    const stop2 = await s.start(["a"]);

    expect(log).toEqual(["a:listen", "a:hydrate"]);

    // La primera baja no desmonta: la segunda superficie sigue usándolo.
    stop1();
    expect(store.stops).toBe(0);
    expect(s.mounted()).toEqual(["a"]);

    stop2();
    expect(store.stops).toBe(1);
    expect(s.mounted()).toEqual([]);
  });

  it("no monta dos veces si se lo piden en paralelo", async () => {
    const log: string[] = [];
    const s = createSession({ a: fake(log, "a") });
    await Promise.all([s.start(["a"]), s.start(["a"]), s.start(["a"])]);
    expect(log).toEqual(["a:listen", "a:hydrate"]);
  });

  it("ignora los nombres repetidos en la misma llamada", async () => {
    const log: string[] = [];
    const store = fake(log, "a");
    const s = createSession({ a: store });
    const stop = await s.start(["a", "a", "a"]);
    expect(log).toEqual(["a:listen", "a:hydrate"]);
    // Una sola baja tiene que alcanzar: si los repetidos hubieran sumado
    // referencias, el dominio quedaría montado para siempre.
    stop();
    expect(s.mounted()).toEqual([]);
  });

  it("desmontar dos veces cuesta una", async () => {
    const log: string[] = [];
    const store = fake(log, "a");
    const s = createSession({ a: store });
    const stop = await s.start(["a"]);
    stop();
    stop();
    expect(store.stops).toBe(1);
  });

  it("se puede volver a montar después de soltar", async () => {
    const log: string[] = [];
    const s = createSession({ a: fake(log, "a") });
    (await s.start(["a"]))();
    await s.start(["a"]);
    expect(log).toEqual(["a:listen", "a:hydrate", "a:stop", "a:listen", "a:hydrate"]);
  });

  it("el efecto desmonta aunque lo cierren antes de terminar de montar", async () => {
    const log: string[] = [];
    const store = fake(log, "a");
    const s = createSession({ a: store });
    // Sin esperar el arranque, como haría un `$effect` que se re-ejecuta.
    s.effect(["a"])();
    await new Promise((r) => setTimeout(r, 0));
    expect(store.stops).toBe(1);
  });
});
