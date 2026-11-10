// El modo `--daemon` del agente: archivo de descubrimiento, saludo con token, peticiones,
// números de secuencia, reemplazo de la conexión de un cliente y cierre ordenado.
// Correr con `node --test` dentro de sidecar/ (no abre conversaciones: no gasta nada).
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import net from "node:net";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));

async function startDaemon(env = {}) {
  const dir = mkdtempSync(join(tmpdir(), "atic-daemon-"));
  const discovery = join(dir, "code-agent.json");
  const child = spawn(process.execPath, [join(here, "agent.mjs"), "--daemon"], {
    // El agente de mentira de estas pruebas corre con Node: se le permite por su ruta.
    env: { ...process.env, ATIC_SIDECAR_DISCOVERY: discovery, ATIC_SIDECAR_VERSION: "v-test", ATIC_SIDECAR_PROC_ALLOW: process.execPath, ...env },
    stdio: "ignore",
  });
  for (let i = 0; i < 200 && !existsSync(discovery); i++) await new Promise((r) => setTimeout(r, 50));
  const found = JSON.parse(readFileSync(discovery, "utf8"));
  return { child, discovery, found };
}

function connect(found, hello) {
  return new Promise((resolve, reject) => {
    const socket = net.connect(found.port, "127.0.0.1");
    const inbox = [];
    let waiter = null;
    const lines = readline.createInterface({ input: socket });
    lines.on("error", () => {});
    lines.on("line", (line) => {
      inbox.push(JSON.parse(line));
      waiter?.();
    });
    const next = async () => {
      while (!inbox.length) await new Promise((r) => (waiter = r));
      return inbox.shift();
    };
    // Como `next`, pero se rinde pasados `ms` (`null`) sin perder lo que llegue después.
    const poll = async (ms) => {
      const end = Date.now() + ms;
      while (!inbox.length) {
        const left = end - Date.now();
        if (left <= 0) return null;
        await new Promise((r) => {
          waiter = r;
          setTimeout(r, left);
        });
      }
      return inbox.shift();
    };
    const send = (msg) => socket.write(JSON.stringify(msg) + "\n");
    socket.on("error", reject);
    socket.on("connect", () => {
      send({ hello: { token: found.token, ...hello } });
      resolve({ socket, next, poll, send });
    });
  });
}

test("el daemon saluda, responde y se reemplaza la conexión de un cliente", async () => {
  const { child, discovery, found } = await startDaemon();
  try {
    assert.equal(found.version, "v-test");
    assert.equal(found.pid, child.pid);

    const first = await connect(found, { client: "code", since: 0 });
    const hello = (await first.next()).hello;
    assert.equal(hello.ok, true);
    assert.equal(hello.fresh, true);
    assert.deepEqual(hello.sessions, []);

    first.send({ id: 1, method: "ping", params: {} });
    const pong = await first.next();
    assert.equal(pong.id, 1);
    assert.equal(pong.result.pong, true);

    // Un segundo saludo del mismo cliente deja fuera al primero.
    const closed = new Promise((r) => first.socket.on("close", r));
    const second = await connect(found, { client: "code", since: hello.seq });
    assert.equal((await second.next()).hello.ok, true);
    await closed;

    // Con un token equivocado, corta.
    const intruder = net.connect(found.port, "127.0.0.1");
    intruder.on("error", () => {});
    intruder.write(JSON.stringify({ hello: { token: "nop", client: "code" } }) + "\n");
    await new Promise((r) => intruder.on("close", r));

    second.send({ id: 2, method: "shutdown", params: {} });
    assert.equal((await second.next()).result.ok, true);
    await new Promise((r) => child.on("exit", r));
    assert.equal(existsSync(discovery), false, "al salir borra su archivo");
  } finally {
    child.kill();
  }
});

test("una pill de otra versión lo retira apenas no hay nada trabajando", async () => {
  const { child, discovery, found } = await startDaemon({ ATIC_SIDECAR_IDLE_MS: "200" });
  try {
    const pill = await connect(found, { client: "code", since: 0 });
    assert.equal((await pill.next()).hello.version, "v-test");
    pill.send({ id: 1, method: "retire", params: {} });
    assert.equal((await pill.next()).result.ok, true);
    // Avisa a sus clientes y se va; su archivo se borra antes para que la pill levante otro.
    let event;
    do event = await pill.next();
    while (event.event !== "retire");
    await new Promise((r) => child.on("exit", r));
    assert.equal(existsSync(discovery), false);
  } finally {
    child.kill();
  }
});

