/**
 * Las preguntas que hace un agente (`AskUserQuestion` de Claude).
 *
 * Llegan como un pedido de permiso más, con las preguntas en el input. Se
 * contestan aprobando esa herramienta con el mismo input y las respuestas
 * agregadas: `answers`, por texto de la pregunta.
 */

import type { AgentTurn } from "$lib/types";

export type QuestionOption = { label: string; description: string };

export type AgentQuestion = {
  question: string;
  header: string;
  options: QuestionOption[];
  multiSelect: boolean;
};

export const QUESTION_TOOL = "AskUserQuestion";

/** Las preguntas del input, o null si no tiene la forma de una. */
export function parseQuestions(input: unknown): AgentQuestion[] | null {
  if (!input || typeof input !== "object") return null;
  const raw = (input as Record<string, unknown>).questions;
  if (!Array.isArray(raw) || raw.length === 0) return null;
  const questions = raw.flatMap((item): AgentQuestion[] => {
    if (!item || typeof item !== "object") return [];
    const q = item as Record<string, unknown>;
    if (typeof q.question !== "string" || !q.question.trim()) return [];
    const options = Array.isArray(q.options)
      ? q.options.flatMap((o): QuestionOption[] => {
          if (!o || typeof o !== "object") return [];
          const { label, description } = o as Record<string, unknown>;
          return typeof label === "string" && label.trim()
            ? [
                {
                  label,
                  description: typeof description === "string" ? description : "",
                },
              ]
            : [];
        })
      : [];
    return [
      {
        question: q.question,
        header: typeof q.header === "string" ? q.header : "",
        options,
        // Claude dice `multiSelect`; OpenCode, `multiple`.
        multiSelect: q.multiSelect === true || q.multiple === true,
      },
    ];
  });
  return questions.length > 0 ? questions : null;
}

/**
 * El input con las respuestas. Varias elegidas van separadas por coma; si se
 * escribió una propia, gana sobre las opciones.
 */
export function withAnswers(
  input: unknown,
  questions: AgentQuestion[],
  picked: string[][],
  written: string[],
): Record<string, unknown> {
  const answers: Record<string, string> = {};
  questions.forEach((q, i) => {
    const own = written[i]?.trim();
    const chosen = own || (picked[i] ?? []).join(", ");
    if (chosen) answers[q.question] = chosen;
  });
  const base =
    input && typeof input === "object" ? (input as Record<string, unknown>) : {};
  return { ...base, answers };
}

/**
 * Una pregunta que el agente no pudo hacer y quedó sin contestar.
 *
 * OpenCode por ACP no tiene cómo preguntarle al cliente: su herramienta
 * `question` falla sola («the user dismissed this question») con las
 * preguntas en el input. Si es lo último del turno, se ofrecen igual y la
 * respuesta va como el siguiente mensaje.
 */
export function openToolQuestion(
  turns: AgentTurn[],
): { id: string; questions: AgentQuestion[] } | null {
  const last = turns[turns.length - 1];
  if (!last) return null;
  for (let i = last.items.length - 1; i >= 0; i--) {
    const item = last.items[i];
    if (item.kind === "message" && item.role === "user") return null;
    if (item.kind !== "tool") continue;
    if (item.status !== "failed") return null;
    const questions = parseQuestions(item.input);
    return questions ? { id: item.id, questions } : null;
  }
  return null;
}

/** Las respuestas como mensaje, para cuando no hay herramienta que contestar. */
export function answersAsMessage(
  questions: AgentQuestion[],
  picked: string[][],
  written: string[],
): string {
  const lines = questions.flatMap((q, i) => {
    const answer = written[i]?.trim() || (picked[i] ?? []).join(", ");
    return answer ? [`- ${q.question} → ${answer}`] : [];
  });
  return lines.join("\n");
}
