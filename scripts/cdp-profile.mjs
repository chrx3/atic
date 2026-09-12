// Perfila por CDP una ventana de Atic y dice en qué se va el CPU del frontend.
//
// Por qué existe: medir por proceso dice CUÁNTO cuesta el overlay, pero no en
// qué se va. La diferencia importa porque decide el arreglo: si el tiempo está
// en `ScriptDuration` es código Svelte, y si está en `LayoutDuration` o en el
// compositor el arreglo es de otra naturaleza. Y aun siendo script, hay que
// saber QUÉ función.
//
// El puerto CDP (9223) sólo existe en el build de debug: lo pone `overlay.rs`
// detrás de `#[cfg(debug_assertions)]`, y es un puerto propio de esa ventana
// porque tiene su propio `user-data-dir`. O sea que para correr esto hay que
// tener `tauri dev` levantado, no el build de release.
//
//   node scripts/cdp-profile.mjs                     # 15 s, top 25
//   node scripts/cdp-profile.mjs --seconds 30
//   node scripts/cdp-profile.mjs --eval "document.querySelectorAll('*').length"
//
// No modifica la app: sólo la observa.

const PORT = Number(arg("--port") ?? 9223);
const SECONDS = Number(arg("--seconds") ?? 15);
const TOP = Number(arg("--top") ?? 25);
const MATCH = arg("--match") ?? "overlay";
const EVAL = arg("--eval");

function arg(name) {
  const i = process.argv.indexOf(name);
  return i === -1 ? undefined : process.argv[i + 1];
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function pickTarget() {
  const res = await fetch(`http://127.0.0.1:${PORT}/json/list`);
  const list = await res.json();
  const pages = list.filter((t) => t.type === "page");
  if (pages.length === 0) throw new Error("no hay targets de tipo page en el puerto " + PORT);
  const hit = pages.find((t) => (t.url + " " + t.title).includes(MATCH)) ?? pages[0];
  return { hit, pages };
}

function connect(url) {
  const ws = new WebSocket(url);
  let next = 0;
  const pending = new Map();

  ws.addEventListener("message", (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(msg.error.message));
      else resolve(msg.result);
    }
  });

  const ready = new Promise((resolve, reject) => {
    ws.addEventListener("open", () => resolve());
    ws.addEventListener("error", (e) => reject(new Error("websocket: " + (e.message ?? "error"))));
  });

  const send = (method, params) =>
    new Promise((resolve, reject) => {
      const id = ++next;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });

  return { ws, ready, send };
}

// `Performance.getMetrics` devuelve un array de {name, value}; el delta entre
// dos lecturas es lo que dice el tipo de trabajo. Los tiempos vienen en
// segundos y las cantidades en unidades.
const METRICAS = [
  "TaskDuration",
  "ScriptDuration",
  "LayoutDuration",
  "RecalcStyleDuration",
  "LayoutCount",
  "RecalcStyleCount",
  "JSHeapUsedSize",
  "JSHeapTotalSize",
  "Nodes",
  "Documents",
  "Frames",
];

function aMapa(metrics) {
  const out = {};
  for (const m of metrics) out[m.name] = m.value;
  return out;
}

