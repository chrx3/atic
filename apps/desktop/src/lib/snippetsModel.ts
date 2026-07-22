import type { Snippet } from "./types";

export function emptySnippet(): Snippet {
  return {
    id: "",
    name: "",
    body: "",
    aliases: [],
    updatedAtMs: 0,
  };
}
