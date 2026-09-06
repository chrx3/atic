import { describe, expect, it } from "vitest";
import {
  agentDisplayName,
  canonicalAgentCli,
  cliFromTitle,
  tabNameFromTitle,
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

describe("tabNameFromTitle", () => {
  it("no ve un nombre donde solo esta la marca del CLI", () => {
    expect(tabNameFromTitle("Grok")).toBeNull();
    expect(tabNameFromTitle("Claude Code")).toBeNull();
    expect(tabNameFromTitle("   ")).toBeNull();
  });

  it("saca el nombre que el TUI agrego detras de su marca", () => {
    expect(tabNameFromTitle("Codex - revisor")).toBe("revisor");
    expect(tabNameFromTitle("Claude Code · refactor")).toBe("refactor");
  });

  it("acepta un nombre que se parece a otro CLI", () => {
    // Renombrar la conversacion a «agy» es legitimo aunque sea un alias.
    expect(tabNameFromTitle("Grok · agy")).toBe("agy");
  });

  it("ignora el cwd, que la pestana ya muestra aparte", () => {
    expect(tabNameFromTitle("Claude Code · ~/Downloads")).toBeNull();
    expect(tabNameFromTitle("Codex · C:\\repo")).toBeNull();
    expect(tabNameFromTitle("grok · /home/lenovo/src")).toBeNull();
  });

  it("ignora una frase larga, que no es un nombre", () => {
    const frase = "esto es una linea de estado larguisima que no nombra nada";
    expect(tabNameFromTitle(`Grok · ${frase}`)).toBeNull();
  });
});

describe("agentDisplayName", () => {
  it("usa el nombre del catálogo", () => {
    expect(agentDisplayName("codex")).toBe("Codex");
  });
});
