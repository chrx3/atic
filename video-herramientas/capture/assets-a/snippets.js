(() => {
  const now = Date.now();
  window.__snips = [{"id":"s1","name":"Firma de correo","body":"Saludos,\nCamila Rojas\nJefa de Operaciones · Planta Norte\n+56 9 8123 4567","aliases":["firma"],"m":30},{"id":"s2","name":"Dirección oficina","body":"Av. Apoquindo 4501, piso 12, Las Condes, Santiago","aliases":["dir"],"m":120},{"id":"s3","name":"Respuesta: reunión","body":"¡Hola! Gracias por escribir. ¿Te acomoda el jueves a las 10:30? Te envío la invitación.","aliases":["reu"],"m":300},{"id":"s4","name":"Deploy a staging","body":"pnpm build && pnpm deploy --env staging","aliases":["deploy"],"m":600},{"id":"s5","name":"Datos de transferencia","body":"Banco Estado · Cuenta Vista 12345678 · RUT 76.123.456-7","aliases":["banco"],"m":1440},{"id":"s6","name":"Link agenda","body":"https://cal.com/camila-rojas/30min","aliases":["agenda"],"m":2880}].map((s) => ({ id: s.id, name: s.name, body: s.body, aliases: s.aliases, updatedAtMs: now - s.m * 60000 }));
  window.__scratch = { body: "Pendientes de hoy\n- Revisar informe Q3 con Pablo\n- Llamar a proveedor de repuestos\n- Subir acta de la reunión de las 10:30\n\nIdea: alertas en un solo panel", updatedAtMs: now - 5 * 60000 };
  Object.assign(window.__mock, {
    list_snippets: () => structuredClone(window.__snips),
    get_scratchpad: () => structuredClone(window.__scratch),
    set_scratchpad: (a) => { window.__scratch = { body: a.body, updatedAtMs: Date.now() }; return structuredClone(window.__scratch); },
    list_notes: [],
    window_flip_board: {"assetsDir":"http://127.0.0.1:1431","blocks":[{"kind":"text","id":"b1","body":"Informe Q3 — revisar con Pablo\nCifras de disponibilidad planta norte","x":60,"y":60,"w":620,"h":180},{"kind":"check","id":"b2","items":[{"id":"i1","text":"Validar gráfico de solicitudes","done":true},{"id":"i2","text":"Agregar resumen ejecutivo","done":false},{"id":"i3","text":"Enviar a gerencia el viernes","done":false}],"x":60,"y":300,"w":560,"h":220},{"kind":"image","id":"b3","asset":"clip-chart.png","width":1166,"height":336,"x":60,"y":580,"w":1000,"h":288},{"kind":"text","id":"b4","body":"Proveedor repuestos: llamar antes de las 12\nCotización #4471","x":1260,"y":60,"w":620,"h":180},{"kind":"check","id":"b5","items":[{"id":"j1","text":"Confirmar despacho","done":false},{"id":"j2","text":"Actualizar planilla","done":true}],"x":1260,"y":300,"w":560,"h":160}]},
  });
})();
