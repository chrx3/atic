/**
 * Los datos del demo.
 *
 * Todo inventado, todo verosímil: un historial que parece de alguien que
 * trabaja, textos que uno guardaría de verdad, una conversación de agente con
 * herramientas reales. Si el contenido no convence, la herramienta tampoco.
 */
import type { IconNode } from "$lib/atic/icons";
import { TOOLS } from "$atic/lib/core/tools";
import {
  AppWindow,
  Calculator,
  Clipboard,
  Crop,
  FileText,
  Lock,
  Mic,
  Moon,
  Pencil,
  Pipette,
  Search,
  Settings,
  SquareTerminal,
  VolumeX,
} from "$lib/atic/icons";

/** "Ahora" del demo: las fechas relativas se calculan desde acá. */
const NOW = Date.now();
const MIN = 60_000;

/* ─── Launcher ──────────────────────────────────────────────────────────── */

export type LauncherItem = {
  id: string;
  label: string;
  hint: string;
  icon: IconNode;
  /**
   * Como en la app (`kind: app|action` en `types.ts`): las acciones de Atic
   * llevan el icono en verde, las apps del sistema no.
   */
  kind: "app" | "action";
  shortcut?: string;
};

/** Atajos globales reales (los mismos de Ajustes en la app). */
const TOOL_SHORTCUTS: Record<string, string> = {
  meetings: "Ctrl+Shift+R",
  dictation: "Ctrl+Shift+D",
  clipboard: "Ctrl+Shift+V",
  snippets: "Ctrl+Shift+S",
  agents: "Ctrl+Shift+A",
  captures: "Ctrl+Shift+4",
  board: "Ctrl+Shift+X",
  color: "Ctrl+Shift+C",
  launcher: "Ctrl+Space",
};

const TOOL_ICONS_MAP = {
  meetings: Search,
  dictation: Mic,
  clipboard: Clipboard,
  snippets: FileText,
  agents: SquareTerminal,
  captures: Crop,
  board: Pencil,
  color: Pipette,
  launcher: Search,
} as const;

export const LAUNCHER_ITEMS: LauncherItem[] = [
  // Las filas de Atic salen del catálogo real: label, short y blurb son los
  // mismos que muestra la app.
  ...TOOLS.map((tool) => ({
    id: tool.id,
    label: tool.label,
    hint: tool.short,
    icon: TOOL_ICONS_MAP[tool.id as keyof typeof TOOL_ICONS_MAP],
    kind: "action" as const,
    shortcut: TOOL_SHORTCUTS[tool.id],
  })),
  { id: "app-spotify", label: "Spotify", hint: "Aplicación", icon: AppWindow, kind: "app" },
  { id: "app-code", label: "Visual Studio Code", hint: "Aplicación", icon: AppWindow, kind: "app" },
  { id: "app-terminal", label: "Terminal", hint: "Aplicación", icon: SquareTerminal, kind: "app" },
  { id: "app-figma", label: "Figma", hint: "Aplicación", icon: AppWindow, kind: "app" },
  { id: "app-browser", label: "Google Chrome", hint: "Aplicación", icon: Search, kind: "app" },
  { id: "app-calc", label: "Calculadora", hint: "Aplicación", icon: Calculator, kind: "app" },
  { id: "app-settings", label: "Ajustes del sistema", hint: "Aplicación", icon: Settings, kind: "app" },
  { id: "app-mute", label: "Silenciar audio", hint: "Acción del sistema", icon: VolumeX, kind: "app" },
  { id: "app-lock", label: "Bloquear pantalla", hint: "Acción del sistema", icon: Lock, kind: "app" },
  { id: "app-sleep", label: "Suspender", hint: "Acción del sistema", icon: Moon, kind: "app" },
];

/* ─── Clipboard ─────────────────────────────────────────────────────────── */

/** Como en la app (`types.ts`): solo texto e imagen. */
export type ClipKind = "text" | "image";

export type ClipEntry = {
  id: number;
  kind: ClipKind;
  text: string;
  /** Cuándo se copió; la hora la formatea el panel como la app. */
  createdAtMs: number;
  pinned?: boolean;
};

