# GPUI parcheado

`vendor/gpui` es `gpui` 0.2.2 de crates.io con cinco arreglos para macOS. Lo usa
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
3. **`src/platform/mac/window.rs`, `move_traffic_light`.** Con barras más
   altas que la nativa (Atic Code pide los semáforos centrados en 50 px), GPUI
   los bajaba fuera de su contenedor (`NSTitlebarContainerView`, de 28 px) y
   AppKit los recortaba: no se veían o quedaban a medias y sin clics. Ahora el
   contenedor crece hasta `2·y + alto del botón` (como Electron con
   `trafficLightPosition`), y al salir de pantalla completa
   (`windowDidExitFullScreen:`) se vuelven a poner.
4. **`src/window.rs`, `on_request_frame` y `on_hover_status_change`.** AppKit
   también pide un cuadro en el acto (`windowDidBecomeKey:`, `displayLayer:`)
   cuando otra ventana cambia dentro de una actualización: al cerrar una, la
   siguiente pasa a ser la activa. Con la app tomada, cada `update` del cuadro
   fallaba («RefCell already borrowed», de a dos o tres seguidos en el registro)
   y se perdían los `on_next_frame` ya sacados de la cola. Ahora ese cuadro se
   salta (`app_busy`): la ventana sigue sucia y el display link la dibuja en el
   siguiente vsync, con sus callbacks. El aviso de hover usa
   `update_window_or_defer` como los de tamaño y foco.

5. **`src/platform/mac/metal_renderer.rs` y `window.rs`, el lienzo recortado.**
   La ventana de la pill es transparente y cubre la pantalla, pero solo se ve
   la pill. GPUI igual pedía drawables, la textura intermedia de los trazos y
   su copia con antialiasing ×4 del tamaño de toda la ventana: en una pantalla
   Retina, más de 200 MB de memoria gráfica. Ahora el drawable va desde la
   esquina superior izquierda hasta donde pinta la escena (`scene_extent`, en
   pasos de 64 px, y se achica solo si sobra más de la mitad). La capa sigue
   del tamaño de la vista y muestra el drawable arriba a la izquierda sin
   escalarlo (`contentsGravity` y `layerContentsPlacement` *top left*): un
   cuadro que llega atrasado cae en su lugar y nunca se ve estirado. (Mover y
   achicar la capa al recorte deformaba el notch por un cuadro.) Además, las
   texturas de los trazos se crean recién cuando hay trazos y se sueltan a los
   5 s sin ellos, y en GPUs de Apple la de antialiasing es `Memoryless`.

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
`src/platform/macos/win.rs`). Para el lienzo recortado, mirar que la pill se
vea entera al abrir y cerrar paneles y que Ajustes y Atic Code se dibujen bien
al cambiarles el tamaño.
