import fs from "node:fs";
const NL = String.fromCharCode(10);
const snips = [
  ["s1", "Firma de correo", ["Saludos,", "Camila Rojas", "Jefa de Operaciones · Planta Norte", "+56 9 8123 4567"].join(NL), ["firma"], 30],
  ["s2", "Dirección oficina", "Av. Apoquindo 4501, piso 12, Las Condes, Santiago", ["dir"], 120],
  ["s3", "Respuesta: reunión", "¡Hola! Gracias por escribir. ¿Te acomoda el jueves a las 10:30? Te envío la invitación.", ["reu"], 300],
  ["s4", "Deploy a staging", "pnpm build && pnpm deploy --env staging", ["deploy"], 600],
  ["s5", "Datos de transferencia", "Banco Estado · Cuenta Vista 12345678 · RUT 76.123.456-7", ["banco"], 1440],
  ["s6", "Link agenda", "https://cal.com/camila-rojas/30min", ["agenda"], 2880],
].map(([id, name, body, aliases, m]) => ({ id, name, body, aliases, m }));
const scratch = ["Pendientes de hoy", "- Revisar informe Q3 con Pablo", "- Llamar a proveedor de repuestos", "- Subir acta de la reunión de las 10:30", "", "Idea: alertas en un solo panel"].join(NL);
const board = { assetsDir: "http://127.0.0.1:1431", blocks: [
  { kind: "text", id: "b1", body: ["Informe Q3 — revisar con Pablo", "Cifras de disponibilidad planta norte"].join(NL), x: 60, y: 60, w: 620, h: 180 },
  { kind: "check", id: "b2", items: [{ id: "i1", text: "Validar gráfico de solicitudes", done: true }, { id: "i2", text: "Agregar resumen ejecutivo", done: false }, { id: "i3", text: "Enviar a gerencia el viernes", done: false }], x: 60, y: 300, w: 560, h: 220 },
  { kind: "image", id: "b3", asset: "clip-chart.png", width: 1166, height: 336, x: 60, y: 580, w: 1000, h: 288 },
  { kind: "text", id: "b4", body: ["Proveedor repuestos: llamar antes de las 12", "Cotización #4471"].join(NL), x: 1260, y: 60, w: 620, h: 180 },
  { kind: "check", id: "b5", items: [{ id: "j1", text: "Confirmar despacho", done: false }, { id: "j2", text: "Actualizar planilla", done: true }], x: 1260, y: 300, w: 560, h: 160 },
] };
const js = `(() => {
  const now = Date.now();
  window.__snips = ${JSON.stringify(snips)}.map((s) => ({ id: s.id, name: s.name, body: s.body, aliases: s.aliases, updatedAtMs: now - s.m * 60000 }));
  window.__scratch = { body: ${JSON.stringify(scratch)}, updatedAtMs: now - 5 * 60000 };
  Object.assign(window.__mock, {
    list_snippets: () => structuredClone(window.__snips),
    get_scratchpad: () => structuredClone(window.__scratch),
    set_scratchpad: (a) => { window.__scratch = { body: a.body, updatedAtMs: Date.now() }; return structuredClone(window.__scratch); },
    list_notes: [],
    window_flip_board: ${JSON.stringify(board)},
  });
})();
`;
fs.writeFileSync(new URL("./snippets.js", import.meta.url), js);
