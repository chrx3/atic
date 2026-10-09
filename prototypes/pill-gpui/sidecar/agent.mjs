// la referencia · proceso del agente.
// Un único proceso Node, siempre vivo, que maneja todas las conversaciones con el Claude Agent SDK.
// Usa el binario oficial de Claude Code instalado (ATIC_CLAUDE_PATH) con la sesión del usuario:
// este proceso nunca lee ni guarda credenciales.
//
// Protocolo con Rust (stdin/stdout, una línea JSON por mensaje):
//   → { id, method, params }            petición
//   ← { id, result } | { id, error }    respuesta
//   ← { event, key?, data }             evento (streaming, permisos, uso…)

import {
  query,
  listSessions,
  getSessionMessages,
  getSessionInfo,
  renameSession,
  deleteSession,
} from "@anthropic-ai/claude-agent-sdk";
import readline from "node:readline";
import { open as openFile } from "node:fs/promises";
import { mkdirSync, realpathSync, watch } from "node:fs";
import { execFile, spawn } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";

const CLAUDE = process.env.ATIC_CLAUDE_PATH || undefined;
const sessions = new Map(); // key → Session

const out = (msg) => process.stdout.write(JSON.stringify(msg) + "\n");
const emit = (event, key, data) => out({ event, key, data });

/* ─────────── Streaming: los fragmentos se agrupan para no saturar la interfaz ─────────── */

const FLUSH_MS = 30;

function makeBuffer(key) {
  let pending = [];
  let timer = null;
  const flush = () => {
    timer = null;
    if (!pending.length) return;
    const batch = pending;
    pending = [];
    emit("stream", key, batch);
  };
  return {
    push(item) {
      // Fusiona deltas consecutivos del mismo bloque en uno solo.
      const last = pending[pending.length - 1];
      if (item.op === "delta" && last && last.op === "delta" && last.msg === item.msg && last.index === item.index) {
        last.text += item.text;
      } else pending.push(item);
      if (!timer) timer = setTimeout(flush, FLUSH_MS);
    },
    flush,
  };
}

/* ─────────── Sesión ─────────── */

function streamingInput() {
  const queue = [];
  let wake = null;
  let closed = false;
  return {
    iterable: {
      async *[Symbol.asyncIterator]() {
        while (!closed) {
          if (queue.length) yield queue.shift();
          else await new Promise((r) => (wake = r));
        }
      },
    },
    push(message) {
      queue.push(message);
      wake?.();
      wake = null;
    },
    close() {
      closed = true;
      wake?.();
    },
  };
}

let permSeq = 0;

