// El modo `--daemon` del agente: archivo de descubrimiento, saludo con token, peticiones,
// números de secuencia, reemplazo de la conexión de un cliente y cierre ordenado.
// Correr con `node --test` dentro de sidecar/ (no abre conversaciones: no gasta nada).
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import net from "node:net";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));

async function startDaemon(env = {}) {
  const dir = mkdtempSync(join(tmpdir(), "atic-daemon-"));
  const discovery = join(dir, "code-agent.json");
  const child = spawn(process.execPath, [join(here, "agent.mjs"), "--daemon"], {
    env: { ...process.env, ATIC_SIDECAR_DISCOVERY: discovery, ATIC_SIDECAR_VERSION: "v-test", ...env },
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
