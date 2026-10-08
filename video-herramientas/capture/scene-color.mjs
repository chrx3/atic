import { open } from "./session.mjs";
const PROBE = process.argv[2] === "probe";
const WALL = `html{background:url(http://127.0.0.1:1431/wall-work.png) 0 0/100% 100% !important} body{background:transparent !important}
.stage{position:fixed !important;left:var(--lx,0px);top:var(--ly,0px);width:312px;min-height:0 !important;height:160px;box-sizing:border-box}
html.rose .stage{width:344px;height:588px}`;
const extra = `Object.assign(window.__mock, {
  color_picker_state: { session: 1, active: true, open: false, patch: null },
  color_picker_set_rose: () => null,
  complete_color_pick: (a) => a.hex,
});
(() => {
  const img = new Image(); img.crossOrigin = "anonymous"; img.src = "http://127.0.0.1:1431/wall-work.png";
  img.onload = () => { const c = document.createElement("canvas"); c.width = img.width; c.height = img.height; const x = c.getContext("2d"); x.drawImage(img, 0, 0); window.__px = x.getImageData(0, 0, c.width, c.height); };
  window.__patchAt = (cx, cy) => { const d = window.__px; const S = 13, h = 6; const px = Math.round(cx * 2), py = Math.round(cy * 2); const rgba = [];
    for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) { const i = ((py - h + y) * d.width + (px - h + x)) * 4; rgba.push(d.data[i], d.data[i + 1], d.data[i + 2], 255); }
    const i = (py * d.width + px) * 4; const r = d.data[i], g = d.data[i + 1], b = d.data[i + 2];
    return { session: 1, hex: "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("").toUpperCase(), r, g, b, size: S, rgba }; };
  window.__place = (cx, cy) => { document.documentElement.style.setProperty("--lx", (cx + 4) + "px"); document.documentElement.style.setProperty("--ly", (cy + 4) + "px"); };
})();`;
const p = await open({ route: "/color-loupe", label: "color-loupe", css: WALL, extra });
await p.move(250, 740);
await p.wait(300);
const sample = async () => { await p.eval(`__place(${p.pos.x},${p.pos.y}); __emit("color-patch", __patchAt(${p.pos.x},${p.pos.y}))`); };
// Glide that re-samples every frame, like Rust's 33 ms loop.
const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
async function track(x, y, ms) {
  const a = { ...p.pos }; const n = Math.max(1, Math.round(ms / (1000 / 60)));
  for (let i = 1; i <= n; i++) { const e = ease(i / n); await p.move(a.x + (x - a.x) * e, a.y + (y - a.y) * e); await sample(); await p.frame(); }
}
async function hold(ms) { const n = Math.round(ms / (1000 / 60)); for (let i = 0; i < n; i++) { await sample(); await p.frame(); } }
const dir = "frames/color";
if (!PROBE) p.startRec(dir);
await sample();
await hold(150);
await track(342, 735, 450);
await hold(150);
await track(474.5, 700, 550);
await hold(300);
// R: the rose opens. Rust resizes the window to ROSE and flips it above the cursor.
await p.eval(`document.documentElement.classList.add("rose"); document.documentElement.style.setProperty("--ly", (${p.pos.y} - 4 - 588) + "px"); __emit("color-toggle-rose", 1)`);
await p.hold(600);
if (PROBE) await p.snap("col-a.png");
const sw = await p.eval(`(() => { const r = document.querySelector(".rose-swatch").getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
const rgbBtn = await p.eval(`(() => { const r = [...document.querySelectorAll(".code")][1].getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; })()`);
await p.glide(rgbBtn[0], rgbBtn[1], 450);
await p.down(); await p.hold(50); await p.up();
await p.hold(350);
await p.glide(sw[0], sw[1], 350);
await p.down(); await p.hold(50); await p.up();
await p.hold(120);
await p.eval(`__emit("color-picker-ended", 1)`);
await p.hold(900);
if (PROBE) { await p.snap("col-b.png"); console.log(JSON.stringify(await p.eval("[...new Set(window.__calls)]"))); }
else { console.log("frames", p.stopRec()); await p.encode(dir, "clips/color.mp4"); }
await p.close();
