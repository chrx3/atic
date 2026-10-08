import { open } from "./session.mjs";
import fs from "node:fs";
import { claude, codex, title, GREEN, RESET } from "./tui-b.mjs";

const REC = process.argv[2] === "rec";
const extra = fs.readFileSync("data-agents.js", "utf8");
const TB = `(() => { document.addEventListener("DOMContentLoaded", () => { const t = document.createElement("div"); t.id = "__tb"; t.innerHTML = '<span class="i"></span><span>Consolas de agentes</span><span class="c">&#x2500;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&#x2610;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&#x2715;</span>'; document.body.appendChild(t); }); })();`;
const CSS = `html{background:url(http://127.0.0.1:1431/wall.png) 0 0/100% 100% !important} body{background:transparent !important}
.agents-window{position:fixed !important; left:40px; top:92px; width:1000px; height:948px !important; border-radius:0 0 10px 10px; overflow:hidden; transform:translateZ(0); box-shadow:0 30px 80px rgba(0,0,0,.55); outline:1px solid rgba(255,255,255,.07)}
#__tb{position:fixed; left:40px; top:60px; width:1000px; height:32px; background:#1c1c1c; color:#e6e6e6; border-radius:10px 10px 0 0; font:12px "Segoe UI Variable Text","Segoe UI",sans-serif; display:flex; align-items:center; gap:10px; padding:0 0 0 14px; box-sizing:border-box; z-index:5; outline:1px solid rgba(255,255,255,.07)}
#__tb .i{width:14px;height:14px;border-radius:4px;background:linear-gradient(135deg,#d6d6cf,#8a8a84)}
#__tb .c{margin-left:auto; padding-right:18px; color:#cfcfcf; font-size:13px}`;