// El perfil trae `samples` (índice de muestra -> id de nodo) y `timeDeltas`
// (µs entre muestras). El tiempo propio de un nodo es la suma de los deltas de
// las muestras que le tocaron. Es tiempo propio, no acumulado: no hay que
// sumar los hijos o se cuenta dos veces.
function porFuncion(profile) {
  const nodos = new Map();
  for (const n of profile.nodes) nodos.set(n.id, n);

  const propio = new Map();
  for (let i = 0; i < profile.samples.length; i++) {
    const id = profile.samples[i];
    const dt = profile.timeDeltas[i] ?? 0;
    propio.set(id, (propio.get(id) ?? 0) + dt);
  }

  const total = [...propio.values()].reduce((a, b) => a + b, 0);

  // Se agrupa por función + ubicación, no por id de nodo: la misma función
  // aparece en varios nodos y si no se juntan queda el top lleno de repetidos.
  const agrupado = new Map();
  for (const [id, us] of propio) {
    const n = nodos.get(id);
    if (!n) continue;
    const cf = n.callFrame;
    const nombre = cf.functionName || "(anónima)";
    const url = cf.url ? cf.url.replace(/^https?:\/\/localhost:1420/, "") : "(nativo)";
    const clave = `${nombre} @ ${url}:${cf.lineNumber + 1}`;
    const prev = agrupado.get(clave) ?? { nombre, url, linea: cf.lineNumber + 1, us: 0 };
    prev.us += us;
    agrupado.set(clave, prev);
  }

  const filas = [...agrupado.values()].sort((a, b) => b.us - a.us);
  const porArchivo = new Map();
  for (const f of filas) {
    const prev = porArchivo.get(f.url) ?? { url: f.url, us: 0 };
    prev.us += f.us;
    porArchivo.set(f.url, prev);
  }

  return {
    total,
    filas: filas.slice(0, TOP),
    archivos: [...porArchivo.values()].sort((a, b) => b.us - a.us).slice(0, 15),
  };
}

const fmtMs = (us) => (us / 1000).toFixed(1).padStart(8) + " ms";
const fmtPct = (us, total) => ((us / total) * 100).toFixed(1).padStart(5) + " %";

const { hit, pages } = await pickTarget();
console.log("");
console.log(`Puerto ${PORT} — ${pages.length} target(s) de tipo page`);
console.log(`Perfilando: ${hit.title}  ${hit.url}`);

const { ws, ready, send } = connect(hit.webSocketDebuggerUrl);
await ready;

await send("Performance.enable");
await send("Profiler.enable");
// 200 µs de muestreo: con el default de 1 ms un frame de 16 ms se muestrea 16
// veces y las funciones cortas desaparecen.
await send("Profiler.setSamplingInterval", { interval: 200 });

if (EVAL) {
  const r = await send("Runtime.evaluate", { expression: EVAL, returnByValue: true, awaitPromise: true });
  console.log("");
  console.log("eval:", JSON.stringify(r.result?.value ?? r.result));
  ws.close();
  process.exit(0);
}

const m0 = aMapa((await send("Performance.getMetrics")).metrics);
await send("Profiler.start");
console.log(`Muestreando ${SECONDS} s...`);
await sleep(SECONDS * 1000);
const { profile } = await send("Profiler.stop");
const m1 = aMapa((await send("Performance.getMetrics")).metrics);
ws.close();

console.log("");
console.log(`=== Trabajo del renderer en ${SECONDS} s ===`);
for (const k of METRICAS) {
  if (m0[k] === undefined || m1[k] === undefined) continue;
  const delta = m1[k] - m0[k];
  const esTiempo = k.endsWith("Duration");
  const valor = esTiempo ? delta.toFixed(2).padStart(8) + " s" : Math.round(delta).toString().padStart(8);
  const porSeg = esTiempo ? ((delta / SECONDS) * 100).toFixed(1).padStart(7) + " % de un núcleo" : "";
  console.log(`  ${k.padEnd(20)} ${valor}   ${porSeg}`);
}

const agg = porFuncion(profile);
console.log("");
console.log(`=== Tiempo propio por función (total muestreado ${(agg.total / 1000).toFixed(0)} ms) ===`);
for (const f of agg.filas) {
  console.log(`  ${fmtMs(f.us)}  ${fmtPct(f.us, agg.total)}  ${f.nombre}  (${f.url}:${f.linea})`);
}

console.log("");
console.log("=== Tiempo propio por archivo ===");
for (const a of agg.archivos) {
  console.log(`  ${fmtMs(a.us)}  ${fmtPct(a.us, agg.total)}  ${a.url}`);
}
console.log("");