export const CLIPBOARD: ClipEntry[] = [
  { id: 1, kind: "text", text: "SELECT db_name, created_at FROM cliente WHERE activo = 1 ORDER BY created_at DESC;", createdAtMs: NOW - 1 * MIN, pinned: true },
  { id: 2, kind: "text", text: "msimonovic@tsgenviro.com", createdAtMs: NOW - 4 * MIN, pinned: true },
  { id: 3, kind: "text", text: "Reunión del martes · pendientes: firmar contrato, revisar presupuesto Q3, confirmar demo.", createdAtMs: NOW - 12 * MIN },
  { id: 4, kind: "image", text: "Captura — 1440 × 900", createdAtMs: NOW - 18 * MIN },
  { id: 5, kind: "text", text: "pnpm --dir apps/desktop tauri dev", createdAtMs: NOW - 24 * MIN },
  { id: 6, kind: "text", text: "https://github.com/chrx3/atic/releases/latest", createdAtMs: NOW - 31 * MIN },
  { id: 7, kind: "text", text: "Av. Providencia 1208, of. 603 — timbre 3B", createdAtMs: NOW - 47 * MIN },
  { id: 8, kind: "image", text: "gráfico-ventas-q3.png — 812 KB", createdAtMs: NOW - 60 * MIN },
  { id: 9, kind: "text", text: "#standup hoy: migración lista, QA el jueves, release el viernes.", createdAtMs: NOW - 120 * MIN },
  { id: 10, kind: "text", text: "git log --oneline -12 | head -6", createdAtMs: NOW - 180 * MIN },
];

/* ─── Textos ────────────────────────────────────────────────────────────── */

/** El modelo real (`types.ts`): nombre, cuerpo y alias. Sin grupos ni usos. */
export type Snippet = {
  id: string;
  name: string;
  body: string;
  aliases: string[];
  updatedAtMs: number;
};

export const SNIPPETS: Snippet[] = [
  {
    id: "s1",
    name: "Firma de correo",
    body: "Saludos,\nCristián\nAtic · herramientas de escritorio",
    aliases: ["firma", "saludo"],
    updatedAtMs: NOW - 2 * 86_400_000,
  },
  {
    id: "s2",
    name: "Dirección de la oficina",
    body: "Av. Providencia 1208, of. 603\nSantiago, Chile",
    aliases: [],
    updatedAtMs: NOW - 5 * 86_400_000,
  },
  {
    id: "s3",
    name: "Deploy del sitio",
    body: "pnpm --dir apps/web build\nrsync -avz build/ server:/var/www/atic/",
    aliases: ["deploy"],
    updatedAtMs: NOW - 9 * 86_400_000,
  },
  {
    id: "s4",
    name: "Respuesta a consulta de precios",
    body: "Hola, gracias por escribir. El plan anual queda en USD 190 por usuario e incluye soporte prioritario. ¿Te agendo una demo de 20 minutos?",
    aliases: [],
    updatedAtMs: NOW - 12 * 86_400_000,
  },
  {
    id: "s5",
    name: "Datos de facturación",
    body: "Razón social: Ciat SpA\nRUT: 76.543.210-K\nGiro: desarrollo de software",
    aliases: ["factura", "rut"],
    updatedAtMs: NOW - 20 * 86_400_000,
  },
];

/* ─── Agentes ───────────────────────────────────────────────────────────── */

/**
 * Los seis del catálogo real (`agentCatalog.ts`), en su orden: el id es el
 * CLI que Atic lanza. Sin modelos fijos: en la app los informa la sesión.
 */
export type AgentId = "claude" | "opencode" | "codex" | "cursor-agent" | "agy" | "grok";

export type AgentDef = {
  id: AgentId;
  name: string;
  /** Monograma para el chip del tab. */
  mark: string;
  cwd: string;
};

export const AGENTS: AgentDef[] = [
  { id: "claude", name: "Claude Code", mark: "C", cwd: "~/dev/atic" },
  { id: "opencode", name: "OpenCode", mark: "●", cwd: "~/dev/atic" },
  { id: "codex", name: "Codex", mark: "O", cwd: "~/dev/atic" },
  { id: "cursor-agent", name: "Cursor", mark: "▰", cwd: "~/dev/atic" },
  { id: "agy", name: "Antigravity", mark: "A", cwd: "~/dev/atic" },
  { id: "grok", name: "Grok", mark: "G", cwd: "~/dev/atic" },
];

