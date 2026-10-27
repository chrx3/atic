//! Prueba de la vista web del sistema (wry: WKWebView en macOS, WebView2 en
//! Windows) dentro de una ventana GPUI, antes de armar el panel de Atic Code.
//!
//! Mide cuánto tarda en cargar cada página y deja probar a mano lo delicado:
//! un menú de GPUI encima de la página, ocultar y mostrar, cambiar el tamaño
//! del panel y el borde redondeado.
//!
//! `cargo run --manifest-path prototypes/pill-gpui/Cargo.toml --example webview_probe -- [url]`

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    actions, anchored, canvas, deferred, div, point, prelude::*, px, rgb, size, App, Application, Bounds, Context, Menu,
    MenuItem, OsAction, SharedString, Window, WindowBounds, WindowOptions,
};

// El menú Edición de la pill: «Copiar» con la acción del sistema (`copy:`), que AppKit
// manda a quien tenga el foco, también a la vista web.
actions!(probe, [Copy, Paste, SelectAll]);
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder};

/// Lo que escribe la vista web desde su propio hilo: cuándo terminó de cargar.
#[derive(Default)]
struct Loads {
    started: Option<Instant>,
    log: Vec<String>,
}

const EDIT_KEYS: &str = r#"
addEventListener('keydown', e => {
  const mod = navigator.platform.startsWith('Mac') ? e.metaKey : e.ctrlKey;
  if (!mod || e.altKey) return;
  const k = e.key.toLowerCase();
  if (k === 'v') { e.preventDefault(); window.ipc.postMessage('paste'); return; }
  const cmd = { c: 'copy', x: 'cut', a: 'selectAll', z: e.shiftKey ? 'redo' : 'undo' }[k];
  if (cmd) { e.preventDefault(); document.execCommand(cmd); }
}, true);
"#;

struct Probe {
    pending: Rc<RefCell<Option<String>>>,
    web: Rc<WebView>,
    loads: Rc<RefCell<Loads>>,
    menu: bool,
    hidden: bool,
    wide: bool,
    created_in: f32,
}

const PAGES: [(&str, &str); 3] = [
    ("Local", "http://127.0.0.1:8765/probe.html"),
    ("example.com", "https://example.com"),
    ("GitHub", "https://github.com/tauri-apps/wry"),
];

impl Probe {
    fn new(window: &mut Window, cx: &mut Context<Self>, url: String) -> Self {
        let loads = Rc::new(RefCell::new(Loads { started: Some(Instant::now()), log: Vec::new() }));
        let made = Instant::now();
        let on_load = loads.clone();
        let view = cx.entity().downgrade();
        // WKWebView espera copiar y pegar desde el menú Edición (`copy:`), y GPUI no se lo
        // pasa: en la página llegan como teclas sueltas. Este script hace la edición en el
        // mismo `keydown` (gesto del usuario, así WebKit lo permite); pegar lo pide a Rust,
        // que lee el portapapeles y lo inserta.
        let pending = Rc::new(RefCell::new(None::<String>));
        let asked = pending.clone();
        let web = WebViewBuilder::new()
            .with_initialization_script(EDIT_KEYS)
            .with_ipc_handler(move |request| {
                if request.body() == "paste" {
                    *asked.borrow_mut() = Some(String::new());
                }
            })
            .with_url(url)
            .with_bounds(Rect { position: LogicalPosition::new(0., 0.).into(), size: LogicalSize::new(10., 10.).into() })
            .with_on_page_load_handler(move |event, url| {
                if let PageLoadEvent::Finished = event {
                    let mut loads = on_load.borrow_mut();
                    let ms = loads.started.take().map_or(0., |t| t.elapsed().as_secs_f32() * 1000.);
                    let line = format!("cargó {url} en {ms:.0} ms");
                    println!("{line}");
                    loads.log.push(line);
                    let _ = &view;
                }
            })
            .build_as_child(&*window)
            .expect("no se pudo crear la vista web");
        let created_in = made.elapsed().as_secs_f32() * 1000.;
        println!("vista web creada en {created_in:.0} ms");
        // Pegar: el script lo pidió; acá se lee el portapapeles y se inserta.
        let poll = pending.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_millis(30)).await;
            if poll.borrow_mut().take().is_none() {
                continue;
            }
            let ok = this.update(cx, |this, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let json = serde_json::to_string(&text).unwrap_or_default();
                    let _ = this.web.evaluate_script(&format!("document.execCommand('insertText', false, {json})"));
                }
            });
            if ok.is_err() {
                break;
            }
        })
        .detach();
        Self { pending, web: Rc::new(web), loads, menu: false, hidden: false, wide: true, created_in }
    }

    fn go(&mut self, url: &str) {
        self.loads.borrow_mut().started = Some(Instant::now());
        let _ = self.web.load_url(url);
    }
}

