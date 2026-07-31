# Plan de implementación — UI flotante V2

## Objetivo

Reemplazar la experiencia flotante por un compositor único, pulido y verificable
que cubra pill, rueda, reuniones, dictado, clipboard, textos, capturas y agentes.
El backend Rust, almacenamiento y comandos existentes se conservan.

## Decisión arquitectónica

```text
Tauri commands ─┐
Fake lab data ──┴─> adapter -> controller -> shared liquid renderer -> host
```

El renderer no conoce Tauri. Producción y lab cambian únicamente el adapter.
Toda superficie que se fusione vive en el mismo documento.

## Fases

### 1. Renderer y lab compartidos

- Tokens V2 y piel SVG única generada con SDF, smooth-union y marching squares.
- Control `Blend` en el lab para recorrer juntas geométricas y fusión orgánica.
- Estado explícito: `idle`, `wheel`, `tool`, `agents`.
- Pill, seis herramientas y panel representativo con datos falsos.
- Navegación por mouse y teclado; reduced motion.

### 2. Host de escritorio

- La rama `codex/floating-ui-v2` apunta la ventana `pill` a `/pill-v2`; la ruta
  anterior se conserva para comparación y rollback hasta completar QA visual.
- Una escena por monitor activo, empezando por un monitor.
- Hit-testing transparente fuera de superficies vivas.
- Foco activable solo cuando hay campos de texto.
- Medición, recorte y rendimiento validados dentro de WebView2.

### 3. Conexión funcional

- Reuniones y dictado.
- Clipboard y textos.
- Capturas.
- Agentes y permisos.

### 4. Robustez

- DPI mixto y cambio de monitor.
- Estados vacíos, error, carga y tareas largas.
- Tema oscuro y contraste sobre fondos variables.
- Trazas de geometría y medición de frames largos.

### 5. Variante futura de borde

`PlacementAdapter` permitirá fijar la misma experiencia a un borde de pantalla y
hacerla nacer de él, al estilo de una Dynamic Island. No se duplica el renderer.

## Criterios de aceptación

- Abrir/cerrar rueda diez veces no produce recorte ni salto de posición.
- Cada herramienta se alcanza con teclado y mouse.
- La primera pulsación llega al control correcto.
- Escribir en agentes no roba ni devuelve foco de forma inesperada.
- Reduced motion conserva todos los estados sin desplazamientos.
- `pnpm check`, autofixer Svelte, Rust fmt/clippy/tests y build pasan.
- Validación visual final realizada dentro de WebView2, no solo en navegador.

## Estado verificado · 2026-07-31

- `pnpm check`: 0 errores y 0 warnings.
- `pnpm build`: build estático correcto; `/pill-v2` y `/lab/floating-v2`
  responden HTTP 200 desde preview.
- Test geométrico: contornos cerrados, puente pill-panel, apertura hacia arriba,
  caché y variación de `Blend` comprobados.
- Perfil CPU orientativo: 60 contornos de la escena de agentes en 208.2 ms
  (3.47 ms por cálculo en esta máquina).
- `cargo fmt --all --check` y `cargo check --workspace`: correctos.
- `cargo test --workspace`: la compilación no terminó dentro del límite de cinco
  minutos; no se considera aprobado ni fallido.
- QA visual WebView2: pendiente. Las capacidades de navegador y control de
  Windows no estuvieron disponibles en esta sesión; no se sustituye por una
  afirmación visual sin evidencia.