export type AgentTool = {
  name: string;
  /** Lo que se ve como argumento, en una línea. */
  arg: string;
  /** Resultado resumido; `null` = todavía corriendo. */
  out: string | null;
  lines?: string[];
};

export type AgentStep =
  | { role: "user"; text: string }
  | { role: "text"; text: string }
  | { role: "tool"; tool: AgentTool }
  | { role: "done" };

/** Una conversación guionizada por agente: el demo la reproduce al tipear. */
export const AGENT_SCRIPTS: Record<AgentId, { suggestion: string; steps: AgentStep[] }> = {
  claude: {
    suggestion: "Arregla el bug de la rueda que se cierra al elegir herramienta",
    steps: [
      { role: "user", text: "Arregla el bug de la rueda que se cierra al elegir herramienta" },
      { role: "text", text: "Voy a mirar el morph de la rueda y el manejador de selección." },
      { role: "tool", tool: { name: "Grep", arg: "\"onSelect\" apps/desktop/src", out: "3 resultados", lines: ["ParticleWheel.svelte:609: onSelect?.(node.tool.id);", "PillSurface.svelte:214: onSelect={onWheelPick}", "pill.svelte.ts:88: openTool(id)"] } },
      { role: "tool", tool: { name: "Read", arg: "PillSurface.svelte · 200-240", out: "41 líneas" } },
      { role: "text", text: "El problema: `openTool` cierra la rueda y abre el float en el mismo tick, así que la animación de cierre le roba el foco al panel. Lo separo con el `--morph-quick-dur` que ya existe: primero se quita del medio la rueda, después nace el float." },
      { role: "tool", tool: { name: "Edit", arg: "PillSurface.svelte", out: "1 edición", lines: ["- onSelect={openTool}", "+ onSelect={pickWithHandoff}"] } },
      { role: "tool", tool: { name: "Bash", arg: "pnpm test --filter wheel", out: "12 tests · ok" } },
      { role: "text", text: "Listo. La rueda ya no pierde el clic y el float hereda el foco cuando termina el morph. Dejé un test de regresión en `wheelGeometry`." },
      { role: "done" },
    ],
  },
  codex: {
    suggestion: "Agrega un test para el cálculo de alcance del goo",
    steps: [
      { role: "user", text: "Agrega un test para el cálculo de alcance del goo" },
      { role: "text", text: "Reviso `liquid.js`: `reach()` resuelve por bisección. Voy a congelar los valores conocidos." },
      { role: "tool", tool: { name: "Read", arg: "docs/demos/liquid.js · 214-227", out: "14 líneas" } },
      { role: "tool", tool: { name: "Write", arg: "apps/web/src/lib/atic/liquid.test.ts", out: "nuevo archivo", lines: ["expect(reach(6)).toBeCloseTo(10.3, 1);", "expect(reach(0)).toBe(0);", "expect(reach(9)).toBeGreaterThan(14);"] } },
      { role: "tool", tool: { name: "Bash", arg: "pnpm vitest run liquid", out: "3 tests · ok" } },
      { role: "text", text: "Test listo: los tres casos cubren el valor de la app (σ = 6 → 10.3 px), el borde (σ = 0) y la regla de diseño (por encima de σ = 9 las gotas no se sueltan)." },
      { role: "done" },
    ],
  },
  opencode: {
    suggestion: "Muéstrame el estado del repo y qué quedó sin commitear",
    steps: [
      { role: "user", text: "Muéstrame el estado del repo y qué quedó sin commitear" },
      { role: "tool", tool: { name: "Bash", arg: "git status --short", out: "4 archivos", lines: [" M apps/desktop/src/lib/ParticleWheel.svelte", "?? apps/web/", " M docs/demos/rueda.html", " M PLAN.md"] } },
      { role: "text", text: "El árbol está limpio salvo la app de escritorio y el sitio nuevo sin commitear. `apps/web/` es el sitio y el demo — va entero. El cambio de `ParticleWheel` es el handoff de la rueda al float." },
      { role: "tool", tool: { name: "Bash", arg: "git diff --stat", out: "3 archivos · +218 −41" } },
      { role: "text", text: "Sugerencia: separa en dos commits — uno del arreglo de la rueda y otro del sitio, para que el historial se pueda leer." },
      { role: "done" },
    ],
  },
  "cursor-agent": {
    suggestion: "Escribe el README del demo web",
    steps: [
      { role: "user", text: "Escribe el README del demo web" },
      { role: "text", text: "Voy a documentar solo lo que hace falta para correr y desplegar el sitio." },
      { role: "tool", tool: { name: "Read", arg: "apps/desktop/README.md", out: "66 líneas" } },
      { role: "tool", tool: { name: "Write", arg: "apps/web/README.md", out: "nuevo archivo", lines: ["# Atic · sitio y demo", "pnpm dev → localhost:5180", "pnpm build → build/"] } },
      { role: "text", text: "README listo. Incluye cómo correrlo, la nota de que los tokens se importan de la app, y el despliegue estático." },
      { role: "done" },
    ],
  },
  agy: {
    suggestion: "Revisa qué pantallas usan el token --skin",
    steps: [
      { role: "user", text: "Revisa qué pantallas usan el token --skin" },
      { role: "text", text: "Busco el token en los estilos y veo quién lo redefine." },
      { role: "tool", tool: { name: "Grep", arg: "--skin styles/ apps/desktop/src", out: "5 resultados", lines: ["tokens.css:54: --color-skin: var(--skin);", "dark.css:61: --skin: #1a1a18;", "light.css:58: --skin: #f7f7f2;"] } },
      { role: "text", text: "El token lo declaran las paletas (un valor por tema) y lo consumen la pill, la rueda y la consola de agentes. Ninguna pantalla lo redefine salvo la consola, que es el caso previsto." },
      { role: "done" },
    ],
  },
  grok: {
    suggestion: "Resume en una línea qué cambió esta semana",
    steps: [
      { role: "user", text: "Resume en una línea qué cambió esta semana" },
      { role: "tool", tool: { name: "Bash", arg: "git log --oneline --since=7.days", out: "6 commits", lines: ["551868e Arreglar latest.json en macOS", "5e855a3 Empaquetar sidecars universales", "0c454b2 Compilar sidecars universales"] } },
      { role: "text", text: "La semana fue de releases de macOS: sidecars universales y el latest.json del updater. Nada en el frontend." },
      { role: "done" },
    ],
  },
};