function startSession(params) {
  const { key, cwd } = params;
  // Chats sueltos: su carpeta propia se crea la primera vez.
  if (params.createCwd) mkdirSync(cwd, { recursive: true });
  if (sessions.has(key)) return { ok: true, reused: true };

  const input = streamingInput();
  const buffer = makeBuffer(key);
  const permissions = new Map(); // requestId → resolve

  const options = {
    cwd,
    pathToClaudeCodeExecutable: CLAUDE,
    includePartialMessages: true,
    enableFileCheckpointing: true,
    // Claude in Chrome se decide al iniciar la sesión (es un flag del binario).
    extraArgs: { "replay-user-messages": null, ...(params.chrome ? { chrome: null } : {}) },
    canUseTool: (toolName, toolInput, ctx) =>
      new Promise((resolve) => {
        const requestId = `p${++permSeq}`;
        permissions.set(requestId, resolve);
        ctx.signal?.addEventListener("abort", () => {
          if (permissions.delete(requestId)) {
            emit("permission_cancel", key, { requestId });
            resolve({ behavior: "deny", message: "Cancelado" });
          }
        });
        buffer.flush();
        emit("permission", key, {
          requestId,
          toolName,
          input: toolInput,
          toolUseID: ctx.toolUseID,
          title: ctx.title,
          displayName: ctx.displayName,
          description: ctx.description,
          decisionReason: ctx.decisionReason,
          blockedPath: ctx.blockedPath,
          suggestions: ctx.suggestions,
          defaultToNo: ctx.defaultToNo,
          suppressAlwaysAllowRule: ctx.suppressAlwaysAllowRule,
        });
      }),
  };
  for (const k of ["model", "effort", "permissionMode", "resume", "resumeSessionAt", "forkSession", "additionalDirectories", "settings"]) {
    if (params[k] !== undefined && params[k] !== null) options[k] = params[k];
  }
  // Como la extensión de VS Code: el modo "Omitir permisos" queda disponible para activarlo en vivo
  // (no se activa solo; el modo inicial es el que eligió el usuario).
  options.allowDangerouslySkipPermissions = true;
  // Contexto del workspace de la referencia: el system prompt de Claude Code + qué carpetas forman el proyecto.
  options.systemPrompt = params.appendSystemPrompt
    ? { type: "preset", preset: "claude_code", append: params.appendSystemPrompt }
    : { type: "preset", preset: "claude_code" };
  // Razonamiento visible (resumido), como en la extensión de VS Code; los modelos nuevos lo omiten si no se pide.
  if (params.thinking === true) options.thinking = { type: "adaptive", display: "summarized" };

  const q = query({ prompt: input.iterable, options });
  const session = { key, cwd, q, input, buffer, permissions, sessionId: params.resume ?? null, known: new Set(), busy: false };
  sessions.set(key, session);
  void watchSession(session);

  // Metadatos (modelos, comandos, agentes, cuenta, estilos) sin gastar ningún mensaje.
  (async () => {
    try {
      const meta = await q.initializationResult();
      emit("meta", key, meta);
      if (params.flags && Object.keys(params.flags).length) await q.applyFlagSettings(params.flags);
      if (params.remoteControl) await setRemoteControl(session, true, params.title);
    } catch (e) {
      emit("error", key, { message: String(e?.message ?? e) });
    }
  })();

  // Lector de mensajes del SDK
  (async () => {
    try {
      for await (const m of q) {
        // Un fallo al procesar un mensaje no debe cerrar la conversación: se avisa y se sigue.
        try {
          handleMessage(session, m);
        } catch (e) {
          emit("error", key, { message: String(e?.message ?? e) });
        }
      }
      // Solo si sigue siendo la sesión vigente de esa clave: al reiniciar un chat (cerrar y volver a abrir
      // con la misma clave) el cierre tardío del proceso viejo no debe tocar al nuevo.
      if (sessions.get(key) === session) emit("closed", key, {});
    } catch (e) {
      if (sessions.get(key) === session) emit("closed", key, { error: String(e?.message ?? e) });
    } finally {
      buffer.flush();
      unwatch(session);
      if (sessions.get(key) === session) sessions.delete(key);
    }
  })();

  return { ok: true };
}

// Remote Control: el ajuste remoteControlAtStartup solo deja el puente "ready" en sesiones del SDK;
// hay que pedirlo para que se conecte y la conversación aparezca en claude.ai/code y la app móvil.
// La autenticación la hace el binario de Claude Code: la referencia nunca ve credenciales.
async function setRemoteControl(s, enabled, name) {
  const r = await s.q.enableRemoteControl(enabled, name || undefined);
  emit("remote", s.key, enabled ? { on: true, url: r?.session_url ?? null } : { on: false });
  return r ?? null;
}

function handleMessage(s, m) {
  const { key, buffer } = s;
  if (m.session_id && m.session_id !== s.sessionId) {
    s.sessionId = m.session_id;
    emit("session", key, { sessionId: m.session_id });
  }
  switch (m.type) {
    case "stream_event": {
      const ev = m.event;
      const msg = m.message_id ?? s.currentMsg;
      if (ev.type === "message_start") {
        s.currentMsg = ev.message.id;
        buffer.push({ op: "message", msg: ev.message.id, parent: m.parent_tool_use_id ?? null, model: ev.message.model });
      } else if (ev.type === "content_block_start") {
        const b = ev.content_block;
        buffer.push({ op: "start", msg: s.currentMsg, index: ev.index, kind: b.type, id: b.id, name: b.name, parent: m.parent_tool_use_id ?? null });
      } else if (ev.type === "content_block_delta") {
        const d = ev.delta;
        const text = d.type === "text_delta" ? d.text : d.type === "thinking_delta" ? d.thinking : d.type === "input_json_delta" ? d.partial_json : null;
        if (text) buffer.push({ op: "delta", msg: s.currentMsg, index: ev.index, text });
      } else if (ev.type === "content_block_stop") {
        buffer.push({ op: "stop", msg: s.currentMsg, index: ev.index });
      }
      void msg;
      return;
    }
    case "assistant":
      buffer.flush();
      if (m.uuid) s.known.add(m.uuid);
      emit("assistant", key, {
        uuid: m.uuid,
        id: m.message.id,
        model: m.message.model,
        parent: m.parent_tool_use_id ?? null,
        content: m.message.content,
        error: m.error,
      });
      return;
    case "user":
      buffer.flush();
      if (m.uuid) s.known.add(m.uuid);
      // Un mensaje que no escribimos aquí (p. ej. desde claude.ai/code con Remote Control) abre un turno.
      if (!m.isReplay && !m.isSynthetic && !m.parent_tool_use_id && hasText(m.message?.content)) s.busy = true;
      emit("user", key, {
        uuid: m.uuid,
        parent: m.parent_tool_use_id ?? null,
        content: m.message?.content,
        isReplay: !!m.isReplay,
        isSynthetic: !!m.isSynthetic,
      });
      return;
    case "result":
      buffer.flush();
      s.busy = false;
      // El archivo de la sesión ya existe (en conversaciones nuevas aparece con el primer mensaje).
      void watchSession(s);
      scheduleCheck(s);
      emit("result", key, {
        subtype: m.subtype,
        isError: m.is_error,
        result: m.result,
        durationMs: m.duration_ms,
        numTurns: m.num_turns,
        costUsd: m.total_cost_usd,
        usage: m.usage,
        modelUsage: m.modelUsage,
        errors: m.errors,
      });
      return;
    case "rate_limit_event":
      emit("rate_limit", key, m.rate_limit_info);
      return;
    case "system":
      buffer.flush();
      if (m.subtype === "init") {
        emit("init", key, {
          model: m.model,
          permissionMode: m.permissionMode,
          tools: m.tools,
          mcpServers: m.mcp_servers,
          slashCommands: m.slash_commands,
          outputStyle: m.output_style,
          apiKeySource: m.apiKeySource,
          cwd: m.cwd,
          fastModeState: m.fast_mode_state,
        });
      } else emit("system", key, m);
      return;
    default:
      // Progreso de herramientas, tareas en segundo plano, hooks, avisos…
      emit("other", key, m);
  }
}