test("sin clientes ni trabajo, se cierra solo", async () => {
  const { child, discovery } = await startDaemon({ ATIC_SIDECAR_IDLE_MS: "300" });
  try {
    const code = await new Promise((r) => child.on("exit", r));
    assert.equal(code, 0);
    assert.equal(existsSync(discovery), false);
  } finally {
    child.kill();
  }
});

/* ─────────── Agentes ACP que corren en el daemon (relevo) ─────────── */

// Un agente de juguete: contesta cada petición con eco; `session/prompt` avisa un trozo y
// responde después; `turn/start` (Codex) responde al tiro y avisa `turn/completed` después.
const FAKE_AGENT = `
const rl = require("readline").createInterface({ input: process.stdin });
const out = (m) => process.stdout.write(JSON.stringify(m) + "\\n");
const delay = Number(process.env.FAKE_DELAY || 400);
rl.on("line", (l) => {
  const m = JSON.parse(l);
  if (m.method === "session/prompt") {
    out({ jsonrpc: "2.0", method: "session/update", params: { update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "hola" } } } });
    setTimeout(() => out({ jsonrpc: "2.0", id: m.id, result: { stopReason: "end_turn" } }), delay);
  } else if (m.method === "turn/start") {
    out({ id: m.id, result: { turn: { id: "t1" } } });
    setTimeout(() => out({ method: "turn/completed", params: { turn: { status: "completed" } } }), delay);
  } else if (m.id !== undefined) out({ jsonrpc: "2.0", id: m.id, result: { echo: m.method } });
});
`;

const spawnParams = (key, extra = {}) => ({ key, program: process.execPath, args: ["-e", FAKE_AGENT], cwd: tmpdir(), meta: { cli: "opencode" }, ...extra });

// El siguiente mensaje que cumple `pred` (los demás se descartan).
async function until(conn, pred) {
  for (;;) {
    const msg = await conn.next();
    if (pred(msg)) return msg;
  }
}

let rpc = 100;
async function call(conn, method, params) {
  const id = ++rpc;
  conn.send({ id, method, params });
  const reply = await until(conn, (m) => m.id === id);
  if (reply.error) throw new Error(reply.error);
  return reply.result;
}

const procOut = (key, pred = () => true) => (m) => m.event === "procOut" && m.key === key && pred(JSON.parse(m.data.line));

test("un agente ACP corre en el daemon: líneas de ida y vuelta, turno en curso y salida", async () => {
  const { child, found } = await startDaemon({ ATIC_SIDECAR_IDLE_MS: "200" });
  try {
    const pill = await connect(found, { client: "code", since: 0 });
    await pill.next();
    const spawned = await call(pill, "procSpawn", spawnParams("c1", { tag: { acp: "opencode", title: "Prueba" } }));
    assert.equal(spawned.ok, true);

    pill.send({ id: 1, method: "procWrite", params: { key: "c1", line: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} }) } });
    const echo = await until(pill, procOut("c1", (m) => m.id === 1));
    assert.equal(JSON.parse(echo.data.line).result.echo, "initialize");

    // Un `session/prompt` abre el turno: el daemon trabaja hasta la respuesta con ese id.
    await call(pill, "procWrite", { key: "c1", line: JSON.stringify({ jsonrpc: "2.0", id: 7, method: "session/prompt", params: { prompt: [] } }) });
    let [listed] = await call(pill, "procList", {});
    assert.equal(listed.busy, true);
    assert.deepEqual(listed.tag, { acp: "opencode", title: "Prueba" });
    // Con un turno en curso no se retira ni se deja cerrar.
    assert.equal((await call(pill, "retire", {})).ok, true);
    assert.equal((await call(pill, "shutdown", {})).busy, true);
    await until(pill, procOut("c1", (m) => m.id === 7));
    [listed] = await call(pill, "procList", {});
    assert.equal(listed?.busy ?? false, false);
    // Terminó el turno: ahora sí se retira, y avisa que el agente se fue con él.
    await until(pill, (m) => m.event === "procExit" && m.key === "c1");
    await new Promise((r) => child.on("exit", r));
  } finally {
    child.kill();
  }
});

