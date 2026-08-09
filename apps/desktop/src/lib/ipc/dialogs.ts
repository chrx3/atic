/**
 * Los diálogos de archivo del sistema.
 *
 * Existe por la regla de capas: `@tauri-apps/*` solo se importa desde `ipc/`.
 * Antes cada pantalla hacía su propio `await import("@tauri-apps/plugin-dialog")`
 * y armaba los filtros a mano, con lo cual el nombre de cada formato estaba
 * escrito en dos sitios.
 *
 * La carga es diferida a propósito: el plugin pesa y la mayoría de las sesiones
 * no abre ningún diálogo.
 */

const AUDIO_EXTENSIONS = ["wav", "mp3", "m4a", "flac", "ogg", "opus", "aac", "webm"];

const EXPORT_FILTER: Record<string, string> = {
  md: "Markdown",
  docx: "Documento de Word",
  pdf: "PDF",
};

/** Quita lo que Windows no acepta en un nombre de archivo. */
export function safeFileName(title: string, fallback: string): string {
  return (
    title
      .replace(/[<>:"/\\|?*]+/g, "-")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 100) || fallback
  );
}

/** Dónde guardar una exportación. `null` si se canceló. */
export async function pickExportPath(
  suggestedName: string,
  format: string,
): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({
    title: "Exportar reunión",
    defaultPath: `${suggestedName}.${format}`,
    filters: [{ name: EXPORT_FILTER[format] ?? format, extensions: [format] }],
  });
}

/** Archivos de audio a importar. Lista vacía si se canceló. */
export async function pickAudioFiles(): Promise<string[]> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    title: "Importar audio",
    multiple: true,
    filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/** Carpeta de trabajo. `null` si se canceló. */
export async function pickDirectory(title = "Elegir carpeta"): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    title,
    directory: true,
    multiple: false,
  });
  return typeof picked === "string" ? picked : null;
}

/** Identity file SSH (.pem / clave privada). `null` si se canceló. */
export async function pickSshIdentityFile(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    title: "Elegir identity file SSH",
    multiple: false,
  });
  return typeof picked === "string" ? picked : null;
}

/** Archivos para adjuntar al mensaje del agente. Lista vacía si se canceló. */
export async function pickAgentFiles(): Promise<string[]> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    title: "Adjuntar archivos",
    multiple: true,
    filters: [
      {
        name: "Imágenes",
        extensions: ["png", "jpg", "jpeg", "gif", "webp"],
      },
      {
        name: "Documentos",
        extensions: ["md", "txt", "pdf", "json", "csv", "ts", "js", "tsx", "jsx", "rs", "py"],
      },
    ],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}
