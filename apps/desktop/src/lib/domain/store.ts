/**
 * El contrato que cumple todo store de dominio, y el mecanismo que los monta.
 *
 * Tres reglas, y las tres salen de errores que este proyecto ya cometió:
 *
 * 1. **El estado del frontend es una proyección, nunca la fuente.** La verdad
 *    vive en Rust. `hydrate()` la lee entera y los eventos traen los cambios;
 *    las escrituras son optimistas y vuelven atrás si el comando falla.
 * 2. **Arrancar es idempotente.** Hay una instancia por webview y cualquier
 *    vista puede montar primero, así que pedir el mismo dominio dos veces
 *    tiene que costar uno.
 * 3. **Lo que debe pasar una sola vez en todo el sistema** —avisarle al SO,
 *    por ejemplo— es una capacidad que se pide al arrancar la sesión, no un
 *    global que cada store decide por su cuenta.
 *
 * Este archivo es TS puro a propósito: los stores de verdad usan runes y
 * hablan con Rust, así que si el mecanismo viviera con ellos no habría forma
 * de probarlo sin montar la app entera.
 */

export interface DomainStore {
  /** Lee el estado completo desde Rust. Idempotente. */
  hydrate(): Promise<void>;
  /** Se suscribe a sus eventos. Devuelve el desmontaje. */
  listen(): Promise<() => void>;
}

interface Entry {
  refs: number;
  ready: Promise<void>;
  stop?: () => void;
}

export interface Session<Name extends string> {
  /** Monta los dominios pedidos y devuelve el desmontaje. */
  start(need: Name[]): Promise<() => void>;
  /** Igual, con la forma que espera el `return` de un `$effect`. */
  effect(need: Name[]): () => void;
  /** Suelta todo. Para el reemplazo en caliente y para los tests. */
  stopAll(): void;
  mounted(): Name[];
}

export function createSession<T extends Record<string, DomainStore>>(
  registry: T,
): Session<keyof T & string> {
  type Name = keyof T & string;
  const live = new Map<Name, Entry>();

  async function acquire(name: Name): Promise<void> {
    const existing = live.get(name);
    if (existing) {
      existing.refs++;
      await existing.ready;
      return;
    }

    const store: DomainStore = registry[name];
    const entry: Entry = { refs: 1, ready: Promise.resolve() };
    // Se registra antes de arrancar, y sin ningún `await` en el medio: así, una
    // segunda llamada para el mismo dominio encuentra la entrada y espera esta
    // misma promesa en vez de montar el store por segunda vez.
    live.set(name, entry);
    entry.ready = (async () => {
      // Suscribirse ANTES de leer, no después. Al revés hay una ventana entre
      // la lectura y la suscripción en la que un evento se pierde, y el store
      // se queda mostrando algo viejo hasta el próximo cambio — que es
      // exactamente el tipo de bug que no se reproduce cuando lo buscás.
      entry.stop = await store.listen();
      await store.hydrate();
    })();
    await entry.ready;
  }

  function release(name: Name): void {
    const entry = live.get(name);
    if (!entry) return;
    if (--entry.refs > 0) return;
    live.delete(name);
    entry.stop?.();
  }

  return {
    /**
     * Se cuentan referencias: dos superficies de la misma ventana pueden pedir
     * el mismo dominio y solo se monta una vez, y solo se desmonta cuando la
     * última lo suelta.
     *
     * (Todavía no hay capacidades del estilo «esta ventana es la que notifica
     * al SO». Van a hacer falta cuando entre el store de agentes, y entonces
     * se agregan acá; declararlas antes sería inventar un parámetro que nadie
     * pasa.)
     */
    async start(need) {
      const unique = [...new Set(need)];
      await Promise.all(unique.map((name) => acquire(name)));

      let released = false;
      return () => {
        if (released) return;
        released = true;
        for (const name of unique) release(name);
      };
    },

    /**
     * El efecto es síncrono y el arranque no, así que el desmontaje tiene que
     * encadenarse a la promesa: si la ventana se cierra antes de que termine
     * de montar, esto igual desmonta lo que alcanzó a montarse.
     */
    effect(need) {
      const pending = this.start(need);
      return () => void pending.then((stop) => stop()).catch(() => {});
    },

    stopAll() {
      for (const entry of live.values()) entry.stop?.();
      live.clear();
    },

    mounted() {
      return [...live.keys()];
    },
  };
}
