//! El editor de prompts de Ajustes: el del dictado (Dictado) y los del
//! resumen (Reuniones). Muestra el prompt elegido para editarlo; el de Atic se
//! arma de nuevo cada vez que se abre, así que para quedarse con una versión
//! hay que guardarla como propia. También se escribe uno desde cero.
//!
//! En el dictado, el elegido es el que se usa (`prompts::set_active_dictation`).
//! En el resumen se elige al resumir, en el menú de Reuniones.

use atic_summarize::SummaryTemplate;
use gpui::{div, prelude::*, px, ClickEvent, Context, Entity, EventEmitter, SharedString, Window};

use crate::i18n::{t, tf};
use crate::meetings::settings::{button, dropdown, field, menu_item, menu_list, row};
use crate::prompts::{self, Kind};
use crate::text_area::TextArea;
use crate::text_input::TextInput;
use crate::theme::{hsla, FAINT, MUTED, SUNKEN, TEXT};

/// Lo que se está mostrando.
#[derive(Clone, PartialEq)]
enum Shown {
    /// Uno de Atic: el del dictado, o una plantilla del resumen.
    Atic(usize),
    Saved(String),
    /// Uno nuevo, escrito desde cero.
    New,
}

/// Lo que pide el editor abierto en Reuniones.
pub enum PromptEvent {
    /// Resumir con lo que se ve: una plantilla de Atic tal cual, uno propio
    /// guardado, o el texto editado sin guardar (nombre y texto).
    Use { template: Option<SummaryTemplate>, saved: Option<String>, edited: Option<(String, String)> },
    Close,
}

impl EventEmitter<PromptEvent> for PromptEditor {}

pub struct PromptEditor {
    kind: Kind,
    /// En la ventana de Reuniones: con «Resumir con este» y «Cerrar».
    in_meetings: bool,
    english: bool,
    shown: Shown,
    text: Entity<TextArea>,
    name: Entity<TextInput>,
    menu: bool,
    /// Lo último que pasó («Guardado»), bajo los botones.
    notice: Option<SharedString>,
}

/// Los de Atic de una clase: (nombre, texto).
fn atic(kind: Kind, english: bool) -> Vec<(String, String)> {
    match kind {
        Kind::Dictation => vec![(t("pill.prompts.aticDefault").to_string(), prompts::default_dictation(english))],
        Kind::Summary => SummaryTemplate::all()
            .iter()
            .map(|&template| {
                (
                    tf("pill.prompts.aticNamed", &[("name", &crate::meetings::pipeline::template_label(template))]),
                    atic_summarize::template_instructions(template, english).to_string(),
                )
            })
            .collect(),
    }
}

