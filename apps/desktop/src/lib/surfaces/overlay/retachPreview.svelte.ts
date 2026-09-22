/**
 * Vista previa del imán: qué panel se colocaría en la isla si se soltara ya.
 * Lo escribe el float que se arrastra y lo pinta la pill.
 */

export type RetachTool = "clipboard" | "snippets" | "system" | "agents";

export const retachPreview = $state<{ tool: RetachTool | null }>({
  tool: null,
});

/** Apagar solo borra lo propio: otro float pudo tomar la vista previa. */
export function showRetachPreview(tool: RetachTool, on: boolean): void {
  if (on) retachPreview.tool = tool;
  else if (retachPreview.tool === tool) retachPreview.tool = null;
}