/* ─────────── Última actividad real de una sesión ─────────── */
// `lastModified` es la fecha del archivo, y Claude Code escribe metadatos al reanudar una sesión aunque
// no se envíe nada: una conversación de hace días aparecería como "ahora". Se usa la hora del último
// mensaje, leída del final del archivo.

const PROJECTS = join(process.env.CLAUDE_CONFIG_DIR || join(homedir(), ".claude"), "projects");
const projectDir = (cwd) => join(PROJECTS, cwd.replace(/[^a-zA-Z0-9]/g, "-"));

async function lastMessageTime(file) {
  let fh;
  try {
    fh = await openFile(file, "r");
    const { size } = await fh.stat();
    for (const span of [64 * 1024, 1024 * 1024]) {
      const len = Math.min(span, size);
      const buf = Buffer.alloc(len);
      await fh.read(buf, 0, len, size - len);
      const lines = buf.toString("utf8").split("\n");
      for (let i = lines.length - 1; i >= 0; i--) {
        const l = lines[i];
        if (!l.includes('"timestamp"') || !(l.includes('"type":"user"') || l.includes('"type":"assistant"'))) continue;
        try {
          const r = JSON.parse(l);
          if ((r.type === "user" || r.type === "assistant") && r.timestamp) return Date.parse(r.timestamp);
        } catch {
          /* línea cortada al inicio del bloque */
        }
      }
      if (len === size) break;
    }
  } catch {
    /* archivo no encontrado: se queda la fecha del SDK */
  } finally {
    await fh?.close();
  }
  return null;
}

/** Modelo y nivel de la última respuesta de la conversación principal (para retomarla con los mismos). */
async function lastSettings(file) {
  let fh;
  try {
    fh = await openFile(file, "r");
    const { size } = await fh.stat();
    for (const span of [256 * 1024, 4 * 1024 * 1024]) {
      const len = Math.min(span, size);
      const buf = Buffer.alloc(len);
      await fh.read(buf, 0, len, size - len);
      const lines = buf.toString("utf8").split("\n");
      for (let i = lines.length - 1; i >= 0; i--) {
        const l = lines[i];
        if (!l.includes('"type":"assistant"')) continue;
        try {
          const r = JSON.parse(l);
          const model = r.message?.model;
          if (r.type === "assistant" && !r.isSidechain && model && !model.startsWith("<")) return { model, effort: r.effort ?? null };
        } catch {
          /* línea cortada al inicio del bloque */
        }
      }
      if (len === size) break;
    }
  } catch {
    /* sin archivo */
  } finally {
    await fh?.close();
  }
  return null;
}

async function withLastActivity(list, dir) {
  await Promise.all(
    list.map(async (s) => {
      const t = await lastMessageTime(join(projectDir(s.cwd || dir), `${s.sessionId}.jsonl`));
      if (t) s.lastModified = t;
    }),
  );
  return list.sort((a, b) => b.lastModified - a.lastModified);
}

