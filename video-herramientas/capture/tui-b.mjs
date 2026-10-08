// ANSI screens that mimic the Claude Code and Codex TUIs, redrawn whole.
const E = "\x1b[";
const rgb = (r, g, b) => `${E}38;2;${r};${g};${b}m`;
const bg = (r, g, b) => `${E}48;2;${r};${g};${b}m`;
const R = `${E}0m`, B = `${E}1m`, D = `${E}2m`;
export const ORANGE = rgb(215, 119, 87), GRAY = rgb(140, 140, 140), GREEN = rgb(78, 186, 101), RED = rgb(255, 107, 128), WHITE = rgb(235, 235, 235), CYAN = rgb(100, 200, 210);
export const RESET = R;
const vis = (s) => s.replace(/\x1b\[[0-9;]*m/g, "");
const pad = (s, w) => s + " ".repeat(Math.max(0, w - [...vis(s)].length));
const box = (lines, w, color) => {
  const inner = w - 2;
  return [
    color + "╭" + "─".repeat(inner) + "╮" + R,
    ...lines.map((l) => color + "│" + R + pad(" " + l, inner) + color + "│" + R),
    color + "╰" + "─".repeat(inner) + "╯" + R,
  ];
};
function screen(body, footer, rows) {
  const room = rows - footer.length;
  const shown = body.slice(Math.max(0, body.length - room));
  const all = [...shown, ...Array(Math.max(0, room - shown.length)).fill(""), ...footer];
  return `${E}?25l${E}H${E}2J` + all.map((l, i) => `${E}${i + 1};1H` + l).join("");
}
const wrap = (text, w) => {
  const out = [];
  let line = "";
  for (const word of text.split(" ")) {
    if ((line + " " + word).trim().length > w) {
      out.push(line);
      line = word;
    } else line = (line ? line + " " : "") + word;
  }
  if (line) out.push(line);
  return out;
};
const SPIN = ["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];

export function claude(st, cols, rows) {
  const w = Math.min(cols - 1, 56);
  const body = [
    ...box([`${ORANGE}✻${R} ${B}Welcome to Claude Code!${R}`, "", `${D}  /help for help, /status for your current setup${R}`, "", `${D}  cwd: C:\\dev\\tienda-web${R}`], w, ORANGE),
    "",
  ];
  if (st.prompt) body.push(`${GRAY}> ${st.prompt}${R}`, "");
  for (const it of st.items) {
    if (it.t === "say") {
      const ls = wrap(it.text, cols - 4);
      body.push(`${WHITE}●${R} ${ls[0]}`, ...ls.slice(1).map((l) => "  " + l), "");
    } else if (it.t === "tool") {
      body.push(`${it.done ? GREEN : GRAY}●${R} ${B}${it.name}${R}(${it.arg})`);
      for (const r of it.res || []) body.push(`  ${GRAY}⎿${R}  ${r}`);
      body.push("");
    } else if (it.t === "diff") {
      for (const d of it.lines)
        body.push(d[0] === "+" ? `     ${bg(28, 64, 38)}${GREEN}${pad(" " + d, cols - 8)}${R}` : `     ${bg(74, 34, 40)}${RED}${pad(" " + d, cols - 8)}${R}`);
      body.push("");
    }
  }
  if (st.spin != null) body.push(`${ORANGE}${SPIN[st.spin % SPIN.length]} ${st.spinText}…${R} ${GRAY}(${st.secs}s · esc to interrupt)${R}`, "");
  const footer = [...box([`${B}>${R} ${st.draft || ""}`], cols - 1, GRAY), `  ${GRAY}? for shortcuts${R}`];
  return screen(body, footer, rows);
}

export function codex(st, cols, rows) {
  const w = Math.min(cols - 1, 50);
  const body = [
    ...box([`${B}>_ OpenAI Codex${R} ${GRAY}(v0.46.0)${R}`, "", `${GRAY}model:${R}     gpt-5-codex  ${CYAN}/model${R}`, `${GRAY}directory:${R} C:\\dev\\tienda-web`], w, GRAY),
    "",
  ];
  if (st.prompt) body.push(`${GRAY}›${R} ${st.prompt}`, "");
  for (const it of st.items) {
    if (it.t === "head") {
      body.push(`${GREEN}•${R} ${B}${it.text}${R}${it.extra ? " " + it.extra : ""}`);
      for (const r of it.sub || []) body.push(`  ${GRAY}└${R} ${r}`);
    } else if (it.t === "say") {
      body.push("");
      for (const l of wrap(it.text, cols - 3)) body.push(" " + l);
    } else if (it.t === "rule") body.push("", `${GRAY}─ ${it.text} ${"─".repeat(Math.max(0, cols - it.text.length - 5))}${R}`);
  }
  if (st.spin != null) body.push("", `${B}•${R} ${CYAN}Working${R} ${GRAY}(${st.secs}s • esc to interrupt)${R}`);
  const footer = ["", `${CYAN}›${R} ${GRAY}Ask Codex to do anything${R}`, "", `  ${GRAY}⏎ send   ⌃J newline   ⌃T transcript${R}`];
  return screen(body, footer, rows);
}
export const title = (t) => `\x1b]0;${t}\x07`;
