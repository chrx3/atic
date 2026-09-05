import { describe, expect, it } from "vitest";
import {
  agentDisplayName,
  canonicalAgentCli,
  cliFromTitle,
  restartCliFromOutput,
} from "./consoleAgent";

describe("canonicalAgentCli", () => {
  it("acepta exe, alias y el nombre de la marca", () => {
    expect(canonicalAgentCli("codex.exe")).toBe("codex");
    expect(canonicalAgentCli("C:\\\\bin\\\\Claude.exe")).toBe("claude");
    expect(canonicalAgentCli("cursor-agent")).toBe("cursor-agent");
    expect(canonicalAgentCli("Claude Code")).toBe("claude");
    expect(canonicalAgentCli("powershell")).toBeNull();
  });
});

describe("restartCliFromOutput", () => {
  it("lee el pedido de reinicio de Codex", () => {
    expect(
      restartCliFromOutput(
        "Update ran successfully! Please restart Codex.\r\nC:\\\\Users\\\\Lenovo\\\\Downloads>",
        "codex",
      ),
    ).toBe("codex");
  });

  it("lee Please restart Claude Code", () => {
    expect(restartCliFromOutput("Please restart Claude Code", null)).toBe("claude");
  });

  it("con solo el ok del updater usa el CLI de la pestaña", () => {
    expect(restartCliFromOutput("update ran successfully", "opencode")).toBe(
      "opencode",
    );
  });

  it("ignora un please restart genérico", () => {
    expect(restartCliFromOutput("Please restart your computer", "codex")).toBeNull();
  });
});

describe("cliFromTitle", () => {
  it("toma la marca aunque el TUI agregue el cwd", () => {
    expect(cliFromTitle("Codex")).toBe("codex");
    expect(cliFromTitle("Claude Code · Downloads")).toBe("claude");
  });
});

describe("agentDisplayName", () => {
  it("usa el nombre del catálogo", () => {
    expect(agentDisplayName("codex")).toBe("Codex");
  });
});
