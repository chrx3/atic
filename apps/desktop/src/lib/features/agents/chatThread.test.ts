import { describe, expect, it } from "vitest";
import type { AgentItem, AgentModel, AgentTurn } from "$lib/types";
import {
  activityTabs,
  countActivity,
  editedFiles,
  formatDuration,
  matchModel,
  modelGroups,
  shortModelName,
  toBlocks,
  type ActivityItem,
} from "./chatThread";

function msg(id: string, role: "user" | "assistant", text = id): AgentItem {
  return { id, kind: "message", role, text, streaming: false };
}

function tool(
  id: string,
  toolKind: "read" | "edit" | "execute" | "search" = "read",
  status: "completed" | "failed" | "in_progress" = "completed",
): AgentItem {
  return {
    id,
    kind: "tool",
    name: toolKind,
    title: id,
    toolKind,
    status,
    input: null,
    output: "",
    locations: [],
  };
}

function turn(items: AgentItem[], status: AgentTurn["status"] = "done"): AgentTurn {
  return { id: `turn-${items[0]?.id ?? "x"}`, items, status, costUsd: null };
}

describe("toBlocks", () => {
  it("junta las herramientas seguidas en un solo bloque", () => {
    const blocks = toBlocks([
      turn([msg("u", "user"), tool("a"), tool("b", "edit"), msg("r", "assistant")]),
    ]);
    expect(blocks.map((b) => b.kind)).toEqual(["user", "activity", "text"]);
    const activity = blocks[1];
    expect(activity.kind === "activity" && activity.items.map((i) => i.id)).toEqual([
      "a",
      "b",
    ]);
  });

  it("un texto en medio corta la actividad en dos tramos", () => {
    const blocks = toBlocks([
      turn([tool("a"), msg("t", "assistant"), tool("b")], "running"),
    ]);
    expect(blocks.map((b) => b.kind)).toEqual(["activity", "text", "activity"]);
    // Solo el último tramo del turno vivo está pasando.
    expect(blocks[0].kind === "activity" && blocks[0].live).toBe(false);
    expect(blocks[2].kind === "activity" && blocks[2].live).toBe(true);
  });

  it("no dibuja permisos ni avisos de estado", () => {
    const blocks = toBlocks([
      turn([
        {
          id: "p",
          kind: "permission",
          tool: "Bash",
          description: "",
          input: null,
          status: "pending",
        },
        { id: "n", kind: "notice", text: "sistema: hook_started" },
        { id: "m", kind: "notice", text: "Modelo: opus" },
        msg("r", "assistant"),
      ]),
    ]);
    expect(blocks.map((b) => b.kind)).toEqual(["text"]);
  });

  it("la actividad de un turno terminado no está viva", () => {
    const blocks = toBlocks([turn([tool("a")], "done")]);
    expect(blocks[0].kind === "activity" && blocks[0].live).toBe(false);
  });
});

describe("countActivity", () => {
  it("cuenta por clase y separa los fallos", () => {
    const items = [
      tool("a", "read"),
      tool("b", "read"),
      tool("c", "edit"),
      tool("d", "execute", "failed"),
      { id: "r", kind: "reasoning", text: "…", streaming: false },
    ] as ActivityItem[];
    const counts = countActivity(items);
    expect(counts).toMatchObject({
      read: 2,
      edit: 1,
      run: 1,
      failed: 1,
      thought: true,
    });
  });
});

describe("activityTabs", () => {
  it("una pestaña por clase presente, con «todo» primero", () => {
    const items = [
      tool("a", "read"),
      tool("b", "search"),
      tool("c", "edit"),
      { id: "r", kind: "reasoning", text: "…", streaming: false },
    ] as ActivityItem[];
    expect(activityTabs(items)).toEqual([
      { id: "all", count: 4 },
      { id: "thought", count: 1 },
      { id: "read", count: 2 },
      { id: "edit", count: 1 },
    ]);
  });

  it("con una sola clase no hay pestañas", () => {
    expect(activityTabs([tool("a"), tool("b")] as ActivityItem[])).toEqual([]);
  });
});

describe("shortModelName", () => {
  it("quita el proveedor", () => {
    expect(shortModelName("opencode-go/Space Bunny Fast")).toBe("Space Bunny Fast");
    expect(shortModelName("minimax/MiniMax-M2.5")).toBe("MiniMax-M2.5");
  });

  it("sin proveedor, o con barra al final, deja el nombre", () => {
    expect(shortModelName("Opus 5")).toBe("Opus 5");
    expect(shortModelName("raro/")).toBe("raro/");
  });
});

