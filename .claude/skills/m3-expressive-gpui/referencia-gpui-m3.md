# Inventario de gpui-m3 (0.2.0 + sin publicar)

Rutas relativas a `../gpui-m3/src/`. Las líneas son las de la definición
(`pub struct`/`pub fn`) a noviembre de 2026; si no calzan, `grep -n "pub struct Nombre"`.
Todos los componentes se reexportan desde la raíz (`gpui_m3::Button`, `gpui_m3::animate_theme`, …);
`MermaidDiagram` solo con la feature `mermaid` (Atic la activa).

## Fundamentos

| Qué | Dónde |
|---|---|
| `init` (fuentes Inter/Roboto, atajos, tema y `MotionSettings`) | `lib.rs:31` |
| `Spring` y sus seis constantes | `motion.rs:20`, `:25-49` |
| `MotionSettings { time_scale, reduced }` | `motion.rs:93` |
| `SpringValue<N>` (estado de un resorte, `update`, `kick`) | `motion.rs:303` |
| `animate` / `animate_from` / `animate_f32` / `animate_px` / `animate_color` | `motion.rs:387`, `:399`, `:541`, `:553`, `:565` |
| `animate_on_activate` | `motion.rs:473` |
| `entrance` | `motion.rs:515` |
| `presence` | `motion.rs:708` |
| `expand(id, open, measured, spring)` / `Expand` | `motion.rs:776`, `:755` |
| `replay`, `play_on_rise` | `motion.rs:629`, `:663` |
| `continuous`, `continuous_at`, `ambient_clock`, `clock` | `motion.rs:216`, `:271`, `:243`, `:123` |
| `pulse` | `motion.rs:617` |
| `tick_delay`, `take_frame_requests` (con `frame-stats`) | `motion.rs:173`, `:196` |
| `cubic_bezier`, `key` | `motion.rs:584`, `:828` |
| `ShapeName` (38 formas: las 35 de M3 + Blob, Blob2, Pebble) | `shapes.rs:14` |
| `Morph { from, to, t, rotation, toward }`, `Morph::paint` | `shapes.rs:575`, `:631` |
| `LOADING_SEQUENCE` (7 formas del indicador) | `shapes.rs:647` |
| `Tone` (Primary, Secondary, Tertiary; **sin** `non_exhaustive`) | `shapes.rs:670` |
| `avatar_for(seed)` | `shapes.rs:688` |
| `Scheme` (todos los roles M3 + success/warning) | `theme.rs:66` |
| `Scheme::from_seed`, `from_seed_variant`, `SchemeVariant` (9) | `theme.rs:277`, `:283`, `:402` |
| `Theme` (`of`, `get`, `set`, `set_scheme`) | `theme.rs:455` |
| `Elevation` (6 niveles, `shadow`) | `theme.rs:504` |
| `corner::{NONE, XS, S, M, L, LI, XL, XLI, XXL, FULL}` | `theme.rs:48` |
| `type_scale::*` (`TypeStyle { size, line_height, weight, emphasized }`) | `theme.rs:606` |
| `TypeScaleExt::type_style` / `type_emphasized` (`use gpui_m3::theme::type_scale::{self, TypeScaleExt};`) | `theme.rs:644` |
| `mix`, `alpha`, `hex` | `theme.rs:567-586` |
| `interaction::use_interaction`, `interactive`, `state_layer`, `focus_ring`, `keyboard_focus` | `interaction.rs:47`, `:52`, `:117`, `:258`, `:296` |
| `animate_theme`, `animate_scheme`, `theme_reveal_at`, `ThemeReveal` (módulo privado: úsalos como `gpui_m3::…`; temporizador de 16 ms + `refresh_windows()`) | `components/theme_motion.rs:70`, `:26`, `:127`, `:157` |

## Componentes (`components/`)

