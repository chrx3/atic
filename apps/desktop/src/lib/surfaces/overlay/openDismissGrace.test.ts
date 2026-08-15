import { describe, expect, it } from "vitest";
import {
  armOpenDismissGrace,
  isOpenDismissGrace,
} from "./openDismissGrace";

describe("openDismissGrace", () => {
  it("cubre un instante después de abrir", () => {
    expect(isOpenDismissGrace()).toBe(false);
    armOpenDismissGrace(200);
    expect(isOpenDismissGrace()).toBe(true);
  });
});
