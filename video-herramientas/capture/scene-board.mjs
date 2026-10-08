import { open } from "./session.mjs";
const extra = `Object.assign(window.__mock, {
  pending_annotation: { path: "http://127.0.0.1:1431/wall-work.png", width: 2160, height: 2160, mode: "board", focus: { x: 0, y: 0, width: 2160, height: 2160 } },
});`;
const p = await open({ route: "/capture-annotate", label: "annotate", extra });
const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
async function drag(x, y, ms) {
  const a = { ...p.pos }; const n = Math.max(1, Math.round(ms / (1000 / 60)));
  for (let i = 1; i <= n; i++) { const e = ease(i / n); await p.move(a.x + (x - a.x) * e, a.y + (y - a.y) * e, true); await p.frame(); }
}
async function click() { await p.down(); await p.hold(50); await p.up(); await p.hold(30); }
async function stroke(from, to, ms) { await p.glide(from[0], from[1], 280); await p.down(); await p.hold(30); await drag(to[0], to[1], ms); await p.hold(30); await p.up(); }
await p.move(700, 300);
await p.wait(800);
const dir = "frames/board";
p.startRec(dir);
await p.hold(100);
await stroke([790, 585], [522, 650], 450);
await p.hold(120);
await p.glide(375, 37, 380); await click();
await stroke([351, 465], [411, 511], 380);
await p.hold(120);
await p.glide(435, 37, 380); await click();
await stroke([256, 511], [596, 511], 450);
await p.hold(150);
await p.glide(640, 260, 450);
await p.hold(800);
console.log("frames", p.stopRec());
await p.encode(dir, "clips/board.mp4");
await p.close();
