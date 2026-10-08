import { connect, sleep } from "./cdp.mjs";
import fs from "node:fs";
import path from "node:path";
const here = (f) => new URL(f, import.meta.url);
const read = (f) => fs.readFileSync(here(f), "utf8");

export async function open({ route, label, w = 1080, h = 1080, dpr = 2, extra = "", css = "", boot = 4000 }) {
  const p = await connect();
  p.on((d) => {
    if (d.method === "Runtime.exceptionThrown") console.log("EXC", d.params.exceptionDetails.exception?.description?.split("\n")[0]);
    if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") console.log("ERR", JSON.stringify(d.params.args.map((a) => a.value ?? a.description)).slice(0, 200));
  });
  await p.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: dpr, mobile: false });
  await p.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
  const cssInject = css ? `document.addEventListener("DOMContentLoaded",()=>{const s=document.createElement("style");s.textContent=${JSON.stringify(css)};document.head.appendChild(s);});` : "";
  const src = `window.__LABEL=${JSON.stringify(label)};` + read("clock.js") + read("mock.js") +
    read("data.js").replace("__CONFIG__", read("config.json")) + extra + cssInject;
  await p.send("Page.addScriptToEvaluateOnNewDocument", { source: src });
  await p.send("Page.navigate", { url: `http://localhost:1430${route}` });
  p.w = w; p.h = h; p.dpr = dpr;
  // Boot: advance virtual time alongside real time.
  const t = Date.now();
  while (Date.now() - t < boot) { await sleep(30); try { await p.eval("window.__advance && __advance(30)"); } catch {} }
  p.step = async (ms = 1000 / 60) => p.eval(`__advance(${ms})`);
  p.wait = async (ms) => { const n = Math.ceil(ms / (1000 / 60)); for (let i = 0; i < n; i++) { await p.step(); await sleep(2); } };
  p.pos = { x: -100, y: -100 };
  await p.eval(`(() => { const c = document.createElement("div"); c.id = "__cur"; c.style.cssText = "position:fixed;left:0;top:0;width:24px;height:24px;z-index:2147483647;pointer-events:none;transform:translate(-100px,-100px);filter:drop-shadow(0 1px 2px rgba(0,0,0,.45))"; c.innerHTML = '<svg width="24" height="24" viewBox="0 0 24 24"><path d="M3 2 L3 19.5 L7.6 15.3 L10.6 22 L13.6 20.7 L10.7 14.2 L17 14.2 Z" fill="#fff" stroke="#000" stroke-width="1.3" stroke-linejoin="round"/></svg>'; document.documentElement.appendChild(c); })()`);
  p.cursorVisible = true;
  p.hideCursor = async (hide = true) => { p.cursorVisible = !hide; await p.eval(`document.getElementById("__cur").style.opacity=${hide ? 0 : 1}`); };
  p.move = async (x, y, down = false) => {
    p.pos = { x, y };
    await p.eval(`window.__cursor={x:${x},y:${y}};document.getElementById("__cur").style.transform="translate(${x}px,${y}px)"`);
    await p.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: down ? "left" : "none", buttons: down ? 1 : 0 });
  };
  p.down = async () => { await p.eval("window.__down=true"); await p.send("Input.dispatchMouseEvent", { type: "mousePressed", x: p.pos.x, y: p.pos.y, button: "left", buttons: 1, clickCount: 1 }); };
  p.up = async () => { await p.eval("window.__down=false"); await p.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: p.pos.x, y: p.pos.y, button: "left", buttons: 0, clickCount: 1 }); };
  p.key = async (key, code, vk, text) => {
    await p.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", key, code, windowsVirtualKeyCode: vk, text });
    await p.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: vk });
  };
  p.type = async (ch) => { await p.send("Input.insertText", { text: ch }); };
  p.snap = async (file, clip) => {
    const jpg = file.endsWith(".jpg");
    const r = await p.send("Page.captureScreenshot", { format: jpg ? "jpeg" : "png", ...(jpg ? { quality: 93 } : {}), ...(clip ? { clip: { ...clip, scale: 1 } } : {}) });
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, Buffer.from(r.data, "base64"));
  };
  // Recorder: every frame = advance 1/60 s + screenshot.
  p.rec = null;
  p.startRec = (dir) => { fs.rmSync(dir, { recursive: true, force: true }); fs.mkdirSync(dir, { recursive: true }); p.rec = { dir, n: 0 }; };
  p.frame = async () => {
    await p.step();
    await sleep(4);
    if (p.rec) { const f = path.join(p.rec.dir, String(p.rec.n++).padStart(5, "0") + ".jpg"); await p.snap(f); }
  };
  p.hold = async (ms) => { const n = Math.round(ms / (1000 / 60)); for (let i = 0; i < n; i++) await p.frame(); };
  // Smooth cursor glide over `ms` (ease in-out), one frame per step.
  p.glide = async (x, y, ms = 500) => {
    const a = { ...p.pos }; const n = Math.max(1, Math.round(ms / (1000 / 60)));
    for (let i = 1; i <= n; i++) { const t = i / n; const e = t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2; await p.move(a.x + (x - a.x) * e, a.y + (y - a.y) * e); await p.frame(); }
  };
  p.stopRec = () => { const n = p.rec?.n; p.rec = null; return n; };
  // Encode the recorded frames to an mp4 clip (60 fps) and drop the frames.
  p.encode = async (dir, out) => {
    const { execFileSync } = await import("node:child_process");
    fs.mkdirSync(path.dirname(out), { recursive: true });
    execFileSync("ffmpeg", ["-v", "error", "-y", "-framerate", "60", "-i", path.join(dir, "%05d.jpg"), "-c:v", "libx264", "-preset", "slow", "-crf", "14", "-pix_fmt", "yuv420p", "-movflags", "+faststart", out]);
    fs.rmSync(dir, { recursive: true, force: true });
  };
  return p;
}
export { sleep };
