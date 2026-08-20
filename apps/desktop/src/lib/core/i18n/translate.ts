/** Diccionarios de UI. Sin Svelte: el locale lo inyecta quien llama. */

import { en } from "./en";
import { es } from "./es";

export type Locale = "es" | "en";
export type Dict = { readonly [key: string]: string | Dict };

export function parseLocale(raw: string | undefined | null): Locale {
  return raw === "en" ? "en" : "es";
}

function lookup(tree: Dict, path: string): string | undefined {
  let cur: string | Dict | undefined = tree;
  for (const part of path.split(".")) {
    if (cur == null || typeof cur === "string") return undefined;
    cur = cur[part];
  }
  return typeof cur === "string" ? cur : undefined;
}

function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    vars[name] == null ? `{${name}}` : String(vars[name]),
  );
}

const TABLES: Record<Locale, Dict> = { es: es as Dict, en: en as Dict };

export function translate(
  locale: Locale,
  key: string,
  vars?: Record<string, string | number>,
): string {
  const found = lookup(TABLES[locale], key) ?? lookup(es, key) ?? key;
  return interpolate(found, vars);
}