test("procKill avisa procExit y el turno de Codex dura hasta turn/completed", async () => {
  const { child, found } = await startDaemon();
  try {
    const pill = await connect(found, { client: "code", since: 0 });
    await pill.next();
    await call(pill, "procSpawn", spawnParams("cx", { meta: { cli: "codex" } }));
    await call(pill, "procWrite", { key: "cx", line: JSON.stringify({ id: 3, method: "turn/start", params: {} }) });
    await until(pill, procOut("cx", (m) => m.id === 3));
    // La respuesta a `turn/start` no cierra el turno: lo cierra el aviso.
    assert.equal((await call(pill, "procList", {}))[0].busy, true);
    await until(pill, procOut("cx", (m) => m.method === "turn/completed"));
    assert.equal((await call(pill, "procList", {}))[0].busy, false);

    await call(pill, "procKill", { key: "cx" });
    await until(pill, (m) => m.event === "procExit" && m.key === "cx");
    assert.deepEqual(await call(pill, "procList", {}), []);
    await assert.rejects(call(pill, "procWrite", { key: "cx", line: "{}" }));
    await call(pill, "shutdown", { force: true });
  } finally {
    child.kill();
  }
});

test("sin la pill, lo del agente se guarda: se reenvía al volver o llega en la foto", async () => {
  const { child, found } = await startDaemon();
  try {
    const first = await connect(found, { client: "code", since: 0 });
    const hello = (await first.next()).hello;
    await call(first, "procSpawn", spawnParams("c2", { tag: { delegation: { parent: "c1" } } }));
    await call(first, "procWrite", { key: "c2", line: JSON.stringify({ jsonrpc: "2.0", id: 9, method: "session/prompt", params: { prompt: [{ type: "text", text: "hola" }] } }) });
    const chunk = await until(first, procOut("c2", (m) => m.method === "session/update"));
    // La pill se cierra a mitad de turno; el agente sigue y termina sin ella.
    first.socket.destroy();
    await new Promise((r) => setTimeout(r, 700));

    // Una reconexión del mismo proceso recibe lo que se perdió.
    const back = await connect(found, { client: "code", since: chunk.seq });
    const resumed = (await back.next()).hello;
    assert.equal(resumed.fresh, false);
    assert.equal(resumed.procs[0].key, "c2");
    assert.equal(resumed.procs[0].lines, undefined, "al reanudar no hace falta la foto");
    const missed = await until(back, procOut("c2"));
    assert.equal(JSON.parse(missed.data.line).result.stopReason, "end_turn");
    back.socket.destroy();
    await new Promise((r) => setTimeout(r, 100));

    // Una pill nueva (sin `since`) recibe la foto: el tag y las líneas de ida y vuelta.
    const fresh = await connect(found, { client: "code", since: 0 });
    const snap = (await fresh.next()).hello;
    assert.equal(snap.fresh, true);
    const [agent] = snap.procs;
    assert.deepEqual(agent.tag, { delegation: { parent: "c1" } });
    assert.deepEqual(agent.meta, { cli: "opencode" });
    assert.equal(agent.busy, false);
    assert.deepEqual(agent.lines.map((l) => [l.d, JSON.parse(l.line).id ?? JSON.parse(l.line).method]), [
      ["in", 9],
      ["out", "session/update"],
      ["out", 9],
    ]);
    // Otro cliente no ve los agentes de «code».
    const notch = await connect(found, { client: "notch", since: 0 });
    assert.deepEqual((await notch.next()).hello.procs, []);
    void hello;
    await call(fresh, "shutdown", { force: true });
  } finally {
    child.kill();
  }
});

