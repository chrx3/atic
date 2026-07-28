# Ajustes y onboarding

**Estado:** `hecho`

## Resumen

Configuración de la app (audio, dictado, atajos, proveedores de resumen,
sonidos, autostart, tema) y onboarding de primer uso con nota de
consentimiento.

## Cómo se usa

- Abrir Ajustes desde la UI principal / pill.
- Configurar dispositivos, backends de dictado, modelos Whisper, SMTP, etc.
- Autostart y modo parlantes desde ajustes.
- Primer arranque: flujo de consentimiento / onboarding.

## Código

- [`crates/core/src/config.rs`](../crates/core/src/config.rs) — schema de config
- [`apps/desktop/src/lib/SettingsModal.svelte`](../apps/desktop/src/lib/SettingsModal.svelte)
- [`apps/desktop/src-tauri/src/state.rs`](../apps/desktop/src-tauri/src/state.rs)

## Pendiente / siguiente

- [ ] Agrupar ajustes por “producto” (reuniones / dictado / agentes) si crece la lista
- [ ] Exponer preferencias del launcher cuando exista

## Relacionado

- [grabacion-reuniones.md](grabacion-reuniones.md)
- [dictado.md](dictado.md)
- [agentes.md](agentes.md)
