import { open } from "./session.mjs";
const WALL = "html{background:url(http://127.0.0.1:1431/assets-d/wall-dict.png) 0 0/100% 100% !important} body{background:transparent !important}";
const TEXT = "Además, el equipo de soporte redujo a la mitad los tickets pendientes antes del cierre del trimestre.";
const extra = `
document.addEventListener("DOMContentLoaded", () => {
  const d = document.createElement("div");
  d.id = "__doc";
  d.style.cssText = "position:fixed;left:214px;top:538px;width:652px;font:15.5px/24.8px 'Segoe UI Variable Text','Segoe UI',sans-serif;color:rgb(58,58,66);pointer-events:none;z-index:0";
  d.innerHTML = '<span id="__dt"></span><span id="__dc" style="display:inline-block;width:1.5px;height:19px;background:#1b1b1f;vertical-align:-4px;margin-left:1px"></span>';
  document.documentElement.appendChild(d);
});
window.__docText = (t, hl) => { const e = document.getElementById("__dt"); e.textContent = t; e.style.background = hl > 0 ? "rgba(0,120,215," + (0.28 * hl) + ")" : "transparent"; e.style.borderRadius = "3px"; };
window.__caret = (on) => { document.getElementById("__dc").style.opacity = on ? 1 : 0; };
`;
const p = await open({ route: "/overlay", label: "overlay", css: WALL, extra });
await p.hideCursor(true);
await p.wait(400);
const dir = "frames/dictation";
p.startRec(dir);
let f = 0;
// deterministic speech-ish levels
const rnd = (i) => { const x = Math.sin(i * 12.9898) * 43758.5453; return x - Math.floor(x); };
const level = (i) => {
  const syll = 0.55 + 0.45 * Math.sin(i / 2.3) * Math.sin(i / 5.1 + 1);
  const pause = (i % 70) > 58 ? 0.15 : 1;
  return Math.max(0.05, Math.min(1, (0.35 + 0.65 * Math.abs(syll) * (0.6 + 0.4 * rnd(i))) * pause));
};
const frame = async (fn) => { f++; await p.eval(`__caret(${Math.floor(f / 32) % 2 === 0})`); if (fn) await fn(f); await p.frame(); };
for (let i = 0; i < 8; i++) await frame();                         // 0.13 s idle
await p.eval(`__emit("dictation-status",{phase:"listening",message:null,text:null})`);
for (let i = 0; i < 132; i++) await frame(async () => p.eval(`__emit("audio-levels",{mic:${Math.pow(10, (-46 + 30 * level(i)) / 20).toFixed(5)},system:0})`)); // 2.2 s listening
await p.eval(`__emit("audio-levels",{mic:0,system:0})`);
await p.eval(`__emit("dictation-status",{phase:"transcribing",message:null,text:null})`);
for (let i = 0; i < 34; i++) await frame();                        // 0.57 s transcribing
await p.eval(`__emit("dictation-status",{phase:"pasted",message:null,text:${JSON.stringify(TEXT)}})`);
for (let i = 0; i < 80; i++) await frame(async () => p.eval(`__docText(${JSON.stringify(TEXT)}, ${Math.max(0, 1 - i / 45).toFixed(3)})`)); // 1.33 s pasted
console.log("frames", p.stopRec());
await p.encode(dir, "clips/dictation.mp4");
await p.close();
