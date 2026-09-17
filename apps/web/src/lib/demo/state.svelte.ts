/**
 * El estado del demo.
 *
 * Es el shell de la app en miniatura: qué capa está arriba (pill, rueda o
 * float), qué herramienta está abierta y la lista de avisos. Nada de esto
 * toca Tauri: el demo corre entero en el navegador y lo que persiste (textos,
 * tema) vive en `localStorage`.
 *
 * Runa a runa, como los stores de la app (`$domain/*.svelte.ts`).
 */
import type { ToolId } from "$atic/lib/core/tools";

export type DemoPhase = "closed" | "wheel";
export type DemoToolId = Exclude<ToolId, "dictation">;

export type Toast = {
  id: number;
  text: string;
  kind?: "ok" | "info" | "rec";
};

/** Un mensaje del hilo de un agente, ya resuelto para pintar. */
export type AgentMsg = {
  id: number;
  kind: "user" | "text" | "tool" | "done";
  text?: string;
  tool?: {
    name: string;
    arg: string;
    out: string | null;
    status: "run" | "ok";
    lines?: string[];
  };
  meta?: string;
};

let toastSeq = 0;

class DemoStore {
  /** La capa superior de la pill: cerrada o con la rueda desplegada. */
  phase = $state<DemoPhase>("closed");
  /** La herramienta abierta como float. `null` = ninguna. */
  openTool = $state<ToolId | null>(null);

  toasts = $state<Toast[]>([]);

  /** La pizarra no es un float: congela el escritorio entero. Con imagen, la
   *  congela SOBRE esa captura (viene de "Dibujar" en el shelf). */
  boardOpen = $state(false);
  boardImage = $state<string | null>(null);

  openBoard(image: string | null = null) {
    this.boardImage = image;
    this.boardOpen = true;
  }

  closeBoard() {
    this.boardOpen = false;
    this.boardImage = null;
  }

  /** Hilos de los agentes: viven acá para sobrevivir al cerrar el float. */
  agentThreads = $state<Record<string, AgentMsg[]>>({});
  agentRunning = $state<Record<string, boolean>>({});

  get busy() {
    return this.phase !== "closed" || this.openTool !== null;
  }

  toast(text: string, kind: Toast["kind"] = "info") {
    const id = ++toastSeq;
    this.toasts = [...this.toasts.slice(-2), { id, text, kind }];
    // 5 s como en la app (`toasts.svelte.ts`): da tiempo a leer sin apuro.
    setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
    }, 5000);
  }

  openWheel() {
    this.openTool = null;
    this.phase = "wheel";
  }

  closeWheel() {
    this.phase = "closed";
  }

  openToolPanel(id: ToolId) {
    this.phase = "closed";
    this.openTool = id;
  }

  closeTool() {
    this.openTool = null;
  }

  /** Esc: primero cierra la herramienta, después la rueda. */
  escape() {
    if (this.boardOpen) this.closeBoard();
    else if (this.openTool) this.openTool = null;
    else if (this.phase === "wheel") this.phase = "closed";
  }
}

export const demo = new DemoStore();

/** El atajo cambia de nombre según la plataforma, como en la app. */
export function navigatorLabel(): string {
  if (typeof navigator === "undefined") return "Ctrl";
  return /Mac|iPhone|iPad/.test(navigator.userAgent) ? "⌘" : "Ctrl";
}

/**
 * Copia al portapapeles REAL del navegador. Es lo único que el demo hace de
 * verdad, y es a propósito: probar el Clipboard pegando en la app de al lado
 * vale más que ver una animación.
 */
export async function copyText(text: string, toast = "Copiado") {
  try {
    await navigator.clipboard.writeText(text);
    demo.toast(toast, "ok");
  } catch {
    demo.toast("El navegador pidió permiso para el portapapeles", "info");
  }
}

/** Persistencia mínima del demo (textos, tema del sitio). */
export function loadJSON<T>(key: string, fallback: T): T {
  if (typeof localStorage === "undefined") return fallback;
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function saveJSON(key: string, value: unknown) {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* Modo privado, cuota llena: el demo sigue sin persistir. */
  }
}