test("procSpawn solo lanza los agentes de Atic Code y el archivo de descubrimiento es del usuario", async () => {
  // Sin el permiso de las pruebas: Node (con que se corre el agente de mentira) tampoco pasa.
  const { child, discovery, found } = await startDaemon({ ATIC_SIDECAR_PROC_ALLOW: "" });
  try {
    // El archivo trae el token y solo lo lee el usuario: en Unix por su modo; en Windows, con una
    // lista de permisos de una sola entrada (la suya) que no hereda nada de la carpeta.
    if (process.platform !== "win32") assert.equal(statSync(discovery).mode & 0o777, 0o600);
    else {
      const entries = execFileSync("icacls", [discovery], { encoding: "utf8" }).split(/\r?\n/).filter((line) => line.includes(":("));
      assert.equal(entries.length, 1, `una sola entrada: ${entries.join(" | ")}`);
      assert.match(entries[0], /:\(F\)\s*$/);
      assert.doesNotMatch(entries[0], /\(I\)/);
      assert.match(entries[0].toLowerCase(), new RegExp(`\\\\${userInfo().username.toLowerCase()}:\\(f\\)`));
    }
    const pill = await connect(found, { client: "code", since: 0 });
    await pill.next();
    const refused = /solo lanza los agentes/;
    await assert.rejects(call(pill, "procSpawn", spawnParams("x1")), refused);
    await assert.rejects(call(pill, "procSpawn", { key: "x2", program: "cmd.exe", args: ["/C", "calc.exe"] }), refused);
    await assert.rejects(call(pill, "procSpawn", { key: "x3", program: "powershell", args: ["-Command", "codex app-server"] }), refused);
    // El nombre de un agente con otros argumentos, o tras un `cmd /C` con más de una orden, no pasa.
    await assert.rejects(call(pill, "procSpawn", { key: "x4", program: "codex", args: ["exec", "rm -rf ."] }), refused);
    await assert.rejects(call(pill, "procSpawn", { key: "x5", program: "cmd.exe", args: ["/C", "C:\\x\\opencode.cmd & calc.exe", "acp"] }), refused);
    await assert.rejects(call(pill, "procSpawn", { key: "x6", program: "cmd.exe", args: ["/C", "C:\\a&calc&\\opencode.cmd", "acp"] }), refused);
    assert.deepEqual(await call(pill, "procList", {}), []);
    // Un agente de verdad sí pasa el filtro: aquí no existe, y lo que falla es lanzarlo.
    const missing = join(tmpdir(), "atic-no-existe", process.platform === "win32" ? "codex.exe" : "codex");
    const launched = await call(pill, "procSpawn", { key: "x7", program: missing, args: ["app-server"] }).then(
      () => null,
      (error) => error,
    );
    assert.ok(launched === null || !refused.test(String(launched.message ?? launched)), "codex app-server pasa el filtro");
    await call(pill, "shutdown", { force: true });
  } finally {
    child.kill();
  }
});

/* ─────────── Conversaciones de Claude con un Claude Code de mentira ─────────── */

