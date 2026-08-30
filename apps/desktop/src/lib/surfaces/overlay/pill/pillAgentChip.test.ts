import { describe, expect, it } from "vitest";
import type { PresenceView } from "$lib/agentPresenceReduce";
import { agentChip, cueAgentId, cueAgentIds, type ChipTone } from "./pillAgentChip";

const emptyChat = {
  unread: 0,
  working: false,
  waiting: 0,
  readyLabel: null as string | null,
};

function presence(
  partial: Partial<PresenceView> & Pick<PresenceView, "id" | "status">,
): PresenceView {
  return {
    backendId: "claude-code",
    backendName: "Claude Code",
    cwd: "/x",
    preview: "El arreglo ya está",
    updatedAt: 1,
    window: null,
    source: "jsonl",
    unread: 0,
    ...partial,
  };
}

function chip(opts: {
  chat?: Partial<typeof emptyChat> & {
    providerSessions?: Array<string | null>;
    updatedAt?: number;
    readyBackendId?: string | null;
  };
  presence?: PresenceView[];
  chatEnabled?: boolean;
  pagerEnabled?: boolean;
  consoles?: Array<string | null | undefined>;
}) {
  return agentChip({
    chat: { ...emptyChat, ...opts.chat },
    presence: opts.presence ?? [],
    chatEnabled: opts.chatEnabled ?? true,
    pagerEnabled: opts.pagerEnabled ?? true,
    consoles: opts.consoles,
  });
}

describe("agentChip", () => {
  it("los dos flags apagados apagan el chip", () => {
    expect(
      chip({
        chat: { unread: 2, readyLabel: "hola" },
        presence: [presence({ id: "t", status: "working" })],
        chatEnabled: false,
        pagerEnabled: false,
      }).tone,
    ).toBe<ChipTone>("off");
  });

  it("prioridad waiting > working > ready, y a igualdad gana el chat", () => {
    expect(
      chip({
        chat: { waiting: 1 },
        presence: [presence({ id: "t", status: "working" })],
      }),
    ).toEqual({
      tone: "waiting",
      label: "permiso",
      target: { kind: "console" },
      logoId: null,
    });

    expect(
      chip({
        chat: { working: true },
        presence: [presence({ id: "t", status: "ready", unread: 1 })],
      }).tone,
    ).toBe("working");

    expect(
      chip({
        chat: { working: true, unread: 1, readyLabel: "Soy Muse Spark" },
      }),
    ).toEqual({
      tone: "working",
      label: "Soy Muse Spark",
      target: { kind: "console" },
      logoId: null,
    });

    expect(
      chip({
        chat: { unread: 1, readyLabel: "desde el chat", updatedAt: 5 },
        presence: [
          presence({
            id: "t",
            status: "ready",
            unread: 1,
            preview: "desde la tui",
            updatedAt: 5,
          }),
        ],
      }),
    ).toEqual({
      tone: "ready",
      label: "desde el chat",
      target: { kind: "console" },
      logoId: null,
    });
  });

  it("sin HWND el destino es none, no la consola", () => {
    const result = chip({
      chatEnabled: false,
      presence: [presence({ id: "t", status: "working", preview: "" })],
    });
    expect(result).toEqual({
      tone: "working",
      label: null,
      target: { kind: "none", presenceId: "t" },
      logoId: "claude-code",
    });
  });

  it("con HWND el destino es focus", () => {
    expect(
      chip({
        chatEnabled: false,
        presence: [
          presence({
            id: "t",
            status: "ready",
            unread: 1,
            window: { pid: 1, hwnd: 99 },
          }),
        ],
      }).target,
    ).toEqual({ kind: "focus", presenceId: "t" });
  });

  it("ignora una presencia cuyo id es providerSession del chat", () => {
    const result = chip({
      chat: { providerSessions: ["t"] },
      presence: [presence({ id: "t", status: "working" })],
    });
    expect(result.tone).toBe("off");
  });

  it("pager apagado no muestra la TUI", () => {
    expect(
      chip({
        pagerEnabled: false,
        presence: [presence({ id: "t", status: "working" })],
      }).tone,
    ).toBe("off");
  });

  it("una TUI más nueva gana al saludo viejo de otra", () => {
    expect(
      chip({
        chat: {
          unread: 1,
          readyLabel: "¡Holaaa!",
          updatedAt: 10,
          readyBackendId: "opencode",
        },
        presence: [
          presence({
            id: "c",
            backendId: "codex",
            status: "ready",
            unread: 1,
            preview: "Soy Codex",
            updatedAt: 50,
          }),
        ],
      }),
    ).toMatchObject({
      tone: "ready",
      label: "Soy Codex",
      logoId: "codex",
    });
  });

  it("ignora presencia stale sin HWND si la consola viva es otra", () => {
    expect(
      chip({
        chatEnabled: false,
        consoles: ["codex"],
        presence: [
          presence({
            id: "o",
            backendId: "opencode",
            status: "ready",
            unread: 1,
            preview: "¡Holaaa!",
            updatedAt: 10,
          }),
          presence({
            id: "c",
            backendId: "codex",
            status: "ready",
            unread: 1,
            preview: "Soy Codex",
            updatedAt: 5,
          }),
        ],
      }),
    ).toEqual({
      tone: "ready",
      label: "Soy Codex",
      target: { kind: "none", presenceId: "c" },
      logoId: "codex",
    });
  });

  it("chat stale de otro backend se ignora si la consola viva no coincide", () => {
    expect(
      chip({
        chat: {
          unread: 1,
          readyLabel: "¡Holaaa!",
          readyBackendId: "opencode",
          updatedAt: 100,
        },
        consoles: ["codex"],
        presence: [
          presence({
            id: "c",
            backendId: "codex",
            status: "ready",
            unread: 1,
            preview: "jokes",
            updatedAt: 5,
          }),
        ],
      }),
    ).toEqual({
      tone: "ready",
      label: "jokes",
      target: { kind: "none", presenceId: "c" },
      logoId: "codex",
    });
  });

  it("presencia working con preview muestra ese texto", () => {
    expect(
      chip({
        chatEnabled: false,
        presence: [
          presence({
            id: "t",
            status: "working",
            preview: "Generando respuesta…",
          }),
        ],
      }),
    ).toEqual({
      tone: "working",
      label: "Generando respuesta…",
      target: { kind: "none", presenceId: "t" },
      logoId: "claude-code",
    });
  });
});

