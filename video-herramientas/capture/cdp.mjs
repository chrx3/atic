// Minimal CDP client over Node's global WebSocket.
export async function connect(port = 9333) {
  const res = await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" });
  const t = await res.json();
  const ws = new WebSocket(t.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0; const pend = new Map(); const listeners = [];
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pend.has(d.id)) { const p = pend.get(d.id); pend.delete(d.id); d.error ? p.j(new Error(JSON.stringify(d.error))) : p.r(d.result); }
    else listeners.forEach((l) => l(d));
  };
  const send = (method, params = {}) => new Promise((r, j) => { const i = ++id; pend.set(i, { r, j }); ws.send(JSON.stringify({ id: i, method, params })); });
  const page = {
    send, on: (f) => listeners.push(f), targetId: t.id,
    async eval(expr) { const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text); return r.result.value; },
    async shot(path, clip) { const fs = await import("node:fs"); const r = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false, ...(clip ? { clip: { ...clip, scale: 1 } } : {}) }); fs.writeFileSync(path, Buffer.from(r.data, "base64")); },
    async close() { await fetch(`http://127.0.0.1:${port}/json/close/${t.id}`); ws.close(); },
  };
  await send("Page.enable"); await send("Runtime.enable");
  return page;
}
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
