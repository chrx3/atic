// Virtual clock: time only advances when the driver calls __advance(ms).
(() => {
  const realNow = performance.now.bind(performance);
  const realDateNow = Date.now;
  const t0 = realNow(), d0 = realDateNow();
  let vt = t0;
  let seq = 1;
  const timers = new Map();
  let rafs = new Map();
  performance.now = () => vt;
  Date.now = () => d0 + (vt - t0);
  const RealDate = Date;
  window.Date = class extends RealDate { constructor(...a) { if (a.length) super(...a); else super(d0 + (vt - t0)); } static now() { return d0 + (vt - t0); } };
  window.setTimeout = (fn, ms = 0, ...args) => { const id = seq++; timers.set(id, { at: vt + Math.max(0, +ms || 0), fn, args }); return id; };
  window.clearTimeout = (id) => timers.delete(id);
  window.setInterval = (fn, ms = 0, ...args) => { const id = seq++; const iv = Math.max(1, +ms || 1); timers.set(id, { at: vt + iv, fn, args, iv }); return id; };
  window.clearInterval = (id) => timers.delete(id);
  window.requestAnimationFrame = (fn) => { const id = seq++; rafs.set(id, fn); return id; };
  window.cancelAnimationFrame = (id) => rafs.delete(id);
  const tracked = new WeakMap();
  function syncAnimations() {
    for (const a of document.getAnimations()) {
      let s = tracked.get(a);
      if (!s) { s = { start: vt - (a.currentTime || 0) }; tracked.set(a, s); try { a.pause(); } catch {} }
      const timing = a.effect?.getComputedTiming?.();
      const end = timing ? timing.endTime : Infinity;
      const ct = vt - s.start;
      if (Number.isFinite(end) && ct >= end) { try { a.finish(); } catch { a.currentTime = end; } }
      else { try { a.currentTime = ct; } catch {} }
    }
  }
  window.__advance = (ms) => {
    const target = vt + ms;
    for (;;) {
      let next = null;
      for (const [id, t] of timers) if (t.at <= target && (!next || t.at < next[1].at || (t.at === next[1].at && id < next[0]))) next = [id, t];
      if (!next) break;
      const [id, t] = next;
      vt = Math.max(vt, t.at);
      if (t.iv) t.at += t.iv; else timers.delete(id);
      try { typeof t.fn === "function" ? t.fn(...t.args) : eval(t.fn); } catch (e) { console.error(e); }
    }
    vt = target;
    const cbs = rafs; rafs = new Map();
    for (const fn of cbs.values()) { try { fn(vt); } catch (e) { console.error(e); } }
    syncAnimations();
    return vt;
  };
  window.__vt = () => vt;
})();
