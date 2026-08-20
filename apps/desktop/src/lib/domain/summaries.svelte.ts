/**
 * Los resúmenes: el guardado de cada grabación y el que se está escribiendo.
 *
 * A diferencia del resto de los stores, este no es solo una proyección: además
 * del resumen guardado tiene el **borrador**, que es a la vez lo que el usuario
 * edita y el destino de los `summarize-delta` que llegan token a token. Los dos
 * escriben en el mismo texto, así que tenerlo en el componente significaba que
 * cerrar el modal a media generación perdía lo que ya había llegado —y los
 * deltas seguían llegando a un oyente muerto—. Acá el borrador sobrevive a la
 * pantalla que lo muestra.
 */

import type { Summary, TemplateInfo } from "$core/types";
import { subscribe } from "$ipc/events";
import {
  getSummary,
  listSummaryTemplates,
  saveSummary,
  summarizeRecording,
} from "$ipc/summaries";
import { toasts } from "./toasts.svelte";
import { t } from "./i18n.svelte";
import type { DomainStore } from "./store";

/** Un fallo que se arregla en Ajustes, no reintentando. */
function isSetupProblem(message: string): boolean {
  const lower = message.toLowerCase();
  return lower.includes("api key") || lower.includes("ollama");
}

class SummariesStore implements DomainStore {
  templates = $state<TemplateInfo[]>([]);

  /** El resumen guardado por grabación. `null` = se preguntó y no hay. */
  byId = $state<Record<string, Summary | null>>({});

  /** Qué grabación se está generando. Rust solo permite una a la vez. */
  generating = $state<string | null>(null);

  /** Lo que se está editando: el cuerpo, el asunto y la plantilla elegida. */
  draft = $state("");
  subject = $state("");
  template = $state("");
  dirty = $state(false);

  /** El último fallo fue de configuración: reintentar no lo arregla. */
  needsSetup = $state(false);

  /** De qué grabación es el borrador. Evita releer al volver a abrirla. */
  #openId: string | null = null;

  async hydrate(): Promise<void> {
    this.templates = await listSummaryTemplates();
    if (!this.template) this.template = this.templates[0]?.id ?? "";
  }

  async listen(): Promise<() => void> {
    return subscribe({
      "summarize-delta": (p) => {
        // Se filtra por id: si la generación de otra grabación sigue viva, sus
        // tokens no tienen que aparecer en este borrador.
        if (p.id !== this.generating) return;
        this.draft += p.delta;
      },
      "summary-ready": (p) => {
        if (p.id === this.generating) this.generating = null;
        void this.#reload(p.id);
      },
      "summarize-error": (p) => {
        if (p.id !== this.generating) return;
        this.generating = null;
        this.needsSetup = isSetupProblem(p.message);
        toasts.push(p.message);
      },
    });
  }

  get current(): Summary | null {
    return this.#openId ? (this.byId[this.#openId] ?? null) : null;
  }

  get busy(): boolean {
    return this.generating !== null;
  }

  /**
   * Prepara el borrador de una grabación. Idempotente: volver a la misma no
   * pisa lo que el usuario venía escribiendo.
   */
  async open(id: string, fallbackTitle: string): Promise<void> {
    if (this.#openId === id) return;
    this.#openId = id;
    this.needsSetup = false;
    const summary = await getSummary(id);
    // Puede haber cambiado de grabación mientras se leía.
    if (this.#openId !== id) return;
    this.byId = { ...this.byId, [id]: summary };
    this.draft = summary?.body ?? "";
    this.subject = summary?.subject ?? `Seguimiento: ${fallbackTitle}`;
    this.template = summary?.template ?? this.templates[0]?.id ?? this.template;
    this.dirty = false;
  }

  /** Marca que lo que hay en pantalla ya no es lo guardado. */
  touch(): void {
    this.dirty = true;
  }

  async generate(id: string): Promise<void> {
    this.generating = id;
    this.needsSetup = false;
    this.draft = "";
    this.dirty = false;
    try {
      await summarizeRecording(id, this.template);
    } catch (error) {
      this.generating = null;
      const message = String(error);
      this.needsSetup = isSetupProblem(message);
      // El borrador se restaura desde lo guardado: dejarlo vacío daría a
      // entender que el resumen anterior se perdió, y sigue estando.
      this.draft = this.byId[id]?.body ?? "";
      throw error;
    }
  }

  async save(id: string, recordingTitle: string): Promise<Summary | null> {
    if (!this.draft.trim()) return null;
    const previous = this.byId[id] ?? null;
    const next: Summary = {
      template: this.template,
      title: previous?.title ?? t("page.summary.namedTitle", { title: recordingTitle }),
      body: this.draft,
      subject: this.subject.trim() || null,
      // `manual` distingue lo escrito a mano de lo que generó un proveedor.
      backend: previous?.backend || "manual",
      created_at: previous?.created_at ?? new Date().toISOString(),
    };
    await saveSummary(id, next);
    this.byId = { ...this.byId, [id]: next };
    this.dirty = false;
    return next;
  }

  async #reload(id: string): Promise<void> {
    const summary = await getSummary(id);
    this.byId = { ...this.byId, [id]: summary };
    if (this.#openId !== id || !summary) return;
    this.draft = summary.body;
    this.subject = summary.subject ?? this.subject;
    this.template = summary.template;
    this.dirty = false;
  }
}

export const summaries = new SummariesStore();
