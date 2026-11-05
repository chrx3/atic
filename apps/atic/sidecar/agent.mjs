// la referencia · proceso del agente.
// Un único proceso Node, siempre vivo, que maneja todas las conversaciones con el Claude Agent SDK.
// Usa el binario oficial de Claude Code instalado (ATIC_CLAUDE_PATH) con la sesión del usuario:
// este proceso nunca lee ni guarda credenciales.
//
// Protocolo con Rust (una línea JSON por mensaje):
//   → { id, method, params }            petición
//   ← { id, result } | { id, error }    respuesta
//   ← { event, key?, data, seq? }       evento (streaming, permisos, uso…)
//
// Dos formas de correr:
// - Sin argumentos: por stdin/stdout, y muere con quien lo lanzó (como antes).
// - Con `--daemon`: un proceso propio que sobrevive a la pill (cerrarla o actualizarla no corta
//   a los agentes). Escucha en 127.0.0.1 con un puerto libre y deja en ATIC_SIDECAR_DISCOVERY
//   `{ port, token, pid, version }`. Cada cliente («code», «notch») abre con
//   `{ hello: { token, client, since, persist } }` y recibe `{ hello: { ok, version, pid, seq,
//   fresh, sessions, alive } }`: con `since` (el último `seq` que vio) se le reenvía lo que se
//   perdió; sin él, una foto de sus sesiones vivas (historial, permisos pendientes, lo que va
//   del mensaje en curso) para rearmar los chats. Los eventos de un cliente desconectado quedan
//   en el registro; sus sesiones siguen trabajando.

import {
  query,
  listSessions,
  getSessionMessages,
  getSessionInfo,
  renameSession,
  deleteSession,
} from "@anthropic-ai/claude-agent-sdk";
import readline from "node:readline";
import net from "node:net";
import { randomBytes } from "node:crypto";
import { open as openFile } from "node:fs/promises";
import { appendFileSync, mkdirSync, readFileSync, realpathSync, statSync, unlinkSync, watch, writeFileSync } from "node:fs";
import { execFile, spawn } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";

const CLAUDE = process.env.ATIC_CLAUDE_PATH || undefined;
const sessions = new Map(); // key → Session

const DAEMON = process.argv.includes("--daemon");
const VERSION = process.env.ATIC_SIDECAR_VERSION || "dev";
const DISCOVERY = process.env.ATIC_SIDECAR_DISCOVERY || null;

// El registro del proceso aparte (`code-agent.log`, junto al archivo de descubrimiento): quién se
// conecta, qué sesiones abren y cierran, y por qué se retira o se cierra. Sin él, una conversación
// que se cortó no deja rastro. Se vacía al pasar 1 MB.
const LOG = DISCOVERY ? join(DISCOVERY, "..", "code-agent.log") : null;
function dlog(text) {
  if (!LOG) return;
  try {
    if (statSync(LOG, { throwIfNoEntry: false })?.size > 1_000_000) writeFileSync(LOG, "");
    appendFileSync(LOG, `${new Date().toISOString()} [${process.pid}] ${text}\n`);
  } catch {
    /* sin registro no se detiene nada */
  }
}
// Sin clientes ni nada trabajando, el proceso se cierra solo pasado este rato.
const IDLE_EXIT_MS = Number(process.env.ATIC_SIDECAR_IDLE_MS) || 10 * 60_000;
// Lo último emitido, para reenviárselo a un cliente que se reconecta (`since`).
const RING_MAX = 5000;

const out = (msg) => process.stdout.write(JSON.stringify(msg) + "\n");
const write = (socket, msg) => {
  if (!socket.destroyed) socket.write(JSON.stringify(msg) + "\n");
};

let seq = 0;
const ring = []; // { seq, client, msg }
const clients = new Map(); // nombre → { name, socket, persist, holding, queue }

// Un evento va al cliente dueño de la sesión (o a todos si no es de ninguna) y queda en el
// registro con su número; si el cliente no está, lo verá en la foto al reconectarse.
function deliver(client, msg) {
  if (!DAEMON) return out(msg);
  msg.seq = ++seq;
  ring.push({ seq: msg.seq, client, msg });
  if (ring.length > RING_MAX) ring.splice(0, ring.length - RING_MAX);
  const targets = client ? [clients.get(client)] : [...clients.values()];
  for (const c of targets) {
    if (!c) continue;
    if (c.holding) c.queue.push(msg);
    else write(c.socket, msg);
  }
}

