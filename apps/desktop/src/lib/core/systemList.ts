/**
 * Qué filas ve el usuario: orden, filtro y búsqueda.
 *
 * Puro y aparte del store para poder probarlo. La lista que llega de Rust trae
 * las dos clases —apps y procesos de segundo plano— porque decidir cuál se ve
 * es de la vista, no del SO.
 */
export type SystemSort = "cpu" | "ram";

/**
 * Lo mínimo que la lista necesita saber de una fila.
 *
 * Estructural a propósito: `core` es TS puro y no conoce los tipos del IPC.
 * La fila que llega de Rust encaja sola.
 */
export type SystemRow = {
  id: string;
  name: string;
  cpu: number;
  ram_bytes: number;
  background: boolean;
};

/** Coincidencia por nombre o por clave, sin acentos ni mayúsculas. */
export function matchesQuery(app: SystemRow, query: string): boolean {
  const q = query.trim().toLocaleLowerCase();
  if (!q) return true;
  const campos = `${app.name} ${app.id}`
    .toLocaleLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "");
  return campos.includes(q.normalize("NFD").replace(/[̀-ͯ]/g, ""));
}

/**
 * Filas visibles, ya ordenadas.
 *
 * El segundo plano está apagado por defecto: son decenas de procesos y
 * convierten la lista en ruido. Con una búsqueda escrita se muestran igual —
 * si alguien escribe "node" es porque lo está buscando.
 */
export function visibleApps<T extends SystemRow>(
  apps: T[],
  opts: { sort: SystemSort; query: string; background: boolean },
): T[] {
  const query = opts.query.trim();
  const conFondo = opts.background || query.length > 0;
  const filas = apps.filter(
    (app) => (conFondo || !app.background) && matchesQuery(app, query),
  );
  return filas.sort((a, b) =>
    opts.sort === "ram" ? b.ram_bytes - a.ram_bytes : b.cpu - a.cpu,
  );
}

/** ¿Vale la pena ofrecer el interruptor? Solo si hay algo detrás. */
export function hasBackground(apps: SystemRow[]): boolean {
  return apps.some((app) => app.background);
}
