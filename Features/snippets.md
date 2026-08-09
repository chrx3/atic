# Snippets

**Estado:** `hecho`

## Resumen

Textos reutilizables gestionados en Atic para pegar contenido repetitivo sin
salir del flujo de trabajo. La expansión automática por trigger todavía no
está implementada.

## Cómo se usa

- Abrir desde la pill (rueda / atajo): float independiente en el overlay con el
  morph fused grow → separate + reverse close
  ([pill-liquid-emerge.md](pill-liquid-emerge.md)).
  Tabs Textos / Notas.
- Pegar cierra el float.

## Código

- [`apps/desktop/src-tauri/src/snippets.rs`](../apps/desktop/src-tauri/src/snippets.rs) — `show_snippets_window`
- [`apps/desktop/src/lib/surfaces/overlay/snippets/SnippetsFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/snippets/SnippetsFloat.svelte)
- Persistencia local en `snippets.json`, `scratchpad.json` y `notes.json`.

## Pendiente / siguiente

- [ ] Expansión por trigger tipado en cualquier app (si se prioriza frente a pegado manual)
- [ ] Integración con un futuro launcher (buscar snippet por nombre)

## Relacionado

- [clipboard-historial.md](clipboard-historial.md)
- [launcher-spotlight.md](launcher-spotlight.md)
- [pill-liquid-emerge.md](pill-liquid-emerge.md)
