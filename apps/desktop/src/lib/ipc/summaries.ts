/** Resumen y envío por correo (BYOK). */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  SendMailResult,
  Summary,
  SummaryProvider,
  TemplateInfo,
} from "$core/types";
import { on } from "./events";

export const listSummaryTemplates = () =>
  invoke<TemplateInfo[]>("list_summary_templates");
export const listSummaryProviders = () =>
  invoke<SummaryProvider[]>("list_summary_providers");
export const ollamaAvailable = () => invoke<boolean>("ollama_available");
export const summarizeRecording = (id: string, template: string) =>
  invoke<void>("summarize_recording", { id, template });
export const getSummary = (id: string) => invoke<Summary | null>("get_summary", { id });
export const saveSummary = (id: string, summary: Summary) =>
  invoke<void>("save_summary", { id, summary });

export const sendSummaryEmail = (
  id: string,
  to: string[],
  subject: string,
  body: string,
) => invoke<SendMailResult>("send_summary_email", { id, to, subject, body });

export const onSummaryReady = (cb: (id: string) => void): Promise<UnlistenFn> =>
  on("summary-ready", (p) => cb(p.id));

export const onSummarizeDelta = (
  cb: (id: string, delta: string) => void,
): Promise<UnlistenFn> => on("summarize-delta", (p) => cb(p.id, p.delta));

export const onSummarizeError = (
  cb: (id: string, message: string) => void,
): Promise<UnlistenFn> => on("summarize-error", (p) => cb(p.id, p.message));
