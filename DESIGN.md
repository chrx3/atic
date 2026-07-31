# Atic Floating UI V2 — Pearl Flow

## Intent

La experiencia flotante se comporta como una sola pieza de material satinado.
La pill no abre ventanas: se estira, desprende herramientas y alimenta una
superficie mayor mediante un cuello líquido. La expresividad se concentra en
esa transformación; el contenido permanece sobrio y familiar.

## Scene

Una persona trabaja varias horas frente a un monitor, con luz ambiental variable
y otra aplicación ocupando casi toda su atención. Atic debe leerse sobre fondos
claros u oscuros sin dominar el escritorio.

## Color strategy

Restrained. Neutros con un único acento funcional visible por estado.

| Token | OKLCH | Uso |
| --- | --- | --- |
| `pearl` | `oklch(0.965 0.006 255)` | Material base |
| `pearl-deep` | `oklch(0.915 0.009 255)` | Superficie secundaria |
| `ink` | `oklch(0.22 0.012 255)` | Texto e iconos |
| `muted` | `oklch(0.48 0.014 255)` | Metadatos |
| `signal` | `oklch(0.62 0.18 252)` | Selección y foco |
| `record` | `oklch(0.61 0.20 28)` | Grabación y riesgo |

El material oscuro usa el mismo eje cromático, no negro puro. Los colores de
estado ocupan menos del diez por ciento de la superficie.

## Typography

- Interfaz: `Aptos`, `SF Pro Text`, `Segoe UI Variable`, `system-ui`.
- Datos técnicos: `Cascadia Mono`, `SFMono-Regular`, monospace.
- Escala fija: 12 / 13 / 15 / 18 px. Sin tipografía display en controles.
- Etiquetas en sentence case; mayúsculas solo para siglas reales.

## Geometry

- Pill en reposo: 48 px de alto; disco principal de 48 px.
- Objetivos interactivos: mínimo 36 px.
- Panel de herramienta: 360–440 px de ancho según contenido.
- Agentes: 520–640 px, persistente y redimensionable en una etapa posterior.
- Radios de panel: 14–16 px. Solo la pill y los nodos circulares usan radio total.
- La piel filtrada y el contenido son capas hermanas con geometría compartida.

## Motion

- Respuesta táctil: 140–170 ms.
- Transformación de superficie: 220–260 ms.
- Salida: 160–200 ms.
- Curva principal: `cubic-bezier(0.22, 1, 0.36, 1)`.
- Sin bounce ni elasticidad gratuita. La viscosidad proviene de la silueta y de
  la continuidad espacial, no de hacer rebotar los controles.
- Reduced motion: crossfade de 100 ms y cambio inmediato de geometría.

## Signature

La firma es la unión visible entre origen y destino. Mientras dos superficies
pertenezcan al mismo flujo, una cápsula o serie de gotas mantiene el cuello. Al
superar el alcance, el cuello se afina y se corta; la separación comunica que la
superficie ya es persistente.

## Component vocabulary

- `LiquidScene`: sistema de coordenadas y estados.
- `LiquidSkin`: una sola silueta SVG calculada desde un campo SDF. Las cajas se
  unen con `smooth-min`; marching squares traza el contorno y Chaikin suaviza la
  curva antes de pintar la piel perlada.
- `LiquidInk`: contenido nítido alineado con la piel.
- `FloatingPill`: origen, actividad y regreso.
- `ToolWheel`: seis acciones de primera clase.
- `ToolSurface`: panel transitorio para reuniones, dictado, clipboard, textos y
  capturas.
- `AgentSurface`: conversación persistente conectada a la pill.
- `PlacementAdapter`: libre ahora; borde/Dynamic Island en una variante futura.

## Non-negotiables

- Lab y producción importan los mismos componentes.
- Un solo DOM para toda silueta que deba fusionarse.
- Ningún texto o icono dentro de la piel SVG; el contenido real permanece en DOM.
- `Blend` modifica el `k` del campo SDF, desde juntas nítidas hasta cuellos suaves.
- La interfaz sigue siendo utilizable con reduced motion.
- WebView2 es el entorno de validación visual autoritativo en Windows.