fn button(id: &'static str, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px(px(10.))
        .py(px(5.))
        .rounded(px(8.))
        .bg(rgb(0x2a2d34))
        .text_color(rgb(0xe6e6e6))
        .text_size(px(13.))
        .cursor_pointer()
        .hover(|el| el.bg(rgb(0x3a3e47)))
        .child(label.into())
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let web = self.web.clone();
        let hidden = self.hidden;
        let log = self.loads.borrow().log.iter().rev().take(3).cloned().collect::<Vec<_>>().join("  ·  ");
        let mut bar = div().flex().items_center().gap(px(8.)).p(px(10.));
        for (i, (name, url)) in PAGES.iter().enumerate() {
            let url = url.to_string();
            bar = bar.child(button(["p0", "p1", "p2"][i], *name).on_click(cx.listener(move |this, _, _, cx| {
                this.go(&url);
                cx.notify();
            })));
        }
        bar = bar
            .child(button("menu", if self.menu { "Cerrar menú" } else { "Menú encima" }).on_click(cx.listener(|this, _, _, cx| {
                this.menu = !this.menu;
                cx.notify();
            })))
            .child(button("hide", if self.hidden { "Mostrar" } else { "Ocultar" }).on_click(cx.listener(|this, _, _, cx| {
                this.hidden = !this.hidden;
                let _ = this.web.set_visible(!this.hidden);
                cx.notify();
            })))
            .child(button("size", if self.wide { "Angostar" } else { "Ensanchar" }).on_click(cx.listener(|this, _, _, cx| {
                this.wide = !this.wide;
                cx.notify();
            })));
        let panel = div()
            .flex_1()
            .m(px(10.))
            .when(!self.wide, |el| el.mr(px(360.)))
            .rounded(px(16.))
            .border_1()
            .border_color(rgb(0x3a3e47))
            .overflow_hidden()
            .bg(rgb(0x1b1d22))
            .child(
                canvas(
                    move |bounds, _, _| {
                        // La vista web va donde GPUI dejó el panel, en puntos lógicos.
                        if !hidden {
                            let _ = web.set_bounds(Rect {
                                position: LogicalPosition::new(f32::from(bounds.origin.x) as f64, f32::from(bounds.origin.y) as f64).into(),
                                size: LogicalSize::new(f32::from(bounds.size.width) as f64, f32::from(bounds.size.height) as f64).into(),
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .size_full(),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x111316))
            .text_color(rgb(0xb0b4bc))
            .child(bar)
            .child(div().px(px(12.)).text_size(px(12.)).child(format!("creada en {:.0} ms  ·  {log}", self.created_in)))
            .child(panel)
            .when(self.menu, |el| {
                el.child(deferred(
                    anchored().position(point(px(220.), px(90.))).child(
                        div()
                            .w(px(260.))
                            .p(px(12.))
                            .rounded(px(14.))
                            .bg(rgb(0x2a2d34))
                            .border_1()
                            .border_color(rgb(0x4a4f5a))
                            .text_color(rgb(0xffffff))
                            .child("Menú de GPUI: ¿se ve encima de la página?"),
                    ),
                ))
            })
    }
}

fn main() {
    let url = std::env::args().nth(1).unwrap_or_else(|| PAGES[0].1.to_string());
    Application::new().run(move |cx: &mut App| {
        cx.set_menus(vec![
            Menu { name: "Prueba".into(), items: vec![] },
            Menu {
                name: "Edición".into(),
                items: vec![
                    MenuItem::os_action("Copiar", Copy, OsAction::Copy),
                    MenuItem::os_action("Pegar", Paste, OsAction::Paste),
                    MenuItem::os_action("Seleccionar todo", SelectAll, OsAction::SelectAll),
                ],
            },
        ]);
        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            |window, cx| cx.new(|cx| Probe::new(window, cx, url)),
        )
        .expect("ventana");
        cx.activate(true);
    });
}
