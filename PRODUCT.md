# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Primary: power users de productividad en escritorio (launcher, clipboard, snippets, capturas) que también viven en flujo de desarrollo y conocimiento — reuniones, dictado y agentes CLI sin salir del contexto. Situación típica: Windows o macOS, trabajo multitarea, atajos globales y UI flotante en lugar de una app “ventana grande”.

## Product Purpose

Atic es una caja de herramientas de escritorio local-first: graba audio del PC, transcribe (Whisper local o Groq opcional), resume (BYOK), y además ofrece dictado, historial de clipboard, snippets, capturas, agentes de consola (Claude Code / Codex / OpenCode / Cursor) y un launcher tipo Spotlight. Vive en pill flotante, bandeja y atajos globales; la ventana principal es para biblioteca y ajustes. Éxito = completar esos flujos sin interrumpir el trabajo en otras apps.

## Positioning

No es “otra ventana de chat” ni un índice Everything del disco. Su diferencia: herramientas reales del SO y del CLI del usuario (agentes con la sesión ya instalada; launcher de apps/acciones; captura/clipboard/dictado al foco real), empaquetadas en una presencia no intrusiva (pill + atajos) en lugar de una app que hay que ir a buscar.

## Operating Context

- Overlay flotante (pill → rueda de siete tools → floats: launcher, clipboard, snippets, agentes, capturas, pizarra).
- Atajos globales configurables; bandeja para mostrar/ocultar y salir.
- Windows primario en desarrollo; macOS en progreso (system audio fase 4).
- Datos locales (`atic.db3`, Whisper on-demand); BYOK para resumen; consent onboarding.

## Capabilities and Constraints

- Siete tools en la rueda + launcher (no en la rueda).
- Local-first con excepciones explícitas (Groq si el usuario elige; proveedores de resumen reciben texto).
- Atic no autentica agentes: se cuelga del CLI ya logueado.
- Morph pill → chrome launcher aún incompleto (vuelo + `.float-emerge`).
- **Undecided:** estándar formal de accesibilidad (WCAG u otro) — no fijado por el producto todavía.
- Platform design language recorded as `web` (UI SvelteKit dentro de shell Tauri desktop); no es iOS/Android nativo.

## Brand Commitments

- Nombre de producto: **Atic** (`com.ciat.atic`).
- Presencia de marca en el núcleo de la pill / ParticleWheel (marca fija al morflear).

## Evidence on Hand

- Catálogo vivo: `Features/` (README + fichas por capacidad).
- README raíz y `docs/` para arquitectura y planes.
- No fabricar testimonios, benchmarks ni claims de mercado ausentes del repo.

## Product Principles

1. No interrumpir: la pill y los atajos ganan sobre ventanas permanentes.
2. Herramientas reales: actuar sobre el foco, el SO y el CLI del usuario, no simular un sandbox aparte.
3. Local-first con honestidad: lo que sale del máquina es elección explícita del usuario.
4. Una presencia, muchas herramientas: la rueda y los floats son la misma familia espacial.
5. Control del usuario: sugerir (p. ej. detectar llamada), nunca forzar grabación u acciones destructivas.

## Accessibility & Inclusion

Sin estándar formal confirmado todavía (open). Preservar `prefers-reduced-motion` y caminos de teclado ya existentes en overlays; no afirmar cumplimiento WCAG hasta decidirlo.
