# Ajustes y onboarding

**Estado:** `parcial`

## Resumen

Configuración de la app (audio, dictado, atajos, proveedores de resumen,
sonidos, autostart, tema) y onboarding de primer uso: consentimiento, Groq o
Whisper local, modelos, atajos y una práctica guiada junto a la pill.

## Cómo se usa

- Abrir Ajustes desde la UI principal / pill.
- Configurar dispositivos, backends de dictado, modelos Whisper, SMTP, etc.
- Autostart y modo parlantes desde ajustes.
- Primer arranque: consentimiento → dictado (Groq recomendado sin gráfica) →
  modelos locales → confirmar atajos → practicar rueda, dictado y portapapeles.
- Se puede repetir cuando se quiera: birrete en la barra de la ventana, o
  Ajustes → General → Repetir tutorial.
- Groq acelera el dictado (y la vista en vivo si se activa después). Las
  reuniones se transcriben en local por defecto; en Reuniones o Ajustes se
  puede pasar a Groq (misma API key).
- Ajustes → Información: versión, GitHub y **Buscar actualizaciones**. Si hay
  un release más nuevo, aparece **Actualizar** (descarga el .exe, instala y
  reinicia).

## Código

- [`crates/core/src/config.rs`](../crates/core/src/config.rs) — schema de config
- [`apps/desktop/src/lib/features/onboarding/OnboardingModal.svelte`](../apps/desktop/src/lib/features/onboarding/OnboardingModal.svelte)
- [`apps/desktop/src/lib/features/onboarding/PracticeCoach.svelte`](../apps/desktop/src/lib/features/onboarding/PracticeCoach.svelte)
- [`apps/desktop/src/lib/features/settings/DictationSection.svelte`](../apps/desktop/src/lib/features/settings/DictationSection.svelte)
- [`apps/desktop/src/lib/features/settings/SettingsPanel.svelte`](../apps/desktop/src/lib/features/settings/SettingsPanel.svelte)

## Pendiente / siguiente

- [ ] Agrupar ajustes por “producto” (reuniones / dictado / agentes) si crece la lista
- [ ] Exponer preferencias del launcher cuando exista

## Relacionado

- [grabacion-reuniones.md](grabacion-reuniones.md)
- [dictado.md](dictado.md)
- [agentes.md](agentes.md)
