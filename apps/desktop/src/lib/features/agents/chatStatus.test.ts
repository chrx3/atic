import { describe, expect, it } from "vitest";
import { chatTabStatus } from "./chatStatus";

const base = { status: "ready", pending: [] as unknown[], unread: 0 };

describe("chatTabStatus", () => {
  it("sin sesión, terminada", () => {
    expect(chatTabStatus(undefined)).toBe("gone");
  });

  it("un permiso pendiente gana aunque esté trabajando", () => {
    expect(chatTabStatus({ ...base, status: "working", pending: [{}] })).toBe(
      "waiting",
    );
  });

  it("trabajando, con error, con respuesta sin leer, o lista", () => {
    expect(chatTabStatus({ ...base, status: "working" })).toBe("working");
    expect(chatTabStatus({ ...base, status: "failed", unread: 2 })).toBe("failed");
    expect(chatTabStatus({ ...base, unread: 1 })).toBe("unread");
    expect(chatTabStatus(base)).toBe("ready");
  });
});

describe("un chat recién abierto no está trabajando", () => {
  const hooks = { items: [{ kind: "notice" }] };
  const asked = { items: [{ kind: "message", role: "user" }] };

  it("los hooks de arranque no cuentan como trabajo", () => {
    expect(chatTabStatus({ ...base, status: "working", turns: [hooks] })).toBe("ready");
  });

  it("con un mensaje tuyo, sí", () => {
    expect(chatTabStatus({ ...base, status: "working", turns: [hooks, asked] })).toBe(
      "working",
    );
  });
});
