// El modo `--daemon` del agente: archivo de descubrimiento, saludo con token, peticiones,
// números de secuencia, reemplazo de la conexión de un cliente y cierre ordenado.
// Correr con `node --test` dentro de sidecar/ (no abre conversaciones: no gasta nada).
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, statSync } from "node:fs";
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
    const send = (msg) => socket.write(JSON.stringify(msg) + "\n");
    socket.on("error", reject);
    socket.on("connect", () => {
      send({ hello: { token: found.token, ...hello } });
      resolve({ socket, next, send });
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