/* ─────────── Cambios hechos desde fuera (VS Code, la terminal, otra app) ─────────── */
// Otro cliente de Claude Code puede seguir la misma conversación y escribir en su archivo. Si la referencia no se
// entera, su proceso continúa desde un punto viejo y la conversación se parte en dos ramas. Se vigila el
// archivo de cada sesión viva: si aparece un mensaje que este proceso no conoce (y no está en medio de
// un turno), se avisa a la interfaz para que recargue y reconecte el chat en el punto más reciente.

const hasText = (content) =>
  typeof content === "string" ? content.trim().length > 0 : Array.isArray(content) && content.some((b) => b?.type === "text" && b.text?.trim());

/** UUID del último mensaje (usuario o Claude) de la conversación principal, leído del final del archivo. */
async function lastEntryUuid(file) {
  let fh;
  try {
    fh = await openFile(file, "r");
    const { size } = await fh.stat();
    const len = Math.min(256 * 1024, size);
    const buf = Buffer.alloc(len);
    await fh.read(buf, 0, len, size - len);
    const lines = buf.toString("utf8").split("\n");
    for (let i = lines.length - 1; i >= 0; i--) {
      const l = lines[i];
      if (!(l.includes('"type":"user"') || l.includes('"type":"assistant"'))) continue;
      try {
        const r = JSON.parse(l);
        if ((r.type === "user" || r.type === "assistant") && r.uuid && !r.isSidechain) return r.uuid;
      } catch {
        /* línea cortada */
      }
    }
  } catch {
    /* todavía no existe */
  } finally {
    await fh?.close();
  }
  return null;
}

const sessionFile = (s) => join(projectDir(s.cwd), `${s.sessionId}.jsonl`);

async function watchSession(s) {
  if (s.watcher || !s.sessionId) return;
  const file = sessionFile(s);
  const base = await lastEntryUuid(file);
  if (s.watcher || sessions.get(s.key) !== s) return;
  if (base) s.lastSeen = base; // lo que ya estaba al abrir la sesión cuenta como conocido
  try {
    s.watcher = watch(file, () => scheduleCheck(s));
  } catch {
    /* el archivo aparece con el primer mensaje: se vuelve a intentar al terminar el turno */
  }
}

function unwatch(s) {
  clearTimeout(s.checkTimer);
  s.watcher?.close();
  s.watcher = null;
}

function scheduleCheck(s) {
  clearTimeout(s.checkTimer);
  s.checkTimer = setTimeout(() => void checkExternal(s), 700);
}

async function checkExternal(s) {
  if (sessions.get(s.key) !== s || !s.sessionId) return;
  if (s.busy) return; // en medio de un turno propio: se revisa al terminar
  const u = await lastEntryUuid(sessionFile(s));
  if (!u || u === s.lastSeen || s.known.has(u)) {
    if (u) s.lastSeen = u;
    return;
  }
  s.lastSeen = u;
  emit("external", s.key, { sessionId: s.sessionId });
}

/* ─────────── Métodos ─────────── */

const need = (key) => {
  const s = sessions.get(key);
  if (!s) throw new Error(`No hay una sesión activa (${key})`);
  return s;
};

