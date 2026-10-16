# GPUI parcheado

`vendor/gpui` es `gpui` 0.2.2 de crates.io con dos arreglos para macOS. Lo usa
toda la pill (y `../gpui-m3`) por el `[patch.crates-io]` de `Cargo.toml`. Los
cambios son de código compartido o de `platform/mac`: Windows compila lo mismo.

El diff exacto contra el original está en `vendor/gpui.patch`.

## Qué cambia

1. **`src/platform/mac/window.rs`, `window_did_change_key_status`.** Con un
   «se volvió la ventana activa» que no lo es (pasa con el panel del notch y con
   ventanas que se abren con la app inactiva), GPUI llamaba a `resignKeyWindow`
   con el candado de la ventana tomado. AppKit vuelve a entrar al mismo
   manejador con `windowDidResignKey:`, que pide el mismo candado: la pill se
   quedaba pegada para siempre. Ahora suelta el candado antes.
2. **`src/window.rs`, `update_window_or_defer`.** Cuando el sistema avisa un
   cambio de tamaño, de posición, de apariencia o de ventana activa en medio de
   una actualización de GPUI (AppKit llama de vuelta en el acto desde
   `setFrame:` o `makeKeyAndOrderFront:`), GPUI no podía tomar la app y perdía
   el aviso («RefCell already borrowed»): la ventana quedaba con un tamaño o un
   estado de foco viejo. Ahora el aviso se reintenta en la siguiente vuelta.

## Qué se sacó (sin cambio de código)

`examples/`, `docs/`, `tests/`, `Cargo.toml.orig`, `.cargo-ok`,
`.cargo_vcs_info.json`, y los bloques `[[example]]`/`[[test]]` de
`Cargo.toml`.

## Rehacerlo con otra versión

```bash
G=~/.cargo/registry/src/index.crates.io-*/gpui-<versión>
rm -rf vendor/gpui && mkdir vendor/gpui
cp -R $G/src $G/resources $G/build.rs $G/README.md $G/LICENSE-APACHE $G/Cargo.toml vendor/gpui/
# borrar los [[example]]/[[test]] de vendor/gpui/Cargo.toml
patch -p1 -d vendor/gpui < vendor/gpui.patch   # revisar a mano si no aplica
```

Después probar en Mac abrir Ajustes y Reuniones desde el notch y escribir en
el lanzador (`set_focusable`, `bring_to_front` y `make_key` en
`src/platform/macos/win.rs`).