describe("cueAgentId", () => {
  it("un agente ocupado gana a una consola suelta", () => {
    expect(
      cueAgentId({
        sessions: [{ backendId: "claude-code", status: "working" }],
        presence: [],
      }),
    ).toBe("claude-code");
  });

  it("la TUI ocupada también nombra al agente", () => {
    expect(
      cueAgentId({
        sessions: [],
        presence: [{ backendId: "codex", status: "waiting" }],
      }),
    ).toBe("codex");
  });

  it("sin agente ocupado es consola: no hay marca", () => {
    expect(
      cueAgentId({
        sessions: [{ backendId: "claude-code", status: "ready" }],
        presence: [{ backendId: "opencode", status: "idle" }],
      }),
    ).toBeNull();
  });

  it("un backend desconocido no inventa logo", () => {
    expect(
      cueAgentId({
        sessions: [{ backendId: "shell", status: "working" }],
        presence: [],
      }),
    ).toBeNull();
  });

  it("una consola Codex cuenta aunque el chat esté idle", () => {
    expect(
      cueAgentIds({
        sessions: [{ backendId: "claude-code", status: "ready" }],
        presence: [],
        consoles: ["C:\\\\Users\\\\x\\\\codex.exe"],
      }),
    ).toEqual(["codex"]);
  });

  it("el logo del aviso no mezcla otras consolas", () => {
    expect(
      cueAgentIds({
        sessions: [{ backendId: "claude-code", status: "working" }],
        presence: [],
        consoles: ["opencode", "codex"],
        chipLogoId: "codex",
      }),
    ).toEqual(["codex"]);
  });

  it("varios agentes ocupados y consolas no se repiten", () => {
    expect(
      cueAgentIds({
        sessions: [{ backendId: "claude-code", status: "working" }],
        presence: [{ backendId: "codex", status: "working" }],
        consoles: ["codex", "opencode"],
      }),
    ).toEqual(["claude-code", "codex", "opencode"]);
  });
});