impl PromptEditor {
    pub fn new(kind: Kind, english: bool, cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| TextArea::new(t("pill.prompts.placeholder"), hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        let name = cx.new(|cx| TextInput::new(t("pill.prompts.namePlaceholder"), hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        // El dictado abre en el que se usa; el resumen, en la primera plantilla.
        let shown = match kind {
            Kind::Dictation => prompts::active_dictation().map_or(Shown::Atic(0), |p| Shown::Saved(p.id)),
            Kind::Summary => Shown::Atic(0),
        };
        let mut editor = Self { kind, in_meetings: false, english, shown: Shown::New, text, name, menu: false, notice: None };
        editor.show(shown, cx);
        editor
    }

    /// El de la ventana de Reuniones: abre en la plantilla o el propio que
    /// está elegido para resumir.
    pub fn for_meetings(english: bool, template: SummaryTemplate, saved: Option<String>, cx: &mut Context<Self>) -> Self {
        let mut editor = Self::new(Kind::Summary, english, cx);
        editor.in_meetings = true;
        let index = SummaryTemplate::all().iter().position(|t| *t == template).unwrap_or(0);
        let shown = saved.filter(|id| prompts::get(id).is_some()).map_or(Shown::Atic(index), Shown::Saved);
        editor.show(shown, cx);
        editor
    }

    /// Resumir con lo que se ve (Reuniones).
    fn use_it(&mut self, cx: &mut Context<Self>) {
        let text = self.text.read(cx).text().to_string();
        let (template, saved, source) = match &self.shown {
            Shown::Atic(index) => {
                let template = SummaryTemplate::all().get(*index).copied();
                let source = atic(self.kind, self.english).get(*index).map(|(_, text)| text.clone()).unwrap_or_default();
                (template, None, Some(source))
            }
            Shown::Saved(id) => (None, Some(id.clone()), prompts::get(id).map(|p| p.text)),
            Shown::New => (None, None, None),
        };
        if text.trim().is_empty() {
            self.notice = Some(t("pill.prompts.emptyText").into());
            cx.notify();
            return;
        }
        // Editado sin guardar: se usa igual, con su nombre o uno genérico.
        let edited = (source.as_deref() != Some(text.as_str())).then(|| {
            let name = self.name.read(cx).text().trim().to_string();
            (if name.is_empty() { t("pill.prompts.edited").to_string() } else { name }, text)
        });
        cx.emit(PromptEvent::Use { template, saved: if edited.is_some() { None } else { saved }, edited });
    }

    /// Muestra otro: su texto en el editor y su nombre en el campo. En el
    /// dictado, además, pasa a ser el que se usa.
    fn show(&mut self, shown: Shown, cx: &mut Context<Self>) {
        let (name, text) = match &shown {
            Shown::Atic(index) => atic(self.kind, self.english).get(*index).cloned().unwrap_or_default(),
            Shown::Saved(id) => prompts::get(id).map(|p| (p.name, p.text)).unwrap_or_default(),
            Shown::New => (String::new(), String::new()),
        };
        let name = if matches!(shown, Shown::Atic(_)) { String::new() } else { name };
        self.text.update(cx, |area, cx| area.set_text(&text, cx));
        self.name.update(cx, |input, cx| input.set_text(name, cx));
        if self.kind == Kind::Dictation {
            match &shown {
                Shown::Saved(id) => prompts::set_active_dictation(Some(id)),
                Shown::Atic(_) => prompts::set_active_dictation(None),
                Shown::New => {}
            }
        }
        self.shown = shown;
        self.menu = false;
        self.notice = None;
        cx.notify();
    }

    fn label(&self) -> String {
        match &self.shown {
            Shown::Atic(index) => atic(self.kind, self.english).get(*index).map(|(name, _)| name.clone()).unwrap_or_default(),
            Shown::Saved(id) => prompts::get(id).map(|p| p.name).unwrap_or_default(),
            Shown::New => t("pill.prompts.new").to_string(),
        }
    }

    /// Guarda lo escrito como uno propio nuevo y lo deja elegido.
    fn save_new(&mut self, cx: &mut Context<Self>) {
        let text = self.text.read(cx).text().to_string();
        if text.trim().is_empty() {
            self.notice = Some(t("pill.prompts.emptyText").into());
            cx.notify();
            return;
        }
        let name = self.name.read(cx).text().to_string();
        let saved = prompts::add(self.kind, &name, &text);
        self.show(Shown::Saved(saved.id), cx);
        self.notice = Some(t("pill.prompts.saved").into());
    }

    fn save_changes(&mut self, cx: &mut Context<Self>) {
        let Shown::Saved(id) = self.shown.clone() else {
            return;
        };
        let text = self.text.read(cx).text().to_string();
        let name = self.name.read(cx).text().to_string();
        prompts::update(&id, &name, &text);
        self.notice = Some(t("pill.prompts.saved").into());
        cx.notify();
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        if let Shown::Saved(id) = self.shown.clone() {
            prompts::remove(&id);
        }
        self.show(Shown::Atic(0), cx);
    }

    fn hint(&self) -> &'static str {
        match self.kind {
            Kind::Dictation => t("pill.prompts.dictationHint"),
            Kind::Summary => t("pill.prompts.summaryHint"),
        }
    }
}

impl Render for PromptEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id_base = match self.kind {
            Kind::Dictation => "prompt-dict",
            Kind::Summary => "prompt-sum",
        };
        let mut col = div().flex().flex_col().gap(px(10.)).child(row(
            t("pill.prompts.title"),
            self.hint(),
            dropdown(
                if self.kind == Kind::Dictation { "prompt-dict-menu" } else { "prompt-sum-menu" },
                self.label(),
                self.menu,
                cx.listener(|editor, _: &ClickEvent, _, cx| {
                    editor.menu = !editor.menu;
                    cx.notify();
                }),
            ),
        ));
        if self.menu {
            let mut options: Vec<(Shown, String)> = atic(self.kind, self.english)
                .into_iter()
                .enumerate()
                .map(|(index, (name, _))| (Shown::Atic(index), name))
                .collect();
            options.extend(prompts::saved(self.kind).into_iter().map(|p| (Shown::Saved(p.id), p.name)));
            options.push((Shown::New, t("pill.prompts.new").to_string()));
            let mut menu = menu_list();
            for (ix, (shown, name)) in options.into_iter().enumerate() {
                let on = shown == self.shown;
                menu = menu.child(menu_item(
                    (if self.kind == Kind::Dictation { "prompt-dict-opt" } else { "prompt-sum-opt" }, ix),
                    name,
                    on,
                    cx.listener(move |editor, _: &ClickEvent, _, cx| editor.show(shown.clone(), cx)),
                ));
            }
            col = col.child(menu);
        }
        // El texto, en una caja como los campos.
        col = col.child(
            div()
                .h(px(150.))
                .p(px(10.))
                .rounded(px(crate::theme::styled().map_or(10., |t| t.r_ctl.min(12.))))
                .bg(crate::theme::styled().map_or_else(|| hsla(SUNKEN), |t| t.control))
                .text_size(px(12.5))
                .line_height(px(18.))
                .child(self.text.clone()),
        );
        let saved = matches!(self.shown, Shown::Saved(_));
        let new = self.shown == Shown::New;
        // El nombre: para uno propio, o para guardar el de Atic como propio.
        col = col.child(row(t("pill.prompts.name"), "", field(&self.name)));
        let mut buttons = div().flex().flex_wrap().items_center().gap(px(8.));
        if saved {
            buttons = buttons
                .child(button(format!("{id_base}-save").into(), t("pill.prompts.saveChanges"), cx.listener(|e, _: &ClickEvent, _, cx| e.save_changes(cx))))
                .child(button(format!("{id_base}-copy").into(), t("pill.prompts.saveAsNew"), cx.listener(|e, _: &ClickEvent, _, cx| e.save_new(cx))))
                .child(button(format!("{id_base}-delete").into(), t("pill.prompts.delete"), cx.listener(|e, _: &ClickEvent, _, cx| e.delete(cx))));
        } else {
            buttons = buttons.child(button(
                format!("{id_base}-mine").into(),
                if new { t("pill.prompts.save") } else { t("pill.prompts.saveAsMine") },
                cx.listener(|e, _: &ClickEvent, _, cx| e.save_new(cx)),
            ));
            if !new {
                buttons = buttons.child(button(
                    format!("{id_base}-blank").into(),
                    t("pill.prompts.fromScratch"),
                    cx.listener(|e, _: &ClickEvent, _, cx| e.show(Shown::New, cx)),
                ));
            }
        }
        if self.in_meetings {
            buttons = buttons
                .child(div().flex_1())
                .child(button(format!("{id_base}-close").into(), t("pill.prompts.close"), cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(PromptEvent::Close))))
                .child(button(format!("{id_base}-use").into(), t("pill.prompts.useIt"), cx.listener(|e, _: &ClickEvent, _, cx| e.use_it(cx))));
        }
        col = col.child(buttons);
        if let Some(notice) = self.notice.clone() {
            col = col.child(div().text_size(px(11.5)).text_color(hsla(MUTED)).child(notice));
        }
        col
    }
}