const emit = (event, key, data) => {
  const s = key ? sessions.get(key) : null;
  if (s) note(s, event, data);
  deliver(s?.client ?? null, { event, key, data });
};

// Lo que hace falta para rearmar un chat sin haber visto sus eventos: lo último de `meta`,
// `init` y `remote`, los permisos que esperan respuesta y lo que va del mensaje en curso.
function note(s, event, data) {
  if (event === "meta" || event === "init" || event === "remote") s.sticky[event] = data;
  else if (event === "permission") s.permData.set(data.requestId, data);
  else if (event === "stream") {
    for (const op of data) {
      if (op.op === "message" && !op.parent) s.inflight = [];
      if (s.inflight.length < 4000) s.inflight.push(op);
    }
  } else if (event === "system") noteTask(s, data);
  else if (event === "assistant" && !data.parent) s.inflight = [];
  else if (event === "result") {
    s.inflight = [];
    // Terminó mientras la pill no estaba: al volver se marca «sin leer».
    if (DAEMON && !clients.has(s.client)) s.unseen = true;
  }
}

// Lo que muestra el panel «Agentes»: sin esto, una pill que se reconecta ve 0 subagentes
// aunque sigan trabajando.
const MAX_TASKS = 200;
function noteTask(s, data) {
  const sub = data?.subtype;
  if (sub === "background_tasks_changed") {
    s.bgTasks = data;
    return;
  }
  if (sub !== "task_started" && sub !== "task_progress" && sub !== "task_notification") return;
  const id = data.task_id;
  if (!id) return;
  const task = s.tasks.get(id) ?? {};
  if (sub === "task_started") task.started = data;
  else task.last = data;
  s.tasks.delete(id);
  s.tasks.set(id, task);
  if (s.tasks.size > MAX_TASKS) s.tasks.delete(s.tasks.keys().next().value);
}

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

/* ─────────── Velocidad: tokens por segundo de escritura ─────────── */

// Un mensaje con menos tokens escribe en un instante y dispararía el promedio.
const SPEED_MIN_TOKENS = 10;

