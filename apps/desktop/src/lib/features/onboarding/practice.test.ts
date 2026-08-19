import { describe, expect, it } from "vitest";
import { PRACTICE_STEPS, SETUP_SHORTCUTS } from "./practice";

describe("practice", () => {
  it("cubre rueda, dictado y portapapeles una sola vez", () => {
    const ids = PRACTICE_STEPS.map((step) => step.id);
    expect(ids).toEqual(["wheel", "dictation", "clipboard"]);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("usa los mismos atajos que se configuran en el setup", () => {
    const setup = SETUP_SHORTCUTS.map((item) => item.key);
    const practice = PRACTICE_STEPS.map((step) => step.shortcutKey);
    expect(practice).toEqual(setup);
  });
});
