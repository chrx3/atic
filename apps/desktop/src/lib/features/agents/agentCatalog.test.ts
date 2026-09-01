import { describe, expect, it } from "vitest";
import configRs from "../../../../../../crates/core/src/config.rs?raw";
import { AGENTS, isAgentShown, shownAgents } from "./agentCatalog";

describe("shownAgents", () => {
  it("sin configurar se ven todos", () => {
    expect(shownAgents([])).toEqual(AGENTS);
  });

  it("respeta el orden del catálogo y no el de la elección", () => {
    const picked = shownAgents(["cursor-agent", "claude"]);
    expect(picked.map((a) => a.cli)).toEqual(["claude", "cursor-agent"]);
  });

  it("un id que ya no existe no deja un hueco", () => {
    expect(shownAgents(["claude", "un-agente-que-ya-no-esta"]).map((a) => a.cli)).toEqual(["claude"]);
  });

  it("dejar fuera a todos devuelve todos: una grilla vacía no tiene salida", () => {
    expect(shownAgents(["un-agente-que-ya-no-esta"])).toEqual(AGENTS);
  });

  it("isAgentShown sigue la misma regla", () => {
    expect(isAgentShown("codex", [])).toBe(true);
    expect(isAgentShown("codex", ["claude"])).toBe(false);
    expect(isAgentShown("codex", ["claude", "codex"])).toBe(true);
  });
});

/**
 * `AGENT_CLIS` en Rust valida lo que se guarda en la config; este catalogo es
 * lo que se ofrece. Si se separan, el agente nuevo aparece en la grilla y su
 * interruptor de Ajustes no guarda nada: falla en silencio, que es justo lo
 * que este proyecto prueba leyendo el Rust de verdad (ver `contract.test.ts`).
 */
describe("gemelo con Rust", () => {
  it("la lista de ids es la misma que valida la config", () => {
    const match = /const\s+AGENT_CLIS\s*:\s*&\[&str\]\s*=\s*&\[([^\]]*)\]/.exec(
      configRs,
    );
    if (!match) throw new Error("No se encontró `const AGENT_CLIS`. ¿Lo renombraron?");
    const ids = [...match[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(ids).toEqual(AGENTS.map((agent) => agent.cli));
  });
});