// Mide cada mensaje del hilo principal entre su primer y su último token, aparte de la
// espera hasta el primero. Los subagentes corren en paralelo y no entran.
function makeSpeed(now = () => performance.now()) {
  let msg = null;
  let total = { tokens: 0, genMs: 0, ttftMs: 0, messages: 0 };
  return {
    start() {
      msg = { start: now(), first: null, tokens: 0 };
    },
    delta() {
      if (msg && msg.first === null) msg.first = now();
    },
    usage(outputTokens) {
      if (msg && typeof outputTokens === "number") msg.tokens = outputTokens;
    },
    stop() {
      const end = now();
      if (msg && msg.first !== null && msg.tokens >= SPEED_MIN_TOKENS && end > msg.first) {
        total.tokens += msg.tokens;
        total.genMs += end - msg.first;
        total.ttftMs += msg.first - msg.start;
        total.messages += 1;
      }
      msg = null;
    },
    // Lo del turno, y vuelve a cero para el siguiente.
    take() {
      const t = total;
      msg = null;
      total = { tokens: 0, genMs: 0, ttftMs: 0, messages: 0 };
      if (!t.messages) return null;
      return { tokens: t.tokens, genMs: Math.round(t.genMs), ttftMs: Math.round(t.ttftMs / t.messages) };
    },
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
// Cuánto puede tardar el turno que Claude Code abre solo tras avisar que terminó una tarea.
const AUTO_TURN_MS = 15_000;

function startSession(params, ctx) {
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
    // Sin esto el SDK anota la sesión como `sdk-ts` y el `/resume` de Claude Code la esconde.
    env: { ...process.env, CLAUDE_CODE_ENTRYPOINT: "atic-code" },
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
  // `mcpServers` y `allowedTools`: el chat del editor de grabaciones registra su servidor MCP y lo deja pasar.
  for (const k of ["model", "effort", "permissionMode", "resume", "resumeSessionAt", "forkSession", "additionalDirectories", "settings", "mcpServers", "allowedTools"]) {
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
  const session = {
    key,
    cwd,
    q,
    input,
    buffer,
    permissions,
    sessionId: params.resume ?? null,
    known: new Set(),
    busy: false,
    speed: makeSpeed(),
    // De qué cliente es (los eventos van solo a él) y lo que guarda para rearmarlo.
    client: ctx?.client ?? null,
    sticky: {},
    permData: new Map(),
    inflight: [],
    unseen: false,
    // Subagentes y tareas en segundo plano: su inicio y su último aviso, por id de tarea.
    tasks: new Map(),
    bgTasks: null,
  };
  sessions.set(key, session);
  dlog(`sesión ${key} abierta (cliente ${session.client ?? "-"}${params.resume ? ", retomada" : ""})`);
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
      dlog(`sesión ${key} terminó (el proceso de Claude Code salió)`);
    } catch (e) {
      if (sessions.get(key) === session) emit("closed", key, { error: String(e?.message ?? e) });
      dlog(`sesión ${key} terminó con error: ${e?.message ?? e}`);
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
      // La hora se toma aquí, antes del lote de 30 ms del buffer.
      if (!m.parent_tool_use_id) {
        if (ev.type === "message_start") s.speed.start();
        else if (ev.type === "content_block_delta") s.speed.delta();
        else if (ev.type === "message_delta") s.speed.usage(ev.usage?.output_tokens);
        else if (ev.type === "message_stop") s.speed.stop();
      }
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
        durationApiMs: m.duration_api_ms,
        numTurns: m.num_turns,
        costUsd: m.total_cost_usd,
        usage: m.usage,
        modelUsage: m.modelUsage,
        errors: m.errors,
        speed: s.speed.take(),
      });
      return;
    case "rate_limit_event":
      emit("rate_limit", key, m.rate_limit_info);
      return;
    case "system":
      buffer.flush();
      if (m.subtype === "init") {
        // Terminó una tarea en segundo plano con el chat en reposo: Claude Code abre un turno solo
        // (sin mensaje del usuario) para contarlo. Se marca ya, antes del primer token, para que la
        // interfaz muestre «Trabajando…», encole lo que se escriba y no lo tome por un cambio externo.
        if (!s.busy && s.autoTurn && Date.now() - s.autoTurn < AUTO_TURN_MS) {
          s.busy = true;
          emit("turn", key, { auto: true });
        }
        s.autoTurn = 0;
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
      } else {
        if (m.subtype === "task_notification" && !m.skip_transcript && !m.ambient && !s.busy) s.autoTurn = Date.now();
        emit("system", key, m);
      }
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
          if ((r.type === "user" || r.type === "assistant") && r.timestamp && !r.isSidechain) return { time: Date.parse(r.timestamp), state: endState(r) };
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

/** En qué quedó la conversación según su último mensaje: `done`, `asking` (terminó preguntando) o `stopped`. */
function endState(entry) {
  const content = entry.message?.content;
  if (entry.type === "assistant") {
    const blocks = Array.isArray(content) ? content : [{ type: "text", text: String(content ?? "") }];
    const last = blocks.filter((b) => b?.type === "text" || b?.type === "tool_use").at(-1);
    if (!last || last.type === "tool_use") return "stopped";
    return /\?[*_`)"»”]*\s*$/.test(last.text ?? "") ? "asking" : "done";
  }
  // Lo último es tuyo (o el resultado de una herramienta): el turno no llegó a cerrarse.
  return "stopped";
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
      const last = await lastMessageTime(join(projectDir(s.cwd || dir), `${s.sessionId}.jsonl`));
      if (last?.time) s.lastModified = last.time;
      if (last?.state) s.endState = last.state;
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
  // En medio de un turno propio (o del que se abre tras una tarea en segundo plano): se revisa al terminar.
  if (s.busy || s.autoTurn) return;
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
      dlog(`sesión ${key} cerrada (cliente ${s.client ?? "-"}, trabajando=${s.busy}, segundo plano=${backgroundRunning(s)})`);
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
  // Cerrar el proceso (Claude Code recién instalado, por ejemplo); con algo trabajando, no.
  shutdown: ({ force } = {}) => {
    if (!DAEMON) return { ok: false };
    if (!force && working()) return { ok: false, busy: true };
    setTimeout(quit, 100);
    return { ok: true };
  },
  // Una pill de otra versión: este proceso se reemplaza apenas no haya nada trabajando.
  retire: () => {
    if (!retiring) dlog(`retiro pedido por una pill de otra versión (trabajando=${working()})`);
    retiring = true;
    return { ok: true };
  },
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

async function handle(req, ctx, reply) {
  const fn = methods[req.method];
  try {
    if (!fn) throw new Error(`Método desconocido: ${req.method}`);
    const result = await fn(req.params ?? {}, ctx);
    reply({ id: req.id, result: result ?? null });
  } catch (e) {
    reply({ id: req.id, error: String(e?.message ?? e) });
  }
}

/* ─────────── Proceso propio (`--daemon`) ─────────── */

// Algo que no se puede cortar: un turno en curso o un permiso esperando respuesta.
// Algo trabaja: un turno, un permiso que espera o un subagente o tarea en segundo plano. Las
// tareas cuentan aunque el turno haya terminado: retirarse o cerrarse por inactividad las mataría.
const backgroundRunning = (s) =>
  (s.bgTasks?.tasks ?? []).some((t) => !t.ambient) || [...s.tasks.values()].some((t) => t.started && t.last?.subtype !== "task_notification");
const working = () => [...sessions.values()].some((s) => s.busy || s.permissions.size > 0 || backgroundRunning(s));

let retiring = false;
let idleSince = null;

function closeAll(announce) {
  for (const s of [...sessions.values()]) {
    // `close` no avisa (borra la sesión antes de que termine su lector): se avisa aquí para que
    // la pill la deje como cerrada y la retome al escribirle, sin mostrar un error.
    if (announce) emit("closed", s.key, {});
    methods.close({ key: s.key });
  }
}

function dropDiscovery() {
  if (!DISCOVERY) return;
  try {
    if (JSON.parse(readFileSync(DISCOVERY, "utf8")).pid === process.pid) unlinkSync(DISCOVERY);
  } catch {
    /* ya no está o es de otro proceso */
  }
}

function quit() {
  dlog(`se cierra (retiro=${retiring}, sesiones=${sessions.size})`);
  dropDiscovery();
  closeAll(true);
  setTimeout(() => process.exit(0), 300);
}

// Revisa cada tanto: reemplazarse si lo pidió una pill más nueva, o cerrarse si quedó solo.
function tick() {
  const busy = working();
  if (retiring && !busy) {
    // Sin el archivo, la próxima petición de la pill levanta el proceso nuevo.
    dropDiscovery();
    emit("retire", null, {});
    return quit();
  }
  if (clients.size || busy) {
    idleSince = null;
    return;
  }
  idleSince ??= Date.now();
  if (Date.now() - idleSince >= IDLE_EXIT_MS) quit();
}

// Las sesiones de un cliente, con lo necesario para rearmar sus chats.
async function snapshot(client) {
  const list = [...sessions.values()].filter((s) => s.client === client);
  return Promise.all(
    list.map(async (s) => {
      let messages = [];
      let title = null;
      if (s.sessionId) {
        try {
          messages = await getSessionMessages(s.sessionId, { dir: s.cwd });
        } catch {
          /* todavía sin archivo */
        }
        try {
          const info = await getSessionInfo(s.sessionId, { dir: s.cwd });
          title = info?.customTitle || info?.summary || null;
        } catch {
          /* sin datos */
        }
      }
      const events = [];
      for (const kind of ["meta", "init", "remote"]) if (s.sticky[kind]) events.push({ event: kind, data: s.sticky[kind] });
      for (const [id, data] of s.permData) if (s.permissions.has(id)) events.push({ event: "permission", data });
      // Las tareas antes que el mensaje en curso: su inicio y luego su último aviso.
      for (const task of s.tasks.values()) {
        if (task.started) events.push({ event: "system", data: task.started });
        if (task.last) events.push({ event: "system", data: task.last });
      }
      if (s.bgTasks) events.push({ event: "system", data: s.bgTasks });
      if (s.inflight.length) events.push({ event: "stream", data: s.inflight });
      const item = { key: s.key, sessionId: s.sessionId, cwd: s.cwd, busy: s.busy, unseen: s.unseen, title, messages, events };
      s.unseen = false;
      return item;
    }),
  );
}

// Un cliente se presentó: reemplaza a la conexión anterior con su nombre y recibe lo que se
// perdió. Mientras se arma la respuesta, lo nuevo espera en su cola para no llegar antes.
function attach(socket, hello) {
  const name = String(hello.client || "code");
  const old = clients.get(name);
  if (old && old.socket !== socket) old.socket.destroy();
  const conn = { name, socket, persist: hello.persist !== false, holding: true, queue: [] };
  dlog(`cliente ${name} conectado (since=${Number(hello.since) || 0}, sesiones=${[...sessions.values()].filter((s) => s.client === name).length})`);
  clients.set(name, conn);
  idleSince = null;
  const since = Number(hello.since) || 0;
  const first = ring.length ? ring[0].seq : seq + 1;
  const resumable = since > 0 && since >= first - 1 && since <= seq;
  const replay = resumable ? ring.filter((e) => e.seq > since && (e.client === null || e.client === name)).map((e) => e.msg) : [];
  (async () => {
    const list = resumable ? [] : await snapshot(name);
    const alive = [...sessions.values()].filter((s) => s.client === name).map((s) => s.key);
    write(socket, { hello: { ok: true, version: VERSION, pid: process.pid, seq, fresh: !resumable, sessions: list, alive } });
    for (const m of replay) write(socket, m);
    for (const m of conn.queue) write(socket, m);
    conn.queue = [];
    conn.holding = false;
  })();
  return conn;
}

function serve() {
  const token = randomBytes(24).toString("hex");
  const server = net.createServer((socket) => {
    socket.setNoDelay(true);
    socket.on("error", () => {});
    let conn = null;
    const lines = readline.createInterface({ input: socket });
    // Una pill que se cierra de golpe corta con ECONNRESET: no es un error del agente.
    lines.on("error", () => {});
    lines.on("line", (line) => {
      if (!line.trim()) return;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        return;
      }
      if (!conn) {
        if (msg.hello?.token !== token) return socket.destroy();
        conn = attach(socket, msg.hello);
        return;
      }
      void handle(msg, { client: conn.name }, (reply) => write(socket, reply));
    });
    socket.on("close", () => {
      if (!conn || clients.get(conn.name) !== conn) return;
      clients.delete(conn.name);
      dlog(`cliente ${conn.name} desconectado`);
      // Un cliente sin `persist` (el chat del notch) no deja sesiones colgando.
      if (!conn.persist) {
        for (const s of [...sessions.values()]) if (s.client === conn.name) methods.close({ key: s.key });
      }
    });
  });
  server.listen(0, "127.0.0.1", () => {
    const { port } = server.address();
    if (DISCOVERY) writeFileSync(DISCOVERY, JSON.stringify({ port, token, pid: process.pid, version: VERSION }));
    dlog(`arrancó (versión ${VERSION}, puerto ${port})`);
  });
  setInterval(tick, Math.min(30_000, IDLE_EXIT_MS));
  process.on("exit", dropDiscovery);
}

if (DAEMON) {
  serve();
} else {
  const rl = readline.createInterface({ input: process.stdin });
  rl.on("line", (line) => {
    if (!line.trim()) return;
    let req;
    try {
      req = JSON.parse(line);
    } catch {
      return;
    }
    void handle(req, { client: null }, out);
  });
  rl.on("close", () => {
    for (const s of sessions.values()) s.q.close();
    process.exit(0);
  });
  out({ event: "ready", data: { pid: process.pid } });
}

process.on("uncaughtException", (e) => emit("fatal", null, { message: String(e?.stack ?? e) }));
process.on("unhandledRejection", (e) => emit("fatal", null, { message: String(e?.stack ?? e) }));
