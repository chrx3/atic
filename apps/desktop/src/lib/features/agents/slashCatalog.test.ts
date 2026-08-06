import { describe, expect, it } from "vitest";
import {
  CLAUDE_CODE_FALLBACK_COMMANDS,
  mergeSlashCommands,
  resolveSlashCommands,
  skillsAsCommands,
} from "./slashCatalog";

describe("slashCatalog", () => {
  it("skillsAsCommands mapea name y description", () => {
    expect(
      skillsAsCommands([
        {
          name: "find-skills",
          description: "Descubre skills",
          path: "/x",
          scope: "user",
        },
      ]),
    ).toEqual([
      {
        name: "find-skills",
        description: "Descubre skills",
        argumentHint: "",
      },
    ]);
  });

  it("sin live une cache, skills y fallback", () => {
    const out = resolveSlashCommands(
      null,
      [{ name: "help", description: "cache", argumentHint: "" }],
      [{ name: "find-skills", description: "skill", argumentHint: "" }],
    );
    const names = out.map((c) => c.name);
    expect(names).toContain("help");
    expect(names).toContain("find-skills");
    expect(names).toContain("compact");
    expect(out.find((c) => c.name === "help")?.description).toBe("cache");
  });

  it("live gana y se enriquece con skills", () => {
    const out = resolveSlashCommands(
      [{ name: "find-skills", description: "", argumentHint: "" }],
      null,
      [{ name: "find-skills", description: "rica", argumentHint: "" }],
    );
    expect(out).toHaveLength(1);
    expect(out[0]?.description).toBe("rica");
  });

  it("merge: posterior pisa si trae texto; si no, conserva", () => {
    const out = mergeSlashCommands(
      [{ name: "a", description: "base", argumentHint: "" }],
      [{ name: "a", description: "", argumentHint: "[x]" }],
      [{ name: "a", description: "nueva", argumentHint: "" }],
    );
    expect(out[0]).toEqual({
      name: "a",
      description: "nueva",
      argumentHint: "[x]",
    });
  });

  it("fallback por defecto no está vacío", () => {
    expect(CLAUDE_CODE_FALLBACK_COMMANDS.length).toBeGreaterThan(5);
  });
});
