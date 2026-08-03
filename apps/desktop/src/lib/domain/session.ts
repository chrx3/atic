/**
 * Qué estado necesita cada ventana, declarado una sola vez.
 *
 * Hay una instancia de cada store por webview —la pill y la principal son
 * contextos JS distintos— y la verdad vive en Rust. Antes eso se resolvía en
 * el `onMount` de cada vista, con el resultado previsible: la pill escuchaba
 * eventos que no usaba, la principal mantenía estado del clipboard que casi
 * nunca miraba, y cuatro dominios estaban montados dos veces con dos relojes.
 *
 * Acá cada superficie dice qué necesita y el mecanismo de `store.ts` se
 * encarga del resto:
 *
 * ```ts
 * // en el componente raíz de la ventana
 * $effect(() => sessionEffect(["config", "recordings", "models", "capture"]));
 * ```
 */

import { capture } from "./capture.svelte";
import { captures } from "./captures.svelte";
import { clipboard } from "./clipboard.svelte";
import { config } from "./config.svelte";
import { dictation } from "./dictation.svelte";
import { models } from "./models.svelte";
import { recordings } from "./recordings.svelte";
import { snippets } from "./snippets.svelte";
import { createSession, type DomainStore } from "./store";

const REGISTRY = {
  config,
  recordings,
  models,
  capture,
  dictation,
  clipboard,
  snippets,
  captures,
} satisfies Record<string, DomainStore>;

export type DomainName = keyof typeof REGISTRY;

const session = createSession(REGISTRY);

export const startSession = session.start.bind(session);
export const sessionEffect = session.effect.bind(session);

// Un módulo con estado sobrevive al reemplazo en caliente, así que sin esto
// cada guardado deja los oyentes anteriores vivos y los eventos se aplican dos
// veces. Solo pasa en dev, y solo se nota como un bug imposible de reproducir.
if (import.meta.hot) {
  import.meta.hot.dispose(() => session.stopAll());
}
