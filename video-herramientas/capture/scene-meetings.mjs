import { open } from "./session.mjs";
import fs from "node:fs";
const extra = fs.readFileSync("assets-d/meetings-data.js", "utf8");
const p = await open({ route: "/", label: "main", extra, w: 760, h: 760, dpr: 2160 / 760 });
await p.wait(600);
const at = (txt) => p.eval(`(()=>{const b=[...document.querySelectorAll('button')].find(e=>e.textContent.trim().startsWith(${JSON.stringify(txt)}));const r=b.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
let r = await at("Reunión semanal");
await p.move(r.x, r.y); await p.down(); await p.wait(50); await p.up(); await p.wait(500);
await p.move(600, 660);
await p.eval(`__emit("recording-status",{active:true,recording:null})`);
await p.wait(50);
await p.eval(`__advance(872000)`);
const emit = (ev, pl) => p.eval(`__emit(${JSON.stringify(ev)},${JSON.stringify(pl)})`);
await emit("live-transcript-final", { start_ms: 0, end_ms: 5000, speaker: "others", speaker_name: null, text: "Partamos por la planta norte: cerramos septiembre con 99,2 % de disponibilidad." });
await emit("live-transcript-final", { start_ms: 6000, end_ms: 9000, speaker: "me", speaker_name: null, text: "Buenísimo. ¿Y las alarmas de temperatura de la línea 3?" });
let i = 0;
const lv = () => { i++; return p.eval(`__emit("audio-levels",{mic:${(0.25 + 0.2 * Math.abs(Math.sin(i / 5))).toFixed(3)},system:${(0.45 + 0.35 * Math.abs(Math.sin(i / 3.3 + 1))).toFixed(3)}})`); };
for (let k = 0; k < 30; k++) { await lv(); await p.step(); }
const dir = "frames/meetings";
p.startRec(dir);
const partial1 = "Bajaron a la mitad desde que cambiamos los umbrales. Quedan dos sensores por calibrar.".split(" ");
const partial2 = "Ok, dejemos eso para esta semana. ¿Quién lo toma?".split(" ");
// 0 – 1.2 s: partial grows word by word
for (let k = 0; k < partial1.length; k++) {
  await emit("live-transcript-partial", { start_ms: 10000, end_ms: 12000, speaker: "others", speaker_name: null, text: partial1.slice(0, k + 1).join(" ") });
  for (let j = 0; j < 5; j++) { await lv(); await p.frame(); }
}
await emit("live-transcript-final", { start_ms: 10000, end_ms: 16000, speaker: "others", speaker_name: null, text: partial1.join(" ") });
for (let k = 0; k < partial2.length; k++) {
  await emit("live-transcript-partial", { start_ms: 17000, end_ms: 19000, speaker: "me", speaker_name: null, text: partial2.slice(0, k + 1).join(" ") });
  for (let j = 0; j < 5; j++) { await lv(); await p.frame(); }
}
await emit("live-transcript-final", { start_ms: 17000, end_ms: 21000, speaker: "me", speaker_name: null, text: partial2.join(" ") });
for (let j = 0; j < 8; j++) { await lv(); await p.frame(); }
// glide to "Resumir" and click
r = await at("Resumir");
const a = { ...p.pos }; const n = 30;
for (let k = 1; k <= n; k++) { const t = k / n; const e = t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2; await p.move(a.x + (r.x - a.x) * e, a.y + (r.y - a.y) * e); await lv(); await p.frame(); }
for (let j = 0; j < 6; j++) { await lv(); await p.frame(); }
await p.down(); for (let j = 0; j < 4; j++) { await lv(); await p.frame(); } await p.up();
for (let j = 0; j < 110; j++) { await lv(); await p.frame(); }
console.log("frames", p.stopRec());
await p.encode(dir, "clips/meetings.mp4");
await p.close();
