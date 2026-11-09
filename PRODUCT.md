# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Primary: power users de productividad en escritorio (launcher, clipboard, snippets, capturas) que también viven en flujo de desarrollo y conocimiento — reuniones, dictado y agentes CLI sin salir del contexto. Situación típica: Windows o macOS, trabajo multitarea, atajos globales y UI flotante en lugar de una app “ventana grande”.

## Product Purpose

Atic es una caja de herramientas de escritorio local-first: graba audio del PC, transcribe (Whisper local o Groq opcional), resume (BYOK), y además ofrece dictado, historial de clipboard, snippets, capturas, agentes de consola (Claude Code / Codex / OpenCode / Cursor) y un launcher tipo Spotlight. Vive en pill flotante, bandeja y atajos globales; Reuniones, Ajustes y Atic Code se abren en su propia ventana. Éxito = completar esos flujos sin interrumpir el trabajo en otras apps.

## Positioning

No es “otra ventana de chat” ni un índice Everything del disco. Su diferencia: herramientas reales del SO y del CLI del usuario (agentes con la sesión ya instalada; launcher de apps/acciones; captura/clipboard/dictado al foco real), empaquetadas en una presencia no intrusiva (pill + atajos) en lugar de una app que hay que ir a buscar.

## Operating Context

- Pill flotante: acoplada a un borde (en Mac, al notch), tira de herramientas al pasar el mouse o burbuja con rueda; cada herramienta con panel se abre en el notch.
- Atajos globales configurables; ícono en la bandeja o la barra de menús.
- Windows y macOS: la misma app (`apps/atic`), con una capa nativa por sistema (`platform/win32`, `platform/macos`).
- Datos locales (`atic.db3`, modelos de transcripción bajo demanda); BYOK para resumen; consent onboarding.

## Capabilities and Constraints

- Catálogo de 12 herramientas en la rueda y la tira; Dictado, Apps (launcher), Copiar texto y la calculadora van por atajo. Detalle en `docs/HERRAMIENTAS.md`.
- Local-first con excepciones explícitas (Groq si el usuario elige; proveedores de resumen reciben texto).
- Atic no autentica agentes: se cuelga del CLI ya logueado.
- **Undecided:** estándar formal de accesibilidad (WCAG u otro) — no fijado por el producto todavía.
- App de escritorio en Rust puro con GPUI (`docs/GPUI.md`); ya no hay Tauri ni UI web. `Platform` queda en `web` solo porque el esquema no tiene un valor para escritorio (`web`, `ios`, `android`, `adaptive`): no es iOS/Android nativo.

## Brand Commitments

- Nombre de producto: **Atic** (`com.ciat.atic`).
- Presencia de marca en la cara de la pill.

## Evidence on Hand

- Catálogo vivo: `Features/` (README + fichas por capacidad).
- README raíz, `AGENTS.md` y `docs/` (`GPUI.md`, `HERRAMIENTAS.md`) para arquitectura y herramientas.
- No fabricar testimonios, benchmarks ni claims de mercado ausentes del repo.

## Product Principles

1. No interrumpir: la pill y los atajos ganan sobre ventanas permanentes.
2. Herramientas reales: actuar sobre el foco, el SO y el CLI del usuario, no simular un sandbox aparte.
3. Local-first con honestidad: lo que sale del máquina es elección explícita del usuario.
4. Una presencia, muchas herramientas: la rueda y los floats son la misma familia espacial.
5. Control del usuario: sugerir (p. ej. detectar llamada), nunca forzar grabación u acciones destructivas.

## Accessibility & Inclusion

Sin estándar formal confirmado todavía (open). Preservar el respeto a «reducir movimiento» del sistema y los caminos de teclado ya existentes; no afirmar cumplimiento WCAG hasta decidirlo.