const p = await open({ route: "/agents", label: "agents", w: 1080, h: 1080, dpr: 2, extra: extra + TB, css: CSS });
p.on((d) => {
  if (d.method === "Runtime.exceptionThrown") console.log("STACK", d.params.exceptionDetails.exception?.description?.slice(0, 600));
});
await p.wait(1000);
const center = (js) =>
  p.eval(`(()=>{const el=${js}; if(!el) return null; const r=el.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
const clickAt = async (pt) => {
  if (!pt) throw new Error("no target");
  await p.move(pt.x, pt.y);
  await p.wait(40);
  await p.down();
  await p.wait(50);
  await p.up();
};

// Pre-roll: Claude Code from the empty board, then Codex from "Nueva consola".
await clickAt(await center(`[...document.querySelectorAll('button.empty-agent')].find(b=>b.innerText.includes('Claude Code'))`));
await p.wait(900);
await clickAt(await center(`[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='Nueva consola')`));
await p.wait(400);
await clickAt(await center(`[...document.querySelectorAll('button')].filter(b=>b.innerText.trim()==='Codex').pop()`));
await p.wait(900);
await p.key("Enter", "Enter", 13, "\r");
await p.wait(1200);
await clickAt(await center(`[...document.querySelectorAll('button')].find(b=>b.getAttribute('aria-label')==='Acomodar una junto a otra')`));
await p.wait(1500);

const cons = await p.eval("window.__consoles");
const ids = Object.keys(cons);
const cc = ids.find((i) => cons[i].command === "claude");
const cx = ids.find((i) => cons[i].command === "codex");
if (!cc || !cx) throw new Error("consoles: " + JSON.stringify(cons));
const out = (id, s) => p.eval(`window.__out(${JSON.stringify(id)}, ${JSON.stringify(s)})`);
const dims = (id) => p.eval(`window.__consoles[${JSON.stringify(id)}]`);
let C = await dims(cc);
let X = await dims(cx);
const cs = { prompt: null, items: [], spin: null, draft: "" };
const xs = {
  prompt: "escribe tests para el carrito de compras",
  items: [{ t: "head", text: "Explored", sub: ["Read cart.ts, cart.test.ts, money.ts"] }],
  spin: 0,
  secs: 12,
};
const drawC = () => out(cc, claude(cs, C.cols, C.rows));
const drawX = () => out(cx, codex(xs, X.cols, X.rows));
await out(cc, title("Claude Code"));
await drawC();
await drawX();
await p.wait(400);

// Focus the Claude Code console so the composer targets it.
await clickAt(
  await center(`(()=>{const t=[...document.querySelectorAll('.xterm')]; return t.find(x=>{let n=x; for(let i=0;i<8&&n;i++){ if((n.innerText||'').includes('Claude Code') && n.querySelector('button')) return true; n=n.parentElement;} return false;}) || t[0];})()`),
);
await p.wait(300);
C = await dims(cc);
X = await dims(cx);
await drawC();
await drawX();
const composer = await center(`document.querySelector('.is-composer textarea, .is-composer input, .is-composer [contenteditable]')`);
console.log("claude", C.cols, C.rows, "codex", X.cols, X.rows, "composer", JSON.stringify(composer));
await p.move(760, 620);
await p.wait(200);

// ---------- Recording ----------
const dir = REC ? "frames/agents" : "frames/agents-probe";
p.startRec(dir);
let f = 0;
const tick = async () => {
  f++;
  if (f % 6 === 0) {
    if (cs.spin != null) {
      cs.spin++;
      await drawC();
    }
    if (xs.spin != null) {
      xs.spin++;
      await drawX();
    }
  }
  if (f % 60 === 0) {
    if (cs.spin != null) cs.secs++;
    if (xs.spin != null) xs.secs++;
  }
};
const frames = async (n) => {
  for (let i = 0; i < n; i++) {
    await tick();
    await p.frame();
  }
};
await frames(6);
await p.glide(composer.x - 40, composer.y, 280);
f += 17;
await p.down();
await frames(3);
await p.up();
const PROMPT = "agrega validación al formulario de registro";
for (let i = 0; i < PROMPT.length; i += 2) {
  await p.type(PROMPT.slice(i, i + 2));
  await frames(1);
}
await frames(8);
await p.key("Enter", "Enter", 13, "\r");
await frames(2);
console.log("writes", JSON.stringify(await p.eval("window.__writes||[]")).slice(0, 300));
cs.prompt = PROMPT;
cs.spin = 0;
cs.secs = 0;
cs.spinText = "Pondering";
await out(cc, title("Validación del formulario de registro"));
await drawC();
await frames(24);
cs.items.push({ t: "say", text: "Voy a revisar el formulario de registro y sus tests." });
await drawC();
await frames(14);
xs.items.push({ t: "head", text: "Edited", extra: "src/cart.test.ts (+46 -0)" });
await drawX();
await frames(8);
cs.items.push({ t: "tool", name: "Read", arg: "src/components/RegisterForm.tsx", res: ["Read 84 lines"], done: true });
await drawC();
await frames(20);
cs.spinText = "Editing";
cs.items.push({ t: "tool", name: "Update", arg: "src/components/RegisterForm.tsx", res: ["Updated with 12 additions and 2 removals"], done: true });
const diff = [
  "+  const emailOk = EMAIL_RE.test(email);",
  "+  const passOk = password.length >= 8;",
  "-  const canSubmit = true;",
  "+  const canSubmit = emailOk && passOk && terms;",
];
const dItem = { t: "diff", lines: [] };
cs.items.push(dItem);
for (const l of diff) {
  dItem.lines.push(l);
  await drawC();
  await frames(6);
}
await frames(12);
xs.items.push({ t: "head", text: "Ran", extra: "pnpm vitest run cart", sub: [] });
await drawX();
cs.spinText = "Testing";
cs.items.push({ t: "tool", name: "Bash", arg: "pnpm test RegisterForm", res: [] });
await drawC();
await frames(30);
cs.items[cs.items.length - 1].res = [`${GREEN}✓ 12 tests passed${RESET} (1.4s)`];
cs.items[cs.items.length - 1].done = true;
await drawC();
xs.items[xs.items.length - 1].sub = [`${GREEN}✓ 9 passed${RESET} (812ms)`];
await drawX();
await frames(16);
cs.spin = null;
cs.items.push({
  t: "say",
  text: "Listo. El formulario valida el correo, exige 8+ caracteres en la contraseña y no deja enviar sin aceptar los términos.",
});
await drawC();
await out(cc, title("Claude Code"));
await frames(12);
xs.spin = null;
xs.items.push({ t: "rule", text: "Worked for 41s" }, { t: "say", text: "Agregué 9 tests al carrito: totales, descuentos y stock agotado." });
await drawX();
await frames(14);
await p.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key: "0", code: "Digit0", windowsVirtualKeyCode: 48, modifiers: 2 });
await p.send("Input.dispatchKeyEvent", { type: "keyUp", key: "0", code: "Digit0", windowsVirtualKeyCode: 48, modifiers: 2 });
await frames(3);
const zoomed = await p.eval("document.body.innerText.match(/\d+%/)?.[0]");
console.log("zoom after ctrl0", zoomed);
await frames(75);
const n = p.stopRec();
console.log("frames", n);
if (REC) await p.encode(dir, "clips/agents.mp4");
else for (const k of [5, 40, 60, 120, 200, n - 1]) fs.copyFileSync(`${dir}/${String(Math.min(k, n - 1)).padStart(5, "0")}.jpg`, `ag-p${k}.jpg`);
await p.close();
