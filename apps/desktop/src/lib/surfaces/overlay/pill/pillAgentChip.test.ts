import { describe, expect, it } from "vitest";
import type { PresenceView } from "$lib/agentPresenceReduce";
import { agentChip, type ChipTone } from "./pillAgentChip";

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
  chat?: Partial<typeof emptyChat> & { providerSessions?: Array<string | null> };
  presence?: PresenceView[];
  chatEnabled?: boolean;
  pagerEnabled?: boolean;
}) {
  return agentChip({
    chat: { ...emptyChat, ...opts.chat },
    presence: opts.presence ?? [],
    chatEnabled: opts.chatEnabled ?? true,
    pagerEnabled: opts.pagerEnabled ?? true,
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
    ).toEqual({ tone: "waiting", label: "permiso", target: { kind: "console" } });

    expect(
      chip({
        chat: { working: true },
        presence: [presence({ id: "t", status: "ready", unread: 1 })],
      }).tone,
    ).toBe("working");

    expect(
      chip({
        chat: { unread: 1, readyLabel: "desde el chat" },
        presence: [
          presence({ id: "t", status: "ready", unread: 1, preview: "desde la tui" }),
        ],
      }),
    ).toEqual({
      tone: "ready",
      label: "desde el chat",
      target: { kind: "console" },
    });
  });

  it("sin HWND el destino es none, no la consola", () => {
    const result = chip({
      chatEnabled: false,
      presence: [presence({ id: "t", status: "working" })],
    });
    expect(result).toEqual({
      tone: "working",
      label: null,
      target: { kind: "none", presenceId: "t" },
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
});
