import { describe, expect, it } from "vitest";
import type { AgentTurn } from "$lib/types";
import {
  answersAsMessage,
  openToolQuestion,
  parseQuestions,
  withAnswers,
} from "./chatQuestions";

const input = {
  questions: [
    {
      question: "¿Qué base de datos?",
      header: "BD",
      options: [{ label: "Postgres", description: "Relacional" }, { label: "SQLite" }],
      multiSelect: false,
    },
    {
      question: "¿Qué tests?",
      header: "Tests",
      options: [{ label: "Unitarios" }, { label: "E2E" }],
      multiSelect: true,
    },
  ],
};

describe("parseQuestions", () => {
  it("lee preguntas, encabezados y opciones", () => {
    const questions = parseQuestions(input);
    expect(questions).toHaveLength(2);
    expect(questions?.[0]).toMatchObject({ header: "BD", multiSelect: false });
    expect(questions?.[0].options[1]).toEqual({ label: "SQLite", description: "" });
    expect(questions?.[1].multiSelect).toBe(true);
  });

  it("lo que no tiene forma de pregunta no es una", () => {
    expect(parseQuestions(null)).toBeNull();
    expect(parseQuestions({ command: "ls" })).toBeNull();
    expect(parseQuestions({ questions: [] })).toBeNull();
    expect(parseQuestions({ questions: [{ header: "sin texto" }] })).toBeNull();
  });
});

describe("withAnswers", () => {
  const questions = parseQuestions(input)!;

  it("agrega las respuestas por texto de la pregunta y conserva el input", () => {
    const out = withAnswers(input, questions, [["SQLite"], ["Unitarios", "E2E"]], []);
    expect(out.questions).toBe(input.questions);
    expect(out.answers).toEqual({
      "¿Qué base de datos?": "SQLite",
      "¿Qué tests?": "Unitarios, E2E",
    });
  });

  it("una respuesta escrita gana; una sin contestar no se manda", () => {
    const out = withAnswers(input, questions, [["SQLite"], []], ["MySQL", ""]);
    expect(out.answers).toEqual({ "¿Qué base de datos?": "MySQL" });
  });
});

describe("preguntas que el agente no pudo hacer (OpenCode por ACP)", () => {
  const opencodeInput = {
    questions: [
      {
        header: "Color",
        multiple: true,
        question: "¿Qué colores?",
        options: [{ label: "Rojo" }, { label: "Azul" }],
      },
    ],
  };
  const failedTool = {
    id: "q1",
    kind: "tool" as const,
    name: "question",
    title: "question",
    toolKind: "other" as const,
    status: "failed" as const,
    input: opencodeInput,
    output: "The user dismissed this question",
    locations: [],
  };
  const turn = (items: unknown[]) =>
    ({ id: "t", items, status: "done", costUsd: null }) as AgentTurn;

  it("lee `multiple` como selección múltiple", () => {
    expect(parseQuestions(opencodeInput)?.[0].multiSelect).toBe(true);
  });

  it("la ofrece si es lo último del turno, aunque el agente haya escrito después", () => {
    const reply = {
      id: "m",
      kind: "message",
      role: "assistant",
      text: "No pude.",
      streaming: false,
    };
    expect(openToolQuestion([turn([failedTool, reply])])?.id).toBe("q1");
  });

  it("no la ofrece si ya contestaste o si la herramienta no falló", () => {
    const mine = {
      id: "u",
      kind: "message",
      role: "user",
      text: "Azul",
      streaming: false,
    };
    expect(openToolQuestion([turn([failedTool]), turn([mine])])).toBeNull();
    expect(
      openToolQuestion([turn([{ ...failedTool, status: "completed" }])]),
    ).toBeNull();
  });

  it("arma el mensaje con las respuestas", () => {
    const questions = parseQuestions(opencodeInput)!;
    expect(answersAsMessage(questions, [["Rojo", "Azul"]], [])).toBe(
      "- ¿Qué colores? → Rojo, Azul",
    );
  });
});
