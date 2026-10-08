import { open } from "./session.mjs";
import fs from "node:fs";
const extra = fs.readFileSync("assets-d/flip-data.js", "utf8");
const css = "html{background:url(http://127.0.0.1:1431/assets-d/flip-desk.png) 0 0/100% 100% !important} body{background:transparent !important}";
const p = await open({ route: "/window-flip", label: "window-flip", extra, css });
await p.move(760, 820);
await p.wait(300);
const dir = "frames/flip";
p.startRec(dir);
await p.hold(300);
await p.eval(`document.documentElement.style.setProperty("background","url(http://127.0.0.1:1431/wall.png) 0 0/100% 100%","important")`);
await p.eval(`__emit("window-flip-open", window.__flipView)`);
await p.hold(700);
// pen tool
const pen = await p.eval(`(()=>{const b=[...document.querySelectorAll('button')].find(e=>e.getAttribute('aria-label')==='Lápiz'||e.title==='Lápiz');const r=b.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
await p.glide(pen.x, pen.y, 380);
await p.hold(60);
await p.down(); await p.hold(60); await p.up();
await p.hold(120);
// ellipse around the tallest bar
const cx = 671, cy = 284, rx = 30, ry = 58;
const start = (a) => ({ x: cx + rx * Math.cos(a), y: cy + ry * Math.sin(a) });
let s = start(-2.2);
await p.glide(s.x, s.y, 300);
await p.down();
const N = 44;
for (let i = 1; i <= N; i++) { const a = -2.2 + (i / N) * (Math.PI * 2 + 0.5); const r = 1 + 0.06 * Math.sin(i / 5); const q = { x: cx + rx * r * Math.cos(a), y: cy + ry * r * Math.sin(a) }; await p.move(q.x, q.y, true); await p.frame(); }
await p.up();
await p.hold(120);
// underline under the idea note
await p.glide(532, 414, 280);
await p.down();
for (let i = 1; i <= 26; i++) { const t = i / 26; await p.move(532 + 268 * t, 414 + 2 * Math.sin(t * 6), true); await p.frame(); }
await p.up();
await p.glide(860, 560, 400);
await p.hold(750);
console.log("frames", p.stopRec());
await p.encode(dir, "clips/flip.mp4");
await p.close();
