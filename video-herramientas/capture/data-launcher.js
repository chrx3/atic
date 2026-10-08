(() => {
const IC = __ICONS__;
const APPS = [
  { id: "app:C:\Program Files\Google\Chrome\Application\chrome.exe", title: "Google Chrome", icon: IC.chrome },
  { id: "app:Visual Studio Code.lnk", title: "Visual Studio Code", icon: IC.vscode },
  { id: "app:Cursor.lnk", title: "Cursor", icon: IC.cursor },
  { id: "app:Zed.lnk", title: "Zed", icon: IC.zed },
  { id: "app:Docker Desktop.lnk", title: "Docker Desktop", icon: IC.docker },
  { id: "app:Microsoft Teams.lnk", title: "Microsoft Teams", icon: IC.teams },
  { id: "app:Microsoft Edge.lnk", title: "Microsoft Edge", icon: IC.edge },
  { id: "app:Explorador de archivos.lnk", title: "Explorador de archivos", icon: IC.explorer },
  { id: "app:Bloc de notas.lnk", title: "Bloc de notas", icon: IC.notepad },
  { id: "app:Outlook.lnk", title: "Outlook", icon: IC.outlook },
  { id: "app:Zen Browser.lnk", title: "Zen Browser", icon: IC.zen },
  { id: "app:Configuración.lnk", title: "Configuración", icon: IC.settings },
  { id: "app:Administrador de tareas.lnk", title: "Administrador de tareas", icon: IC.taskmgr },
  { id: "app:Calculadora.lnk", title: "Calculadora", icon: IC.calc },
  { id: "app:Símbolo del sistema.lnk", title: "Símbolo del sistema", icon: IC.cmd },
];
const ACTIONS = [
  { id: "action:capture", title: "Capturar pantalla", subtitle: "Seleccionar ventana, región o monitor" },
  { id: "action:board", title: "Pizarra", subtitle: "Congelar la pantalla y marcarla" },
  { id: "action:color", title: "Color", subtitle: "Cuentagotas: un píxel de la pantalla al portapapeles" },
  { id: "action:clipboard", title: "Clipboard", subtitle: "Abrir el historial junto a la pill" },
  { id: "action:system", title: "Sistema", subtitle: "Volumen, recursos y pantallas junto a la pill" },
  { id: "action:sys-mute", title: "Silenciar", subtitle: "Alternar el silencio de la salida de audio" },
  { id: "action:agent-console:claude", title: "Claude Code", subtitle: "Nueva consola de Claude Code (claude) en la pizarra" },
];
const icons = Object.fromEntries(APPS.map((a) => [a.id, a.icon]));
const hit = (e, score) => e.id.startsWith("app:") ? { id: e.id, kind: "app", title: e.title, subtitle: "Aplicación", score } : { id: e.id, kind: "action", title: e.title, subtitle: e.subtitle, score };
const norm = (s) => s.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
function search(q) {
  q = q.trim(); if (!q) return [];
  const out = [];
  const m = q.match(/^(\d[\d.]*)\s*usd\s+a\s+clp$/i);
  if (m) { const n = +m[1].replace(/\./g, ""); out.push({ id: "calc:" + q, kind: "action", title: Math.round(n * 965.71).toLocaleString("es-CL") + " CLP", subtitle: "valor oficial del 29 sep · Enter para copiar", score: 4294967295 }); return out; }
  if (/^[\d\s+\-*/().,^%]+$/.test(q) && /[+\-*/^%]/.test(q)) { try { const v = Function("return (" + q.replace(/,/g, ".").replace(/\^/g, "**") + ")")(); if (Number.isFinite(v)) out.push({ id: "calc:" + q, kind: "action", title: String(+v.toFixed(6)).replace(".", ","), subtitle: "Enter para copiar", score: 4294967295 }); } catch {} }
  const nq = norm(q);
  const all = [...APPS, ...ACTIONS];
  const scored = [];
  for (const e of all) { const t = norm(e.title); const words = t.split(/\s+/); const initials = words.map((w) => w[0]).join("");
    let s = 0; if (t.startsWith(nq)) s = 1000; else if (words.some((w) => w.startsWith(nq))) s = 800; else if (initials.startsWith(nq)) s = 700; else if (t.includes(nq)) s = 400;
    if (s) scored.push([s - t.length, e]); }
  scored.sort((a, b) => b[0] - a[0]);
  return out.concat(scored.slice(0, 24).map(([s, e]) => hit(e, s)));
}
Object.assign(window.__mock, {
  overlay_active_anchor: { x: 540, y: 460 },
  launcher_search: (a) => search(a.query),
  launcher_list_favorites: () => [APPS[0], APPS[1], APPS[5], APPS[7]].map((e) => hit(e)),
  launcher_list_recents: () => [APPS[1], APPS[0], APPS[5]].map((e, i) => ({ ...hit(e), running: i < 2, lastUsedAt: Date.now() - i * 600000 })),
  launcher_icon: (a) => icons[a.id] ?? null,
  launcher_toggle_favorite: () => [],
  show_launcher: () => { setTimeout(() => window.__emit("launcher-bubble-anchor", { side: "top", offset: 162, x: 378, y: 56, w: 324, h: 40 }), 0); return null; },
});
window.__openLauncher = () => window.__emit("launcher-bubble-anchor", { side: "top", offset: 162, x: 378, y: 56, w: 324, h: 40 });
})();