// Habla el protocolo del SDK lo justo: responde las peticiones de control y, por cada mensaje,
// abre un turno (`init`, el eco, una respuesta y `result`) y lo anota en el archivo de la sesión
// como Claude Code. Lo que llega con un turno en curso queda en su cola: al terminar el turno lo
// anota en el archivo al tiro (con su compañero `isMeta`) y abre el siguiente turno después de
// `FAKE_GAP_MS`, como los hooks y la API. El eco de un mensaje de la cola lleva otro uuid que el
// del archivo (el del adjunto `queued_command`), como en Claude Code.
const FAKE_CLAUDE = `
import readline from "node:readline";
import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
const SID = "0f0f0f0f-1111-4222-8333-444444444444";
const out = (m) => process.stdout.write(JSON.stringify(m) + "\\n");
mkdirSync(process.env.FAKE_PROJECT_DIR, { recursive: true });
const file = join(process.env.FAKE_PROJECT_DIR, SID + ".jsonl");
const log = (e) => appendFileSync(file, JSON.stringify({ ...e, sessionId: SID, timestamp: new Date().toISOString() }) + "\\n");
const TURN_MS = Number(process.env.FAKE_TURN_MS || 500);
const GAP_MS = Number(process.env.FAKE_GAP_MS || 1500);
let running = false;
let last = null;
const queue = [];
const write = (message) => {
  const uuid = randomUUID();
  log({ type: "user", uuid, parentUuid: last, message });
  log({ type: "user", uuid: randomUUID(), parentUuid: uuid, isMeta: true, message: { role: "user", content: [{ type: "text", text: "[meta]" }] } });
  last = uuid;
  return uuid;
};
function turn(prompts) {
  running = true;
  out({ type: "system", subtype: "init", session_id: SID, model: "fake", tools: [], mcp_servers: [], slash_commands: [], permissionMode: "default", cwd: process.cwd() });
  for (const p of prompts) out({ type: "user", uuid: p.echo ?? write(p.message), session_id: SID, parent_tool_use_id: null, isReplay: true, message: p.message });
  setTimeout(() => {
    const uuid = randomUUID();
    const message = { id: "m-" + uuid, model: "fake", role: "assistant", content: [{ type: "text", text: "ok" }] };
    log({ type: "assistant", uuid, parentUuid: last, message });
    last = uuid;
    out({ type: "assistant", uuid, session_id: SID, parent_tool_use_id: null, message });
    out({ type: "result", subtype: "success", is_error: false, session_id: SID, duration_ms: TURN_MS, num_turns: 1, result: "ok" });
    running = false;
    if (!queue.length) return;
    const next = queue.splice(0);
    for (const p of next) {
      log({ type: "queue-operation", operation: "dequeue" });
      write(p.message);
      p.echo = randomUUID();
    }
    setTimeout(() => turn(next), GAP_MS);
  }, TURN_MS);
}
readline.createInterface({ input: process.stdin }).on("line", (line) => {
  const m = JSON.parse(line);
  if (m.type === "control_request") return out({ type: "control_response", response: { subtype: "success", request_id: m.request_id, response: {} } });
  if (m.type !== "user") return;
  if (running) {
    log({ type: "queue-operation", operation: "enqueue" });
    // Con FAKE_DROP_QUEUE, lo de la cola se pierde sin eco ni turno.
    if (!process.env.FAKE_DROP_QUEUE) queue.push({ message: m.message });
  } else turn([{ message: m.message }]);
}).on("close", () => process.exit(0));
`;

// Un daemon que usa el Claude Code de mentira, con su carpeta de proyectos aparte.
async function startWithFakeClaude(env = {}) {
  const dir = mkdtempSync(join(tmpdir(), "atic-claude-"));
  const fake = join(dir, "fake-claude.mjs");
  writeFileSync(fake, FAKE_CLAUDE);
  const config = join(dir, "config");
  const cwd = join(dir, "proyecto");
  mkdirSync(cwd);
  const projectDir = join(config, "projects", cwd.replace(/[^a-zA-Z0-9]/g, "-"));
  const daemon = await startDaemon({ ATIC_CLAUDE_PATH: fake, CLAUDE_CONFIG_DIR: config, FAKE_PROJECT_DIR: projectDir, ...env });
  return { ...daemon, cwd, transcript: join(projectDir, "0f0f0f0f-1111-4222-8333-444444444444.jsonl") };
}

// Los eventos de la sesión `key` que llegan en `ms` (o hasta el primero que cumple `pred`).
async function collect(conn, key, ms, pred = () => false) {
  const seen = [];
  const end = Date.now() + ms;
  for (;;) {
    const msg = await conn.poll(end - Date.now());
    if (!msg) return seen;
    if (msg.key !== key) continue;
    seen.push(msg);
    if (pred(msg)) return seen;
  }
}

const isResult = (m) => m.event === "result";
const external = (events) => events.filter((m) => m.event === "external");

