/** Un ítem de `Menu`. Vive fuera del componente para poder tiparlo desde afuera. */
import type { IconNode } from "morphicons/svelte";

export type MenuItem<T extends string> = {
  id: T;
  label: string;
  /** Icono Lucide ya resuelto (`TOOL_ICONS[id]` o el export suelto). */
  icon?: IconNode;
  /** Lo que hace, cuando la etiqueta sola no alcanza. */
  hint?: string;
  /** Acción que no se puede deshacer: se pinta en rojo. */
  danger?: boolean;
  disabled?: boolean;
  /** Ítem activo: se marca con el tilde, como en un menú del sistema. */
  checked?: boolean;
};
