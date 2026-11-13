// Las conversaciones de Codex abiertas desde Atic que se suman a la lista de Atic Code.
// Correr con `node --test` dentro de sidecar/.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { codexSessionMessages, listCodexSessions } from "./codex-history.mjs";

const msg = (role, type, ...texts) => ({ type: "response_item", payload: { type: "message", role, content: texts.map((text) => ({ type, text })) } });

function rollout(root, id, { originator = "atic", cwd = "/repo", records = [], when = 0 } = {}) {
  const dir = join(root, "2026", "11", "12");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `rollout-2026-11-12T20-17-06-${id}.jsonl`);
  const lines = [{ type: "session_meta", payload: { id, cwd, originator } }, ...records];
  writeFileSync(file, lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
  if (when) utimesSync(file, when, when);
  return file;
}

test("lista solo las de Atic en la carpeta, con el primer mensaje del usuario", async () => {
  const root = mkdtempSync(join(tmpdir(), "atic-codex-"));
  const chat = [
    msg("developer", "input_text", "You are Codex"),
    msg("user", "input_text", "# AGENTS.md instructions for /repo", "<environment_context>…</environment_context>"),
    msg("user", "input_text", "<instrucciones_de_atic>…"),
    msg("user", "input_text", "armemos el workflow para subir a prod"),
    msg("assistant", "output_text", "Listo"),
  ];
  rollout(root, "a1", { records: chat, when: 2000 });
  rollout(root, "a2", { records: [msg("user", "input_text", "otro")], when: 3000 });
  rollout(root, "b1", { originator: "codex_cli_rs", records: chat });
  rollout(root, "c1", { cwd: "/otra", records: chat });
  rollout(root, "d1", { records: [msg("user", "input_text", "<environment_context/>")] });

  const list = await listCodexSessions({ dir: "/repo/", limit: 40, root });
  assert.deepEqual(
    list.map((s) => [s.sessionId, s.firstPrompt, s.agent]),
    [
      ["a2", "otro", "codex"],
      ["a1", "armemos el workflow para subir a prod", "codex"],
    ],
  );
  assert.equal((await listCodexSessions({ dir: "/repo", limit: 1, root })).length, 1);
});

test("los mensajes salen con la forma de los de Claude, sin lo inyectado", async () => {
  const root = mkdtempSync(join(tmpdir(), "atic-codex-"));
  rollout(root, "a1", {
    records: [
      msg("user", "input_text", "# AGENTS.md instructions for /repo"),
      msg("user", "input_text", "hola"),
      { type: "response_item", payload: { type: "function_call", name: "exec" } },
      msg("assistant", "output_text", "Hola, ¿qué hacemos?"),
    ],
  });
  const messages = await codexSessionMessages({ sessionId: "a1", root });
  assert.deepEqual(
    messages.map((m) => [m.type, m.message.content[0].text]),
    [
      ["user", "hola"],
      ["assistant", "Hola, ¿qué hacemos?"],
    ],
  );
  assert.ok(messages.every((m) => m.uuid));
  await assert.rejects(codexSessionMessages({ sessionId: "no-existe", root }));
});