test("un mensaje dirigido que queda para el turno siguiente no deja la sesión libre ni parece un cambio de fuera", async () => {
  const { child, found, cwd } = await startWithFakeClaude();
  try {
    const pill = await connect(found, { client: "code", since: 0 });
    await pill.next();
    await call(pill, "start", { key: "k1", cwd });
    // Un primer turno deja el archivo vigilado, como en una conversación que ya venía.
    await call(pill, "send", { key: "k1", text: "cero" });
    await collect(pill, "k1", 5000, isResult);
    // Sin nada en la cola, el resultado deja la sesión libre.
    assert.deepEqual((await collect(pill, "k1", 1200)).filter((m) => m.event === "turn" || m.event === "external"), []);

    await call(pill, "send", { key: "k1", text: "uno" });
    await collect(pill, "k1", 5000, (m) => m.event === "user" && m.data.isReplay);
    // Dirigido: entra con el turno en curso y Claude Code lo deja en su cola.
    await call(pill, "send", { key: "k1", text: "dos" });
    let events = await collect(pill, "k1", 5000, isResult);
    assert.ok(events.some(isResult), "terminó el primer turno");
    // Entre ese resultado y el turno siguiente, el archivo ya tiene el mensaje (y su `isMeta`).
    events = await collect(pill, "k1", 1000);
    assert.deepEqual(external(events), [], "lo que escribe la propia sesión no es un cambio de fuera");
    assert.ok(events.some((m) => m.event === "turn" && m.data.queued === true), "avisa que sigue trabajando");
    // Con el mensaje en la cola, el proceso no se deja cerrar ni retirar.
    const refused = await call(pill, "shutdown", {});
    assert.equal(refused.busy, true);
    events = [...events, ...(await collect(pill, "k1", 6000, isResult))];
    assert.ok(events.some((m) => m.event === "user" && m.data.isReplay), "el mensaje de la cola abrió su turno");
    assert.ok(events.some(isResult), "y terminó");
    events = await collect(pill, "k1", 1500);
    assert.deepEqual(external(events), []);
    await call(pill, "shutdown", { force: true });
  } finally {
    child.kill();
  }
});

test("si el mensaje en cola no abre un turno, la sesión queda libre pasado el plazo", async () => {
  const { child, found, cwd } = await startWithFakeClaude({ FAKE_DROP_QUEUE: "1", ATIC_SIDECAR_QUEUED_TURN_MS: "800" });
  try {
    const pill = await connect(found, { client: "code", since: 0 });
    await pill.next();
    await call(pill, "start", { key: "k3", cwd });
    await call(pill, "send", { key: "k3", text: "uno" });
    await collect(pill, "k3", 5000, (m) => m.event === "user" && m.data.isReplay);
    await call(pill, "send", { key: "k3", text: "dos" });
    await collect(pill, "k3", 5000, isResult);
    const events = await collect(pill, "k3", 3000, (m) => m.event === "turn" && m.data.queued === false);
    assert.deepEqual(
      events.filter((m) => m.event === "turn").map((m) => m.data.queued),
      [true, false],
    );
    assert.equal((await call(pill, "shutdown", {})).ok, true, "ya no trabaja: se deja cerrar");
    await new Promise((r) => child.on("exit", r));
  } finally {
    child.kill();
  }
});

test("un cambio de fuera se avisa con la sesión libre; los mensajes isMeta y sintéticos no cuentan", async () => {
  const { child, found, cwd, transcript } = await startWithFakeClaude();
  try {
    const pill = await connect(found, { client: "code", since: 0 });
    await pill.next();
    await call(pill, "start", { key: "k2", cwd });
    await call(pill, "send", { key: "k2", text: "hola" });
    await collect(pill, "k2", 5000, isResult);
    await collect(pill, "k2", 1200);
    const line = (entry) => appendFileSync(transcript, JSON.stringify({ sessionId: "s", timestamp: new Date().toISOString(), parentUuid: null, ...entry }) + "\n");
    // Lo que agrega Claude Code solo (el relleno al retomar, un `isMeta`) no es otra conversación.
    line({ type: "user", uuid: "meta-1", isMeta: true, message: { role: "user", content: "[meta]" } });
    line({ type: "assistant", uuid: "synthetic-1", message: { model: "<synthetic>", role: "assistant", content: [{ type: "text", text: "No response requested." }] } });
    assert.deepEqual(external(await collect(pill, "k2", 1500)), []);
    // Un mensaje escrito desde otro cliente sí.
    line({ type: "user", uuid: "foreign-1", message: { role: "user", content: "desde la terminal" } });
    const events = await collect(pill, "k2", 3000, (m) => m.event === "external");
    assert.equal(external(events).length, 1);
    await call(pill, "shutdown", { force: true });
  } finally {
    child.kill();
  }
});
