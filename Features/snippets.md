# Snippets

**Estado:** `hecho`

## Resumen

Textos reutilizables gestionados en Atic para pegar contenido repetitivo sin
salir del flujo de trabajo. La expansión automática por trigger todavía no
está implementada.

## Cómo se usa

- Definir snippets en la UI de la app.
- Invocarlos / pegarlos según el flujo de snippets (lista + pegado al destino).

## Código

- [`apps/desktop/src-tauri/src/snippets.rs`](../apps/desktop/src-tauri/src/snippets.rs)
- Persistencia local en `snippets.json`, `scratchpad.json` y `notes.json`,
  implementada en el mismo módulo.

## Pendiente / siguiente

- [ ] Expansión por trigger tipado en cualquier app (si se prioriza frente a pegado manual)
- [ ] Integración con un futuro launcher (buscar snippet por nombre)

## Relacionado

- [clipboard-historial.md](clipboard-historial.md)
- [launcher-spotlight.md](launcher-spotlight.md)
