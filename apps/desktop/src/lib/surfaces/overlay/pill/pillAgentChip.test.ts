import { describe, expect, it } from "vitest";
import type { PresenceView } from "$lib/agentPresenceReduce";
import {
  type ChatChipSession,
  agentChipLogos,
  agentChip,
  agentChips,
  cueAgentId,
  cueAgentIds,
  presenceIdsToDismissOnAticHide,
  type ChipTone,
  logoSlots,
} from "./pillAgentChip";

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
    answering?: boolean;
  };
  presence?: PresenceView[];
  chatEnabled?: boolean;
  pagerEnabled?: boolean;
  consoles?: Array<string | null | undefined>;
  now?: number;
  workingLabel?: string;
  answeringLabel?: string;
}) {
  return agentChip({
    chat: { ...emptyChat, ...opts.chat },
    presence: opts.presence ?? [],
    chatEnabled: opts.chatEnabled ?? true,
    pagerEnabled: opts.pagerEnabled ?? true,
    consoles: opts.consoles,
    now: opts.now,
    workingLabel: opts.workingLabel,
    answeringLabel: opts.answeringLabel,
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

  it("ready sin leer muestra el inicio de la respuesta; ya visto se apaga", () => {
    expect(
      chip({ presence: [presence({ id: "t", status: "ready", unread: 0 })] }).tone,
    ).toBe<ChipTone>("off");
    expect(
      chip({
        presence: [presence({ id: "t", status: "ready", unread: 1, preview: "chau" })],
      }).label,
    ).toBe("chau");
    expect(
      chip({
        presence: [presence({ id: "t", status: "ready", unread: 0, preview: null })],
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
      id: "chat",
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
      id: "chat",
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
      id: "chat",
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
      id: "t",
      tone: "working",
      // Sin preview la TUI solo puede decir que trabaja (no hay stream).
      label: "Trabajando…",
      target: { kind: "none", presenceId: "t" },
      logoId: "claude-code",
    });
  });

  it("sin texto, el chip distingue contestar de trabajar", () => {
    // Stream vivo: contesta.
    expect(
      chip({ chat: { working: true, readyLabel: null, answering: true } }),
    ).toMatchObject({ tone: "working", label: "Contestando…" });
    // Herramientas o pensando: trabaja (y ausente el flag, igual).
    expect(
      chip({ chat: { working: true, readyLabel: null, answering: false } }),
    ).toMatchObject({ tone: "working", label: "Trabajando…" });
    expect(chip({ chat: { working: true, readyLabel: null } })).toMatchObject({
      tone: "working",
      label: "Trabajando…",
    });
    // Con preview manda el texto, aunque esté contestando.
    expect(
      chip({
        chat: { working: true, readyLabel: "voy por la mitad", answering: true },
      }),
    ).toMatchObject({ tone: "working", label: "voy por la mitad" });
    // La UI pisa los textos con su i18n.
    expect(
      chip({
        chat: { working: true, readyLabel: null, answering: true },
        workingLabel: "WORK",
        answeringLabel: "ANSWER",
      }),
    ).toMatchObject({ tone: "working", label: "ANSWER" });
    expect(
      chip({
        chat: { working: true, readyLabel: null, answering: false },
        workingLabel: "WORK",
        answeringLabel: "ANSWER",
      }),
    ).toMatchObject({ tone: "working", label: "WORK" });
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

  it("con HWND propio el destino es consola", () => {
    expect(
      chip({
        chatEnabled: false,
        presence: [
          presence({
            id: "t",
            status: "ready",
            unread: 1,
            window: { pid: 1, hwnd: 99, own: true },
          }),
        ],
      }).target,
    ).toEqual({ kind: "console", presenceId: "t" });
  });

  it("lista cada consola en vez de quedarse con una", () => {
    expect(
      agentChips({
        chat: { ...emptyChat },
        chatEnabled: false,
        pagerEnabled: true,
        presence: [
          presence({
            id: "a",
            status: "ready",
            unread: 1,
            preview: "uno",
            updatedAt: 1,
          }),
          presence({
            id: "b",
            backendId: "codex",
            status: "working",
            preview: "dos",
            updatedAt: 2,
          }),
        ],
      }).map((c) => c.id),
    ).toEqual(["b", "a"]);
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
      id: "c",
      tone: "ready",
      label: "Soy Codex",
      logoId: "codex",
    });
  });

  it("conserva una presencia ready aunque la consola viva sea otra", () => {
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
      id: "o",
      tone: "ready",
      label: "¡Holaaa!",
      target: { kind: "none", presenceId: "o" },
      logoId: "opencode",
    });
  });

  it("conserva working fresco sin HWND aunque sea de otra marca", () => {
    expect(
      chip({
        chatEnabled: false,
        consoles: ["codex"],
        now: 1_000_000,
        presence: [
          presence({
            id: "o",
            backendId: "opencode",
            status: "working",
            updatedAt: 1_000,
          }),
        ],
      }),
    ).toMatchObject({ id: "o", tone: "working", logoId: "opencode" });
  });

  it("working realmente viejo sin HWND se descarta cuando ya hay clock", () => {
    expect(
      chip({
        chatEnabled: false,
        consoles: ["codex"],
        now: 1_100_001,
        presence: [
          presence({
            id: "o",
            backendId: "opencode",
            status: "working",
            updatedAt: 1_000,
          }),
        ],
      }).tone,
    ).toBe("off");
  });

  it("sin now no aplica la poda de working", () => {
    expect(
      chip({
        chatEnabled: false,
        consoles: ["codex"],
        presence: [
          presence({
            id: "o",
            backendId: "opencode",
            status: "working",
            updatedAt: 1,
          }),
        ],
      }),
    ).toMatchObject({ id: "o", tone: "working" });
  });

  it("un working viejo con HWND se conserva", () => {
    expect(
      chip({
        chatEnabled: false,
        consoles: ["codex"],
        now: 1_100_001,
        presence: [
          presence({
            id: "o",
            backendId: "opencode",
            status: "working",
            updatedAt: 1_000,
            window: { pid: 1, hwnd: 99 },
          }),
        ],
      }),
    ).toMatchObject({ id: "o", tone: "working", target: { kind: "focus" } });
  });

  it("un working viejo con consola viva de la misma marca se conserva", () => {
    expect(
      chip({
        chatEnabled: false,
        consoles: ["opencode"],
        now: 1_100_001,
        presence: [
          presence({
            id: "o",
            backendId: "opencode",
            status: "working",
            updatedAt: 1_000,
          }),
        ],
      }),
    ).toMatchObject({ id: "o", tone: "working", logoId: "opencode" });
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
      id: "c",
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
      id: "t",
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

describe("agentChipLogos", () => {
  const aggregateState = {
    sessions: [{ backendId: "claude-code", status: "working" }],
    presence: [{ backendId: "codex", status: "working" }],
    consoles: ["opencode"],
  };

  it("no mezcla logos en un chip real sin logo y deja el fallback genérico", () => {
    expect(
      agentChipLogos({ tone: "working", logoId: null }, aggregateState, true),
    ).toEqual([]);
  });

  it("conserva un solo logo del chip real y agrega solo el dock minimizado", () => {
    expect(
      agentChipLogos({ tone: "working", logoId: "codex" }, aggregateState, true),
    ).toEqual(["codex"]);
    expect(agentChipLogos({ tone: "off", logoId: null }, aggregateState, true)).toEqual(
      ["claude-code", "codex", "opencode"],
    );
    expect(
      agentChipLogos({ tone: "off", logoId: null }, aggregateState, false),
    ).toEqual([]);
  });
});

describe("presenceIdsToDismissOnAticHide", () => {
  it("apaga la TUI de Atic, también la consola ya cerrada, no la externa", () => {
    expect(
      presenceIdsToDismissOnAticHide([
        { id: "own", backendId: "claude-code", window: { hwnd: 1, own: true } },
        { id: "pty", backendId: "codex", window: null },
        { id: "gone", backendId: "opencode", window: null },
        { id: "wt", backendId: "claude-code", window: { hwnd: 9, own: false } },
      ]),
    ).toEqual(["own", "pty", "gone"]);
  });
});

describe("logoSlots", () => {
  it("hasta tres logos se ven enteros", () => {
    expect(logoSlots(["claude"])).toEqual({ shown: ["claude"], extra: 0, cells: 1 });
    expect(logoSlots(["claude", "opencode", "codex"])).toEqual({
      shown: ["claude", "opencode", "codex"],
      extra: 0,
      cells: 3,
    });
  });

  it("con más, dos logos y un contador: nunca más de tres celdas", () => {
    // Regresión: cinco logos de 18 px se salían de la pestaña acoplada.
    const cinco = ["claude", "opencode", "codex", "cursor", "grok"];
    expect(logoSlots(cinco)).toEqual({
      shown: ["claude", "opencode"],
      extra: 3,
      cells: 3,
    });
  });

  it("sin logos ocupa igual una celda (el logo genérico)", () => {
    expect(logoSlots([]).cells).toBe(1);
  });
});

describe("un chip por sesión de chat", () => {
  const session = (over: Partial<ChatChipSession>): ChatChipSession => ({
    id: "s1",
    backendId: "claude-code",
    status: "ready",
    pending: 0,
    unread: 0,
    lastText: null,
    answering: false,
    updatedAt: 0,
    ...over,
  });

  it("cada sesión su chip, y el clic lleva a esa sesión", () => {
    const chips = agentChips({
      chat: { ...emptyChat },
      chats: [
        session({ id: "a", unread: 1, lastText: "Listo el cambio", updatedAt: 1 }),
        session({ id: "b", backendId: "codex", status: "working", updatedAt: 2 }),
        session({ id: "c", pending: 1, updatedAt: 3 }),
        session({ id: "d" }),
      ],
      chatEnabled: true,
      pagerEnabled: false,
      presence: [],
    });
    expect(chips.map((c) => c.id)).toEqual(["chat:c", "chat:b", "chat:a"]);
    expect(chips[0].tone).toBe("waiting");
    expect(chips[2].target).toEqual({ kind: "chat", session: "a" });
    expect(chips[1].logoId).toBe("codex");
  });
});
