# Atic · sitio y demo

El sitio de Atic y su demo interactivo. Es una app de SvelteKit con salida
estática: se compila a HTML/CSS/JS y se sube a cualquier host.

```bash
cd apps/web
pnpm install
pnpm dev        # http://localhost:5180
pnpm build      # → build/
pnpm check      # svelte-check
```

## Qué hay adentro

| Ruta | Qué es |
| --- | --- |
| `src/routes/+page.svelte` | La página: hero, demo, herramientas, videos, privacidad y descarga. |
| `src/lib/site/` | Las secciones del sitio, Nav, tema y pie. |
| `src/lib/demo/` | El demo: pill + rueda, floats de cada herramienta, pizarra y capturas. |
| `src/lib/atic/` | Piezas portadas de la app: marca, iconos, fusión líquida. |

## Reglas que hay que respetar

**1. Los tokens no se copian.** `src/app.css` importa las paletas, `tokens.css`,
`scales.css`, `motion.css` y `layers.css` directo de `apps/desktop/src`. Si la
app cambia una curva o un color, el sitio cambia con ella. Lo único duplicado
son los tokens de morph que en la app viven en `app.css` (el sitio no importa
ese archivo entero porque tiene el árbol viejo de la app); están comentados
como tales.

**2. Las piezas de la app se importan por `$atic`.** El alias apunta a
`apps/desktop/src` (ver `svelte.config.js`). Así el catálogo de herramientas
(`$atic/lib/core/tools`) y los textos de cada herramienta son los mismos que
muestra la app.

**3. La pill ES la rueda.** `src/lib/demo/Pill.svelte` es el disco cerrado y el
núcleo de la rueda en la misma pieza, como en la app: las gotas se desprenden
del núcleo por el filtro de goo (`src/lib/atic/liquid.ts`). El anillo y el
tamaño de las gotas están ajustados para siete gajos y explicados en el
comentario del componente.

**4. El contenido del demo es verosímil.** Datos en `src/lib/demo/data.ts`;
las capturas y la pizarra se dibujan en canvas (`captureArt.ts`) porque en el
navegador no hay pantalla que recortar.

## Videos (Remotion)

La sección de videos vive en `src/lib/site/VideoSlot.svelte`. Se produce cada
video con Remotion, se exporta a `static/videos/` y se agrega al array
`VIDEOS`; mientras el array esté vacío se muestra el marco con el aviso.

## Desplegar

Cualquier host estático sirve el `build/`: Vercel, Netlify, GitHub Pages o un
bucket. Es un sitio sin servidor — no hay variables de entorno ni API.