describe("modelGroups", () => {
  const models = [
    { id: "minimax/M2" },
    { id: "minimax/M2.5" },
    { id: "opencode-go/space-bunny-free" },
    { id: "nvidia/nemotron" },
  ];

  it("agrupa por proveedor y pone primero el del modelo en uso", () => {
    const groups = modelGroups(models, "opencode-go/space-bunny-free");
    expect(groups.map((g) => g.provider)).toEqual(["opencode-go", "minimax", "nvidia"]);
    expect(groups[1].models.map((m) => m.id)).toEqual(["minimax/M2", "minimax/M2.5"]);
  });

  it("ids sin proveedor quedan en un solo grupo", () => {
    const groups = modelGroups([{ id: "opus" }, { id: "sonnet" }], "opus");
    expect(groups).toHaveLength(1);
    expect(groups[0].provider).toBe("");
  });
});

describe("matchModel", () => {
  const models: AgentModel[] = [
    { id: "opus", name: "Opus 5", description: "", efforts: [] },
    { id: "sonnet", name: "Sonnet 5", description: "", efforts: [] },
    { id: "gpt-5", name: "GPT-5", description: "", efforts: [] },
    { id: "gpt-5-codex", name: "GPT-5 Codex", description: "", efforts: [] },
  ];

  it("prefiere el id exacto", () => {
    expect(matchModel(models, "gpt-5")?.name).toBe("GPT-5");
  });

  it("resuelve el id completo al alias que lo contiene", () => {
    expect(matchModel(models, "claude-opus-5-5")?.name).toBe("Opus 5");
  });

  it("entre dos alias contenidos gana el más largo", () => {
    expect(matchModel(models, "gpt-5-codex-latest")?.name).toBe("GPT-5 Codex");
  });

  it("sin coincidencia no inventa", () => {
    expect(matchModel(models, "haiku")).toBeUndefined();
    expect(matchModel(models, "")).toBeUndefined();
  });
});

describe("editedFiles", () => {
  function edit(
    id: string,
    input: unknown,
    status: "completed" | "failed" = "completed",
    locations: string[] = [],
  ): ActivityItem {
    return {
      ...(tool(id, "edit", status) as ActivityItem & { kind: "tool" }),
      input,
      locations,
    };
  }

  it("junta las ediciones de un mismo archivo y suma sus líneas", () => {
    const files = editedFiles([
      edit("a", { file_path: "/r/a.ts", old_string: "x", new_string: "y\nz" }),
      tool("r") as ActivityItem,
      edit("b", { file_path: "/r/b.ts", content: "1\n2\n3" }),
      edit("c", { file_path: "/r/a.ts", old_string: "p\nq", new_string: "" }),
    ]);
    expect(files).toEqual([
      { path: "/r/a.ts", add: 3, del: 3 },
      { path: "/r/b.ts", add: 3, del: 0 },
    ]);
  });

  it("prefiere la ubicación que informa el agente y descarta lo que falló", () => {
    const files = editedFiles([
      edit("a", { old_string: "x", new_string: "y" }, "completed", ["/r/loc.ts"]),
      edit("b", { file_path: "/r/bad.ts", content: "x" }, "failed"),
    ]);
    expect(files).toEqual([{ path: "/r/loc.ts", add: 1, del: 1 }]);
  });
});

describe("turno plegado", () => {
  const done = (items: AgentItem[], durationMs?: number): AgentTurn => ({
    ...turn(items, "done"),
    durationMs,
  });

  it("pliega lo de en medio y deja la respuesta a la vista", () => {
    const blocks = toBlocks([
      done(
        [
          msg("u", "user"),
          tool("a"),
          msg("m", "assistant"),
          tool("b"),
          msg("r", "assistant"),
        ],
        33_000,
      ),
    ]);
    expect(blocks.map((b) => b.kind)).toEqual(["user", "work", "text"]);
    const work = blocks[1];
    expect(work.kind === "work" && work.blocks.map((b) => b.kind)).toEqual([
      "activity",
      "text",
      "activity",
    ]);
  });

  it("si terminó trabajando, no hay respuesta que dejar afuera", () => {
    const blocks = toBlocks([
      done([msg("u", "user"), msg("m", "assistant"), tool("a")], 5_000),
    ]);
    expect(blocks.map((b) => b.kind)).toEqual(["user", "work"]);
  });

  it("sin duración, en curso o sin herramientas, no se pliega", () => {
    expect(
      toBlocks([done([tool("a"), msg("r", "assistant")])]).map((b) => b.kind),
    ).toEqual(["activity", "text"]);
    expect(
      toBlocks([{ ...turn([tool("a")], "running"), durationMs: 10 }]).map(
        (b) => b.kind,
      ),
    ).toEqual(["activity"]);
    expect(toBlocks([done([msg("r", "assistant")], 10)]).map((b) => b.kind)).toEqual([
      "text",
    ]);
  });

  it("formatea la duración", () => {
    expect(formatDuration(33_400)).toBe("33s");
    expect(formatDuration(72_000)).toBe("1m 12s");
    expect(formatDuration(3_900_000)).toBe("1h 5m");
  });
});
