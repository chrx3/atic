(() => {
  const cbs = new Map(); let nid = 1;
  window.__calls = []; window.__cbs = cbs; window.__listeners = {};
  window.__mock = window.__mock || {};
  const internals = {
    metadata: { currentWindow: { label: window.__LABEL }, currentWebview: { windowLabel: window.__LABEL, label: window.__LABEL } },
    transformCallback(cb, once) { const id = nid++; cbs.set(id, cb); return id; },
    unregisterCallback(id) { cbs.delete(id); },
    convertFileSrc(p) { return p; },
    async invoke(cmd, args) {
      window.__calls.push(cmd);
      if (cmd === "plugin:event|listen") { (window.__listeners[args.event] ||= []).push(args.handler); return args.handler; }
      if (cmd === "plugin:event|unlisten") return null;
      const m = window.__mock[cmd];
      if (typeof m === "function") return m(args);
      if (m !== undefined) return structuredClone(m);
      if (/^(list_|agent_sessions|agent_presences|system_alerts|agent_threads|failed_shortcuts|shared_shortcuts|agent_backends)/.test(cmd)) return [];
      return null;
    },
  };
  window.__TAURI_INTERNALS__ = internals;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__emit = (event, payload) => (window.__listeners[event] || []).forEach((h) => { const cb = cbs.get(h); cb && cb({ event, id: 0, payload }); });
})();
