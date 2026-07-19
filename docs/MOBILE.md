# Fase 6 — Companion móvil (futuro)

Documento de alcance. **No implementado** en el código; solo preparación
arquitectónica (`#[cfg_attr(mobile, tauri::mobile_entry_point)]` en
`apps/desktop/src-tauri`).

## Qué será

App companion Tauri 2 (iOS / Android) que reutiliza los crates:

- `atic-core` — grabaciones, SQLite, config
- `atic-transcribe` / `atic-summarize` / `atic-mailer` — según dispositivo
- `atic-audio` — **solo micrófono** (reuniones presenciales)

## Qué no será

iOS y Android **no permiten** capturar el audio de llamadas VoIP del sistema
(Teams, Meet, Zoom, teléfono). No se promete paridad con el escritorio en
grabación de llamadas.

## Alcance mínimo (MVP)

1. Ver biblioteca de grabaciones / resúmenes (sync o import desde escritorio).
2. Reenviar un resumen por correo / compartir.
3. Grabar audio ambiental por micrófono y, si el dispositivo aguanta, transcribir
   en local o subir solo el texto a un backend BYOK.

## Sync (por decidir)

Opciones, de más simple a más compleja:

- Export/import ZIP (WAV + JSON) por AirDrop / carpeta compartida
- Carpeta en OneDrive/iCloud sincronizada
- API local en la LAN (escritorio como servidor)

## Cómo arrancar cuando toque

```bash
cd apps/desktop
pnpm tauri android init   # o ios init (requiere macOS)
```

Luego adaptar UI (rutas táctiles) y permisos de micrófono / notificaciones.
