# Trabajo con agentes en Atic

## Prioridad: probar cambios pronto

El usuario itera sobre la pill y sus herramientas flotantes. Reduce el tiempo
entre editar y ver el resultado. Aplica estas reglas independientemente del
modelo utilizado. Las instrucciones explícitas del usuario tienen prioridad.

1. Identifica si el cambio afecta frontend, Rust de la app o sidecar MCP.
2. Reutiliza la sesión de desarrollo existente. No levantes otra instancia ni
   reinicies la app por rutina. Comprueba qué proceso y checkout están activos.
3. Implementa un cambio pequeño y deja disponible su resultado para probar.
4. Valida el área afectada; reserva las comprobaciones amplias para el cierre.

## Qué ejecutar según el cambio

| Área modificada | Ciclo para ver el resultado | Validación dirigida |
| --- | --- | --- |
| Svelte/CSS de la pill o herramientas | Guardar y aprovechar HMR en la app abierta | Inspección visual; tipos/lint de lo afectado al cerrar |
| Lógica TypeScript | HMR; probar el comportamiento | Tests del archivo o módulo afectado |
| Rust de la app o crates que consume | Dejar que `tauri dev` recompile y reinicie | Check y tests del crate o módulo afectado |
| `crates/atic-mcp` | Compilar el sidecar y reconectar el cliente que lo usa | Check/tests de `atic-mcp`; probar la herramienta cambiada |
| `apps/web` | Servidor web con HMR | Validaciones web al cerrar |
| Solo documentación | Revisar contenido y diff | No compilar ni correr tests de la app |

Los tests no actualizan el ejecutable. `cargo check` tampoco genera un binario
actualizado. No confundas "compila", "pasaron los tests" y "lo probé en la app".

## Sesión de desarrollo

- Desde la raíz: `pnpm dev`. En macOS, si necesitas preparar el entorno, usa
  `bash apps/desktop/dev-atic.sh`. Lee `docs/MACOS.md` si hay problemas de entorno.
- Los cambios en `apps/desktop/src` normalmente usan HMR y no necesitan Rust.
  Si un cambio solo visual dispara una compilación, investiga el motivo.
- Para Rust, espera la recompilación de la sesión existente; no lances un build
  adicional mientras `tauri dev` ya está compilando el mismo cambio.
- Para el MCP, `pnpm --dir apps/desktop mcp:build` compila y copia el sidecar.
  Comprueba la ruta configurada en el cliente y reconecta solo el proceso
  afectado cuando sea seguro. No interrumpas sesiones de agentes con trabajo
  activo. Cambiar el sidecar no exige por sí solo recompilar toda la app;
  cambiar también el hub en `apps/desktop/src-tauri` sí afecta al backend.
- No uses `pnpm build`, instaladores ni scripts de release para iterar sobre UI.
  No borres `target/`, no ejecutes `cargo clean` ni cambies perfiles/cachés como
  optimización sin un problema concreto y evidencia. Conserva el perfil que
  esperan los scripts existentes del sidecar.

## Validación proporcional

- No ejecutes `pnpm verify`, `pnpm verify:all` ni tests de todo el workspace
  entre cada ajuste visual. Primero deja que el usuario vea el cambio.
- Para TypeScript: `pnpm --dir apps/desktop exec vitest run <ruta-del-test>`.
- Para Rust: `cargo check --locked -p <crate>` y
  `cargo test --locked -p <crate> <filtro-del-test>`, según lo que necesites
  comprobar. No encadenes check, build y tests por rutina si repiten trabajo.
- Al corregir lógica, usa o agrega una prueba de regresión relevante. Para
  cambios puramente visuales, no inventes tests que solo copien la implementación.
- Antes de subir, aplica las validaciones del área indicadas en
  `CONTRIBUTING.md`. Antes de un release, usa `pnpm verify:all`.
- Amplía las pruebas si cambias contratos compartidos, persistencia, permisos
  o comportamiento entre componentes. No elimines ni debilites pruebas para
  ahorrar tiempo. No repitas comprobaciones exitosas sin cambios relevantes.

## Comunicación y cuidado del trabajo

- Antes de una compilación costosa, explica qué componente la necesita y por
  qué. Si hay lentitud, mide la etapa que tarda antes de proponer optimizaciones.
- Avisa cuando el cambio ya esté disponible para probar, aunque aún falten
  validaciones. Solo afirma haberlo visto funcionar si lo comprobaste.
- Al cerrar, resume qué cambió, qué comprobaste y qué quedó sin validar.
- Usa español de Chile (tuteo). Conserva cambios ajenos y no publiques releases
  ni hagas push salvo que el usuario lo haya solicitado.