/* ─── Color ─────────────────────────────────────────────────────────────── */

export const COLOR_RECENTS = ["#E85A52", "#6FAF88", "#8FA9B8", "#D4A84B", "#1A1A18"];

/* ─── Rueda ─────────────────────────────────────────────────────────────── */

/**
 * Qué gajos lleva la rueda, en orden. Es la configuración de Ajustes → Pill
 * que se ve en la app: cinco herramientas en el primer anillo y el resto
 * detrás del gajo «Más». Se resuelve con `pillLayout` del core, como la app.
 */
export const PILL_RING_IDS = ["clipboard", "agents", "captures", "board", "color"];
export const PILL_MORE_IDS = ["meetings", "snippets"];

/** El gajo que abre el segundo anillo. No es una herramienta. */
export const PILL_MORE_NODE = {
  id: "more",
  label: "Más",
  short: "Las que dejaste en el segundo anillo",
} as const;

/** Abre la ventana principal. Tampoco es una herramienta. */
export const PILL_WINDOW_NODE = {
  id: "window",
  label: "Ventana principal",
  short: "Abrir la app",
} as const;

/* ─── Atajos (para el footer del demo) ──────────────────────────────────── */

export const DEMO_SHORTCUTS = [
  { keys: "Ctrl+Shift+Space", label: "Abrir la rueda" },
  { keys: "Ctrl+Space", label: "Launcher" },
  { keys: "Esc", label: "Cerrar" },
];

export type { IconNode };
