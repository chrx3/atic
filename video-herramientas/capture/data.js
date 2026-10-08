const __now = Date.now(); const __ago = (m) => __now - m * 60000;
const ASSET = "http://127.0.0.1:1431/";
window.__clip = [
  { id: "c1", kind: "text", preview: "https://github.com/chrx3/atic", text: "https://github.com/chrx3/atic", createdAtMs: __ago(0.2), pinned: false, fingerprint: "f1", source: "Chrome" },
  { id: "c2", kind: "image", preview: "Imagen 1166×336", imagePath: ASSET + "clip-chart.png", createdAtMs: __ago(3), pinned: false, fingerprint: "f2", source: "Recortes" },
  { id: "c3", kind: "text", preview: "Reunión movida al jueves 10:30 — sala Andes", text: "Reunión movida al jueves 10:30 — sala Andes", createdAtMs: __ago(9), pinned: true, fingerprint: "f3", source: "Teams" },
  { id: "c4", kind: "text", preview: "pnpm --dir apps/desktop exec vitest run", text: "pnpm --dir apps/desktop exec vitest run", createdAtMs: __ago(16), pinned: false, fingerprint: "f4", source: "Terminal" },
  { id: "c5", kind: "text", preview: "#7C5CFF", text: "#7C5CFF", createdAtMs: __ago(24), pinned: false, fingerprint: "f5", source: "Atic" },
  { id: "c6", kind: "text", preview: "Hola Camila, te adjunto el informe Q3 con los cambios que conversamos.", text: "Hola Camila, te adjunto el informe Q3 con los cambios que conversamos.", createdAtMs: __ago(41), pinned: false, fingerprint: "f6", source: "Outlook" },
  { id: "c7", kind: "text", preview: "SELECT planta, AVG(disponibilidad) FROM turnos GROUP BY planta;", text: "SELECT planta, AVG(disponibilidad) FROM turnos GROUP BY planta;", createdAtMs: __ago(75), pinned: false, fingerprint: "f7", source: "VS Code" },
  { id: "c8", kind: "text", preview: "Av. Apoquindo 4501, Las Condes", text: "Av. Apoquindo 4501, Las Condes", createdAtMs: __ago(130), pinned: false, fingerprint: "f8", source: "Chrome" },
];
window.__mock = Object.assign(window.__mock || {}, {
  get_config: __CONFIG__,
  overlay_work_areas: [{ x: 0, y: 0, w: innerWidth, h: innerHeight, primary: true }],
  overlay_cursor: () => window.__cursor || null,
  overlay_primary_down: () => !!window.__down,
  dictation_phase: "idle",
  is_recording: false,
  media_now: null,
  agents_window_visible: false,
  set_overlay_hit_rects: (a) => { window.__hits = a.rects; return null; },
  overlay_cursor_over_hit: (a) => { const c = window.__cursor; if (!c) return false; return (window.__hits || []).some((r) => r.id === a.id && c.x >= r.x && c.x <= r.x + r.w && c.y >= r.y && c.y <= r.y + r.h); },
  list_clipboard_history: () => structuredClone(window.__clip),
  get_scratchpad: { body: "", updated_at: 0 },
});
