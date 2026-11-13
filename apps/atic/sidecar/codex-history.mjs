// Las conversaciones de Codex que abrió Atic Code, leídas de sus rollouts
// (`~/.codex/sessions/AAAA/MM/DD/rollout-…-<id>.jsonl`). Codex las guarda por su cuenta: sin
// esto, al cerrar el chat desaparecían de la lista, que solo lee las sesiones de Claude.
import { createReadStream } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import readline from "node:readline";

const sessionsRoot = () => join(process.env.CODEX_HOME || join(homedir(), ".codex"), "sessions");

/** Lo que Atic o Codex meten como mensaje del usuario sin que él lo haya escrito. */
const injected = (text) => {
  const t = text.trimStart();
  return !t || t.startsWith("<") || t.startsWith("# AGENTS.md instructions");
};

/** El texto que escribió el usuario en un mensaje del rollout, o `null` si no es suyo. */
function userText(payload) {
  const parts = (payload.content ?? []).filter((c) => c?.type === "input_text" && typeof c.text === "string" && !injected(c.text));
  return parts.length ? parts.map((c) => c.text).join("\n\n") : null;
}

function assistantText(payload) {
  const parts = (payload.content ?? []).filter((c) => c?.type === "output_text" && typeof c.text === "string" && c.text.trim());
  return parts.length ? parts.map((c) => c.text).join("\n\n") : null;
}

async function* records(file) {
  const lines = readline.createInterface({ input: createReadStream(file, { encoding: "utf8" }), crlfDelay: Infinity });
  try {
    for await (const line of lines) {
      if (!line.trim()) continue;
      try {
        yield JSON.parse(line);
      } catch {
        /* línea a medio escribir */
      }
    }
  } finally {
    lines.close();
  }
}

async function rolloutFiles(root) {
  const out = [];
  const walk = async (dir, depth) => {
    let entries;
    try {
      entries = await readdir(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const path = join(dir, entry.name);
      if (entry.isDirectory() && depth < 3) await walk(path, depth + 1);
      else if (entry.isFile() && entry.name.startsWith("rollout-") && entry.name.endsWith(".jsonl")) out.push(path);
    }
  };
  await walk(root, 0);
  return out;
}

// Lo leído de cada rollout, mientras no cambie: la lista se pide cada vez que se abre un espacio.
const summaries = new Map(); // ruta → { mtime, info }

/** La cabecera y el primer mensaje del usuario de un rollout. */
async function summarize(file) {
  let meta = null;
  for await (const r of records(file)) {
    if (r.type === "session_meta") {
      meta = r.payload ?? {};
      continue;
    }
    if (!meta) return null;
    const p = r.payload;
    if (r.type === "response_item" && p?.type === "message" && p.role === "user") {
      const text = userText(p);
      if (text) return { meta, firstPrompt: text };
    }
  }
  return meta ? { meta, firstPrompt: null } : null;
}

const samePath = (a, b) => String(a ?? "").replace(/[\\/]+$/, "") === String(b ?? "").replace(/[\\/]+$/, "");

/**
 * Las sesiones de Codex abiertas desde Atic en la carpeta `dir`, con la forma de las de Claude
 * (`sessionId`, `firstPrompt`, `lastModified`) más `agent: "codex"`.
 */
export async function listCodexSessions({ dir, limit, root = sessionsRoot() }) {
  const found = [];
  for (const file of await rolloutFiles(root)) {
    let info;
    try {
      const { mtimeMs } = await stat(file);
      const cached = summaries.get(file);
      if (cached?.mtime === mtimeMs) info = cached.info;
      else {
        const summary = await summarize(file);
        info = summary && { ...summary, lastModified: mtimeMs };
        summaries.set(file, { mtime: mtimeMs, info });
      }
    } catch {
      continue;
    }
    // Solo las de Atic (no las de la terminal ni VS Code) y con algo escrito por el usuario.
    if (!info?.firstPrompt || info.meta.originator !== "atic" || !samePath(info.meta.cwd, dir) || !info.meta.id) continue;
    found.push({ sessionId: info.meta.id, firstPrompt: info.firstPrompt, lastModified: info.lastModified, cwd: info.meta.cwd, agent: "codex" });
  }
  found.sort((a, b) => b.lastModified - a.lastModified);
  return limit ? found.slice(0, limit) : found;
}

/** El rollout de la sesión `id`, si existe. */
async function rolloutOf(id, root) {
  const files = await rolloutFiles(root);
  return files.find((f) => f.endsWith(`-${id}.jsonl`)) ?? null;
}

/**
 * Los mensajes de una sesión de Codex con la forma de `getSessionMessages` de Claude: solo los
 * textos del usuario y las respuestas (las herramientas no se reconstruyen).
 */
export async function codexSessionMessages({ sessionId, root = sessionsRoot() }) {
  const file = await rolloutOf(sessionId, root);
  if (!file) throw new Error(`No se encontró la sesión de Codex ${sessionId}`);
  const out = [];
  let n = 0;
  for await (const r of records(file)) {
    const p = r.payload;
    if (r.type !== "response_item" || p?.type !== "message") continue;
    const uuid = p.id || `${sessionId}-${n}`;
    n += 1;
    if (p.role === "user") {
      const text = userText(p);
      if (text) out.push({ type: "user", uuid, parent_tool_use_id: null, message: { role: "user", content: [{ type: "text", text }] } });
    } else if (p.role === "assistant") {
      const text = assistantText(p);
      if (text) out.push({ type: "assistant", uuid, parent_tool_use_id: null, message: { id: uuid, role: "assistant", content: [{ type: "text", text }] } });
    }
  }
  return out;
}