| Grupo | Componente | Dónde | Notas de Expressive |
|---|---|---|---|
| Botones | `Button` | `button.rs:230` | Radio que se aplasta al presionar (`SPATIAL_FAST`), `M3ButtonSize` XS–XL, `selected`, `confirm`, `label_on_hover`, `icon_only` |
| | `IconButton` | `button.rs:665` | `hover_morph`, `launch(generation)`, `IconButtonWidth`, `square`, `selected` |
| | `Fab` | `button.rs:996` | Tamaños, `label` (extendido), `hover_radius` |
| | `FavStar` | `button.rs:1208` | |
| | `ButtonGroup` / `ButtonGroupItem` | `button_group.rs:93`, `:37` | Estándar (el presionado se ensancha) y `connected()` (el elegido se vuelve píldora) |
| | `SplitButton` | `chip.rs:405` | `sublabel`, `open`, `on_toggle` |
| | `FabMenu` | `fab_menu.rs:91` | Escalonado; `FabMenuColor` |
| Selección | `Switch` | `switch.rs:20` | Íconos en el pulgar |
| | `SegmentedButtons` | `segmented.rs:52` | `sliding`, multi, denso |
| | `Slider`, `RangeSlider`, `StopSlider` | `slider.rs:566`, `:601`, `:21` | Tamaños Expressive, vertical, burbuja |
| | `Chip`, `Badge` | `chip.rs:42`, `:571` | `Badge::pop(id)` (solo `Badge`), `Badge::dot()` |
| | `Radio`, `Checkbox`, `ChoiceRow` | `choice.rs:20`, `:150`, `:310` | Indeterminado, error |
| | `MorphDot` / `DotShape` | `visual.rs:919`, `:907` | Cuadrado ↔ círculo ↔ rombo con `SPATIAL_FAST` |
| Navegación | `NavItem` (`large`), `RailItem`, `IconTabs` | `nav.rs:20`, `:260`, `:394` | Ícono que salta al elegirse |
| | `TabStrip`/`Tab`, `TreeRow`, `ListItem` | `lists.rs:433`, `:24`, `:224` | `ListItem` **no** tiene `selected` |
| | `TopAppBar` (`collapse_progress`) | `top_app_bar.rs:150`, `:59` | Pequeña, centrada, mediana, grande |
| | `Toolbar` / `ToolbarItem` | `toolbar.rs:161`, `:32` | `floating`, `docked`, `vertical`, `vibrant`, `fab`; solo ítems de ícono |
| | `Carousel` / `CarouselLayout` | `carousel.rs:451`, `:54` | 5 layouts, `scroll_to` |
| Superficies | `Menu` / `MenuItem` | `surfaces.rs:243`, `:549` | `stagger`, `grouped`, `footer`, `actions` |
| | `Popover`, `pop_in`, `pop_in_toward`, `shadow_pop` | `surfaces.rs:94`, `:48`, `:53`, `:19` | Entrada que sube 12 px con `SPATIAL_FAST` |
| | `Card` (`entrance`), `ListGroup`, `Bubble` (`entrance`) | `surfaces.rs:961`, `:1100`, `:1201` | |
| | `ExpandableCard` | `disclosure.rs:29` | Alto con `motion::expand` |
| | `BottomSheet`, `SideSheet` | `sheets.rs:126`, `:307` | |
| | `Dialog`, `Toast`, `Tooltip`, `context_menu`, `LinearProgress` | `overlays.rs:44`, `:246`, `:355`, `:321`, `:386` | `LinearProgress`: determinado, relleno con resorte, no continuo |
| | `RichTooltip`, `Banner`, `DividerLabel`, `Divider` | `rich_tooltip.rs:38`, `feedback.rs:245`, `:30`, `divider.rs:40` | |
| | `Presence` / `Exit` | `presence.rs:47`, `:13` | Salidas animadas |
| Formas y progreso | `Shape` | `visual.rs:26` | `breathe`, `breathe_on_hover(id, otra)` + `hovered(bool)`, `spin`, `drift`, `rotate`, `scale`, `image` |
| | `Avatar` | `visual.rs:287` | |
| | `LoadingIndicator` (`contained`) | `visual.rs:456` | **Continuo a 15 fps** |
| | `WavyProgress`, `WavyCircularProgress` | `visual.rs:548`, `:730` | **Continuo a 30 fps, aun con valor fijo** |
| | `Ring` | `visual.rs:653` | Color fijo `primary` |
| Texto | `TextField`, `TextArea`, `SearchBar`/`SearchView` | `text_field.rs:188`, `text_area.rs:114`, `search.rs:250`, `:363` | Cursor a 4 fps |
| | `SuggestionList`, `CommandPalette` | `suggest.rs:249`, `palette.rs:65` | |
| | `DatePicker`, `TimePicker` | `date_picker.rs:562`, `time_picker.rs:281` | |
| | `SelectableText`, `selection_root` | `selectable.rs:906`, `:527` | |
| Código | `Markdown`, `CodeBlock`, `CodeOutput` (`clip_lines`), `CodeEditor`, `DiffView`, `DiffBar` | `markdown.rs:823`, `code.rs:177`, `:25`, `code_editor.rs:221`, `diff.rs:387`, `:635` | `clip_lines` corta por `\n` (un JSON de una línea no se corta) |
| Ajustes | `SettingRow`, `Kbd`, `KeyCombo` | `setting_row.rs:20`, `keys.rs:152`, `:225` | |
| Apariencia | `SelectCard`, `ColorSwatches`, `HsvPicker` | `color.rs:23`, `:140`, `:368` | |
| Otros | `DropZone`, `ImageThumb`, `ImageCropper`, `ReorderList`, `ResizeHandle`, `Splitter`, `MermaidDiagram` | `media.rs:22`, `:172`, `image_cropper.rs:123`, `reorder.rs:86`, `resize.rs:39`, `:276`, `mermaid.rs:36` | |

## Qué piden cuadros (y a cuántos fps)

| Fuente | Ritmo | Se detiene |
|---|---|---|
| `animate*`, `entrance`, `presence`, `expand`, `animate_on_activate` | Cada cuadro de GPUI (sin tope propio) | Sí, al asentarse el resorte |
| `LoadingIndicator` | 15 fps (4 atrás) | No, mientras se dibuje |
| `WavyProgress`, `WavyCircularProgress`, `pulse`, `Chip` con `pulse_icon`, `DividerLabel` pendiente con estilo `Pill` | 30 fps (4 atrás) | No |
| `Shape::breathe/spin/drift` | 30 fps; atrás se congelan (no piden cuadros) | No |
| Cursor de `TextField`/`TextArea`/`CodeEditor` | 4 fps | Solo con foco |
| Ripple (`interaction`) | Cada cuadro de GPUI | Al terminar la onda |

`continuous_at` avisa a `window.current_view()` (la vista que lo dibuja) con un
temporizador pendiente por entidad; con `fps >= 60` usa `request_animation_frame`. El aviso
invalida también sus ancestros: en Atic Code, el `LoadingIndicator` va en `SpinnerLayer`
(`code/spinners.rs`, solo ese indicador), nunca en una sección ni en la raíz; otro continuo
necesita su propia capa hermana (ver SKILL.md, sección 5).

## Gallery y changelog

- `cargo run --example gallery` (`M3_SECTION=<n>`, `M3_MODE=light|dark`,
  `M3_SEED=<n>`, `M3_SIZE=WxH`); interruptor **Slow ×5**. Secciones en
  `examples/gallery/`.
- `changelog/changelog.json` → `node scripts/changelog.mjs md`; ver
  `changelog/README.md`. La versión en curso es «Sin publicar» en `CHANGELOG.md`.
