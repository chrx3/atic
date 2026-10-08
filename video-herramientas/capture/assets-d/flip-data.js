(() => {
  const A = "http://127.0.0.1:1431/assets-d";
  window.__flipView = {
    key: "chrome-informe", title: "Informe Q3 — Operaciones", exe: "chrome.exe", icon: "",
    previewPath: A + "/flip-preview.png",
    blocks: [
      { kind: 'text', id: 't1', body: 'Pendientes antes de enviar\nConfirmar cifras de la planta sur con Camila.', x: 40, y: 40, w: 480, h: 130 },
      { kind: 'check', id: 'k1', items: [
        { id: 'i1', text: 'Revisar gráfico de disponibilidad', done: true },
        { id: 'i2', text: 'Agregar comparación con Q2', done: false },
        { id: 'i3', text: 'Enviar a gerencia el lunes', done: false } ], x: 40, y: 200, w: 480, h: 190 },
      { kind: 'image', id: 'im1', asset: 'chart.png', width: 1624, height: 376, x: 580, y: 40, w: 580, h: 134 },
      { kind: 'text', id: 't2', body: 'Idea: mostrar la tendencia de 12 meses en vez del trimestre.', x: 580, y: 240, w: 580, h: 100 },
    ],
    assetsDir: A, cardLeft: 70 / 1080, cardTop: 150 / 1080, cardWidth: 940 / 1080, cardHeight: 800 / 1080, page: null,
  };
  Object.assign(window.__mock, {
    window_flip_state: null,
    window_flip_board: () => ({ blocks: window.__flipView.blocks, assetsDir: A }),
    window_flip_focus_is_foreign: false,
  });
})();
