/**
 * Lo que una ficha de chat acepta desde afuera: el historial del
 * portapapeles pega en el composer en vez de en una PTY.
 */
export type ChatInsert = {
  insertText: (text: string) => void;
  attachImage: (path: string) => void;
};
