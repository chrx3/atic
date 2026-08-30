/**
 * Todo lo que Rust emite, en un solo mapa.
 *
 * Antes cada envoltorio declaraba su nombre y su carga por su cuenta, así que
 * el catálogo de eventos solo existía repartido entre treinta funciones y no
 * había forma de saber si un nombre estaba bien escrito hasta que el evento no
 * llegaba nunca. Acá el nombre es una clave y equivocarse es un error de tipo.
 *
 * Las cargas son **crudas**: exactamente lo que manda Rust, sin desenvolver. Si
 * un consumidor solo quiere el `id` de un `{ id, message }`, que lo saque él —
 * el mapa tiene que poder leerse contra el código de Rust sin traducir nada.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ToolId } from "$core/tools";
import type {
  AgentDeltaPayload,
  AgentPresence,
  AgentsComposerInsert,
  AnnotateOpen,
  BubbleOpen,
  CaptureItem,
  ConsoleExitPayload,
  ConsoleOutputPayload,
  DictationStatusPayload,
  DownloadProgress,
  Levels,
  MeetingDetectionPayload,
  Segment,
  StatusPayload,
  TranscribeProgress,
} from "$core/types";

export type AgentsWorkspaceShortcut =
  | "split-right"
  | "split-down"
  | "new-console"
  | "close-console";

export interface AticEvents {
  // Grabación
  "audio-levels": Levels;
  "recording-status": StatusPayload;
  "recordings-changed": void;
  "capture-error": { message: string };
  "capture-warn": { message: string };
  "meeting-detection": MeetingDetectionPayload;

  // Dictado
  "dictation-status": DictationStatusPayload;

  // Modelos
  "model-download-progress": DownloadProgress;
  "model-download-done": { id: string };
  "model-download-error": { id: string; message: string };

  // Transcripción
  "transcribe-progress": TranscribeProgress;
  "transcript-ready": { id: string };
  "transcribe-error": { id: string; message: string };
  "live-transcript-partial": Segment;
  "live-transcript-final": Segment;
  "live-transcript-error": { message: string };

  // Resumen
  "summary-ready": { id: string };
  "summarize-delta": { id: string; delta: string };
  "summarize-error": { id: string; message: string };

  // Capturas
  "screenshot-created": CaptureItem;
  "screenshot-shelf-updated": void;
  /** Con qué imagen tiene que abrirse el editor de anotaciones. */
  "annotate-open": AnnotateOpen;
  /** Hay una foto congelada lista y el overlay de selección puede pintarla. */
  "overlay-session-started": void;
  "overlay-session-ended": void;

  // Clipboard y fragmentos
  "clipboard-history-changed": void;
  "agents-composer-insert": AgentsComposerInsert;
  "snippets-changed": void;
  "clipboard-bubble-anchor": BubbleOpen;
  "clipboard-bubble-dismiss": void;
  "snippets-bubble-anchor": BubbleOpen;
  "snippets-bubble-dismiss": void;

  // Cola de pegado
  "paste-queue-changed": void;
  "paste-queued": { preview: string };

  // Pill y overlay
  "pill-visibility": boolean;
  "pill-clipboard-toggle": void;
  "pill-clipboard-close": void;
  "pill-snippets-toggle": void;
  "pill-snippets-close": void;
  "pill-reset": void;
  "pill-radial-press": void;
  "pill-radial-release": void;
  "overlay-dismiss": void;
  /** `main` está debajo del cursor: soltar drag fullscreen del overlay. */
  "overlay-yield-main": void;
  /** El overlay ya está colocado: republicar viewport CSS y hit-rects. */
  "overlay-ready": void;

  // Agentes
  "agents-bubble-anchor": BubbleOpen;
  "agents-bubble-dismiss": void;
  /** El float ya está abierto: agrandar si estaba achicado junto a la pill. */
  "agents-bubble-expand": void;
  "agent-event": AgentDeltaPayload;
  "agent-presence": AgentPresence[];
  "console-output": ConsoleOutputPayload;
  "console-exit": ConsoleExitPayload;
  "agents-workspace-shortcut": AgentsWorkspaceShortcut;

  // Sistema
  "shortcuts-failed": string[];
  /**
   * Tema de UI persistido (`light` | `dark` | `system`).
   *
   * El overlay no comparte localStorage con main (perfil WebView2 propio),
   * así que el cache `atic-theme` no alcanza: hay que avisarle por IPC.
   */
  "ui-theme": string;
  /**
   * Idioma de UI persistido (`es` | `en`).
   *
   * El overlay no hidrata el store de config de `main`: hay que avisarle.
   */
  "ui-language": string;
  /**
   * El setup o la práctica de primer uso cambió.
   *
   * El overlay no comparte el store de `main`: tiene que volver a leer config
   * para mostrar u ocultar el coach junto a la pill.
   */
  "onboarding-practice": void;
  /**
   * Cambió qué herramientas muestra la pill (Ajustes → Pill).
   *
   * Mismo motivo que los de arriba: el overlay tiene su propia copia de la
   * config y no se entera de que `main` la guardó.
   */
  "pill-tools": void;
  /** Float launcher: ancla / dismiss (sale de la pill). */
  "launcher-bubble-anchor": BubbleOpen;
  "launcher-bubble-dismiss": void;
  /** Launcher abierto (reset UI / foco). */
  "launcher-opened": void;
  /** Launcher oculto (Esc, exclusive, run). */
  "launcher-closed": void;
  /** Abrir el buscador de la ventana principal (puede emitirlo el frontend). */
  "open-search": void;
  /**
   * Catálogo / ToolRail / atajo: el overlay vuela al slot y ejecuta la tool.
   * También lo emite Rust (p.ej. dictado toggle al empezar).
   */
  "activate-tool-slot": ToolId;
  /**
   * Solo vuelo al slot (sin ejecutar). PTT: vuela en paralelo al start.
   */
  "fly-tool-slot": ToolId;
}

export type AticEvent = keyof AticEvents;

/** Un evento suelto. La base de los envoltorios `onXxx` de cada módulo. */
export function on<K extends AticEvent>(
  name: K,
  cb: (payload: AticEvents[K]) => void,
): Promise<UnlistenFn> {
  return listen<AticEvents[K]>(name, (e) => cb(e.payload));
}

export type Handlers = {
  [K in AticEvent]?: (payload: AticEvents[K]) => void;
};

/**
 * Muchos eventos de una vez, con un solo desmontaje.
 *
 * Reemplaza al patrón que hoy está copiado en cinco archivos: juntar promesas
 * en un array, esperarlas todas y acordarse de recorrerlo al desmontar. Ese
 * patrón falla en silencio de dos maneras —olvidarse una baja, o desmontar
 * antes de que las suscripciones resuelvan y quedarse con oyentes vivos para
 * siempre— y las dos las cierra esta función.
 *
 * ```ts
 * const stop = await subscribe({
 *   "audio-levels": (l) => (levels = l),
 *   "recordings-changed": () => void hydrate(),
 * });
 * ```
 */
export async function subscribe(handlers: Handlers): Promise<() => void> {
  const entries = Object.entries(handlers) as [AticEvent, (payload: never) => void][];

  const unlisteners = await Promise.all(
    entries.map(([name, cb]) =>
      listen(name, (e) => cb(e.payload as never)).catch(() => {
        // Fuera de Tauri no hay nada que escuchar. Que una superficie no pueda
        // suscribirse no debe impedir que las demás lo hagan.
        return () => {};
      }),
    ),
  );

  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    for (const un of unlisteners) un();
  };
}
