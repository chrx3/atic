(() => {
  const iso = (d, h, m) => new Date(2026, 8, d, h, m).toISOString();
  const recs = [
    { id: "r1", title: "Reunión semanal de operaciones", started_at: iso(29, 9, 30), duration_secs: 1712, mic_path: null, system_path: null, status: "summarized" },
    { id: "r2", title: "Llamada con proveedor de sensores", started_at: iso(26, 16, 0), duration_secs: 1045, mic_path: null, system_path: null, status: "summarized" },
    { id: "r3", title: "Planificación Q4", started_at: iso(24, 11, 15), duration_secs: 2890, mic_path: null, system_path: null, status: "transcribed" },
    { id: "r4", title: "Retro del sprint 38", started_at: iso(22, 17, 30), duration_secs: 1930, mic_path: null, system_path: null, status: "transcribed" },
    { id: "r5", title: "Entrevista — analista de datos", started_at: iso(19, 10, 0), duration_secs: 2410, mic_path: null, system_path: null, status: "summarized" },
  ];
  const seg = (s, sp, name, text) => ({ start_ms: s * 1000, end_ms: s * 1000 + 6000, speaker: sp, speaker_name: name, text });
  const transcript = { language: "es", segments: [
    seg(4, "others", "Camila", "Partamos por la planta norte: cerramos septiembre con 99,2 % de disponibilidad."),
    seg(12, "me", null, "Buenísimo. ¿Y las alarmas de temperatura de la línea 3?"),
    seg(19, "others", "Diego", "Bajaron a la mitad desde que cambiamos los umbrales. Quedan dos sensores por calibrar."),
    seg(28, "me", null, "Ok, dejemos eso para esta semana. ¿Quién lo toma?"),
    seg(33, "others", "Diego", "Yo lo veo con el proveedor el jueves."),
    seg(41, "others", "Camila", "Otra cosa: el reporte semanal todavía lo armamos a mano, son como tres horas."),
    seg(50, "me", null, "Propongo automatizarlo para Q4 y mandarlo los lunes a las nueve."),
  ] };
  const summary = { template: "meeting", title: "Reunión semanal de operaciones", subject: "Resumen — Reunión semanal de operaciones", backend: "local", created_at: iso(29, 10, 2),
    body: "## Resumen\n\nLa planta norte cerró septiembre con **99,2 % de disponibilidad**. Las alarmas de temperatura de la línea 3 bajaron a la mitad tras ajustar los umbrales.\n\n## Decisiones\n\n- Automatizar el reporte semanal en Q4; se envía los lunes a las 9:00.\n- Calibrar los dos sensores pendientes de la línea 3.\n\n## Tareas\n\n- [ ] **Diego** — revisar la calibración con el proveedor (jueves).\n- [ ] **Camila** — definir el formato del reporte automático.\n- [ ] **Tomás** — presupuesto de la automatización para Q4.\n" };
  Object.assign(window.__mock, {
    list_recordings: () => structuredClone(recs),
    get_transcript: (a) => structuredClone(transcript),
    get_summary: (a) => (a.id === "r1" || a.id === "r2" || a.id === "r5") ? structuredClone(summary) : null,
    list_summary_templates: [{ id: "meeting", label: "Reunión" }, { id: "brief", label: "Breve" }],
    list_summary_providers: [],
    current_model_ready: true,
    list_models: [],
    is_recording: false,
    is_focused: true,
    "plugin:window|is_focused": true,
  });
})();