const methods = {
  ping: () => ({ pong: true, claude: CLAUDE ?? "bundled" }),
  start: startSession,
  send: ({ key, text, images }) => {
    const s = need(key);
    const content = [];
    for (const img of images ?? []) content.push({ type: "image", source: { type: "base64", media_type: img.mediaType, data: img.data } });
    content.push({ type: "text", text });
    s.busy = true;
    s.input.push({ type: "user", message: { role: "user", content }, parent_tool_use_id: null, session_id: s.sessionId ?? "" });
    return { ok: true };
  },
  interrupt: async ({ key }) => {
    const s = need(key);
    for (const [id, resolve] of s.permissions) {
      resolve({ behavior: "deny", message: "Interrumpido por el usuario", interrupt: true });
      s.permissions.delete(id);
    }
    await s.q.interrupt();
    return { ok: true };
  },
  close: ({ key }) => {
    const s = sessions.get(key);
    if (s) {
      unwatch(s);
      s.input.close();
      s.q.close();
      sessions.delete(key);
    }
    return { ok: true };
  },
  permission: ({ key, requestId, result }) => {
    const s = need(key);
    const resolve = s.permissions.get(requestId);
    if (!resolve) return { ok: false };
    s.permissions.delete(requestId);
    resolve(result);
    return { ok: true };
  },
  setModel: ({ key, model }) => need(key).q.setModel(model),
  setPermissionMode: ({ key, mode }) => need(key).q.setPermissionMode(mode),
  applyFlags: ({ key, settings }) => need(key).q.applyFlagSettings(settings),
  // Thinking en vivo: activado = razonamiento resumido visible; desactivado = sin razonamiento.
  setThinking: ({ key, on }) => (on ? need(key).q.setMaxThinkingTokens(null, "summarized") : need(key).q.setMaxThinkingTokens(0)),
  rewindFiles: ({ key, messageId, dryRun }) => need(key).q.rewindFiles(messageId, { dryRun }),
  usage: ({ key }) => need(key).q.usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET({ skipBehaviors: true }),
  context: ({ key }) => need(key).q.getContextUsage({ detail: "summary" }),
  account: ({ key }) => need(key).q.accountInfo(),
  mcpStatus: ({ key }) => need(key).q.mcpServerStatus(),
  mcpToggle: ({ key, name, enabled }) => need(key).q.toggleMcpServer(name, enabled),
  mcpReconnect: ({ key, name }) => need(key).q.reconnectMcpServer(name),
  reloadPlugins: ({ key }) => need(key).q.reloadPlugins(),
  remoteControl: ({ key, enabled, name }) => setRemoteControl(need(key), enabled, name),
  stopTask: ({ key, taskId }) => need(key).q.stopTask(taskId),
  backgroundTasks: ({ key, toolUseId }) => need(key).q.backgroundTasks(toolUseId),
  listSessions: async ({ dir, limit }) => withLastActivity(await listSessions({ dir, limit }), dir),
  deleteSession: ({ sessionId, dir }) => deleteSession(sessionId, { dir }),
  sessionMessages: ({ sessionId, dir }) => getSessionMessages(sessionId, { dir }),
  sessionInfo: ({ sessionId, dir }) => getSessionInfo(sessionId, { dir }),
  renameSession: ({ sessionId, title, dir }) => renameSession(sessionId, title, { dir }),
  sessionSettings: ({ sessionId, dir }) => lastSettings(join(projectDir(dir), `${sessionId}.jsonl`)),
  claudeInfo: () => claudeInfo(),
  claudeUpdate: () => claudeUpdate(),
};

/* ─────────── Claude Code instalado: versión y actualización ─────────── */
// Se usa el mismo binario que las conversaciones; la actualización la hace el propio `claude update`.

const claudeBin = () => CLAUDE || "claude";

function claudeVersion() {
  return new Promise((resolve) =>
    execFile(claudeBin(), ["--version"], { timeout: 15_000 }, (err, stdout) => resolve(err ? null : (/(\d+\.\d+\.\d+)/.exec(String(stdout))?.[1] ?? null))),
  );
}

async function latestVersion() {
  try {
    const r = await fetch("https://registry.npmjs.org/@anthropic-ai/claude-code/latest", { signal: AbortSignal.timeout(8000) });
    return (await r.json()).version ?? null;
  } catch {
    return null;
  }
}

async function claudeInfo() {
  const [version, latest] = await Promise.all([claudeVersion(), latestVersion()]);
  let target = null;
  try {
    target = CLAUDE ? realpathSync(CLAUDE) : null;
  } catch {
    /* sin enlace */
  }
  return { version, latest, path: CLAUDE ?? null, target };
}

let updating = null;
function claudeUpdate() {
  // Un solo `claude update` a la vez: si ya hay uno en curso se espera ese mismo.
  updating ??= new Promise((resolve) => {
    const p = spawn(claudeBin(), ["update"], { env: process.env });
    const chunk = (d) => emit("claudeUpdate", null, { text: String(d) });
    p.stdout.on("data", chunk);
    p.stderr.on("data", chunk);
    p.on("error", (e) => resolve({ ok: false, message: String(e.message) }));
    p.on("close", async (code) => resolve({ ok: code === 0, code, version: await claudeVersion() }));
  }).finally(() => (updating = null));
  return updating;
}

/* ─────────── Bucle principal ─────────── */

const rl = readline.createInterface({ input: process.stdin });
rl.on("line", async (line) => {
  if (!line.trim()) return;
  let req;
  try {
    req = JSON.parse(line);
  } catch {
    return;
  }
  const fn = methods[req.method];
  try {
    if (!fn) throw new Error(`Método desconocido: ${req.method}`);
    const result = await fn(req.params ?? {});
    out({ id: req.id, result: result ?? null });
  } catch (e) {
    out({ id: req.id, error: String(e?.message ?? e) });
  }
});
rl.on("close", () => {
  for (const s of sessions.values()) s.q.close();
  process.exit(0);
});

process.on("uncaughtException", (e) => emit("fatal", null, { message: String(e?.stack ?? e) }));
process.on("unhandledRejection", (e) => emit("fatal", null, { message: String(e?.stack ?? e) }));
out({ event: "ready", data: { pid: process.pid } });
