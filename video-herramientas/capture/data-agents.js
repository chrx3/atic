(() => {
const BS = String.fromCharCode(92);
const P = (...parts) => parts.join(BS);
const ROOT = P("C:", "dev", "tienda-web");
for (const k of Object.keys(localStorage)) if (k.startsWith("atic.agents")) localStorage.removeItem(k);
localStorage.setItem("atic.agents.startFolder", ROOT);
window.__consoles = {}; let n = 0;
Object.assign(window.__mock, {
  cli_on_path: (a) => ["claude", "codex", "opencode", "cursor-agent"].includes(a.name),
  console_open: (a) => { const id = "con-" + (++n); window.__consoles[id] = { ...a.options, id }; return id; },
  console_tail: "",
  console_foreground_cli: (a) => (window.__consoles[a.session]?.command || null),
  console_changed_files: [],
  agents_take_new_console: null,
  hub_status: null,
  console_resize: (a) => { const c = window.__consoles[a.session]; if (c) { c.cols = a.cols; c.rows = a.rows; } return null; },
  console_write: (a) => { (window.__writes ||= []).push(a); return null; },
  list_directories: (a) => ({ path: a.path && a.path !== "~" ? a.path : ROOT, parent: P("C:", "dev"), entries: ["node_modules", "public", "src", "tests"].map((x) => ({ name: x, path: P(ROOT, x) })), roots: [{ name: "Inicio", path: P("C:", "Users", "dev") }, { name: "Escritorio", path: P("C:", "Users", "dev", "Desktop") }, { name: "Documentos", path: P("C:", "Users", "dev", "Documents") }] }),
});
window.__out = (session, data) => window.__emit("console-output", { session, data });
window.__rect = (text) => { const els = [...document.querySelectorAll("button, [role=button], a, input, textarea")]; const el = els.find((e) => (e.innerText || e.placeholder || e.getAttribute("aria-label") || "").trim().startsWith(text)); if (!el) return null; const r = el.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; };
})();
