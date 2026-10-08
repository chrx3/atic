//! Las carpetas del espacio, como un workspace de VS Code con varias raíces.
//!
//! Atic Code guarda una lista de carpetas (`space-folders.json` en los datos
//! de la pill) y una de ellas es la activa: ahí se abre cada consola nueva.
//! Claude y Codex reciben además las otras con `--add-dir`, así un mismo
//! agente lee y edita en todas. PowerShell solo arranca en la activa.

use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, svg, ClickEvent, Context, FontWeight, PathPromptOptions, SharedString};
use serde::{Deserialize, Serialize};

use super::console::hsla;
use super::SpaceView;

const TEXT: u32 = 0xf0f0ea;
const MUTED: u32 = 0x9a9a90;
const CHIP: u32 = 0x232321;
const CHIP_HOVER: u32 = 0x2e2e2b;
const CHIP_ACTIVE: u32 = 0x3a3a36;
/// Una carpeta con nombre largo se corta: la barra es compartida.
const NAME_MAX: usize = 18;

#[derive(Default, Serialize, Deserialize)]
pub struct Folders {
    list: Vec<PathBuf>,
    active: usize,
}

fn file() -> Option<PathBuf> {
    crate::paths::file("space-folders.json")
}

impl Folders {
    pub fn load() -> Self {
        let mut folders: Self = file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        folders.list.retain(|path| path.is_dir());
        folders.active = folders.active.min(folders.list.len().saturating_sub(1));
        folders
    }

    fn save(&self) {
        let Some(path) = file() else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "espacio: no se guardaron las carpetas");
                }
            }
            Err(error) => tracing::warn!(%error, "espacio: carpetas sin serializar"),
        }
    }

    pub fn list(&self) -> &[PathBuf] {
        &self.list
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Donde se abren las consolas nuevas.
    pub fn active(&self) -> Option<&PathBuf> {
        self.list.get(self.active)
    }

    pub fn add(&mut self, paths: Vec<PathBuf>) {
        let mut last = None;
        for path in paths {
            match self.list.iter().position(|known| same(known, &path)) {
                Some(index) => last = Some(index),
                None => {
                    self.list.push(path);
                    last = Some(self.list.len() - 1);
                }
            }
        }
        if let Some(index) = last {
            self.active = index;
        }
        self.save();
    }

    pub fn remove(&mut self, index: usize) {
        if index >= self.list.len() {
            return;
        }
        self.list.remove(index);
        if self.active > index || self.active >= self.list.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.save();
    }

    pub fn select(&mut self, index: usize) {
        if index < self.list.len() {
            self.active = index;
            self.save();
        }
    }

    /// Las otras carpetas para un agente que arranca en `cwd`. Solo si `cwd`
    /// es una de la lista: un agente abierto en otra parte no las recibe.
    pub fn extras(&self, cwd: &Path) -> Vec<PathBuf> {
        if !self.list.iter().any(|path| same(path, cwd)) {
            return Vec::new();
        }
        self.list.iter().filter(|path| !same(path, cwd)).cloned().collect()
    }
}

pub fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
}

/// La línea de un agente con las otras carpetas. Claude y Codex entienden
/// `--add-dir`; los demás arrancan igual que antes.
pub fn with_add_dirs(agent: &str, line: &str, dirs: &[PathBuf]) -> String {
    if dirs.is_empty() || !matches!(agent, "claude" | "codex") {
        return line.to_string();
    }
    let mut out = line.to_string();
    for dir in dirs {
        out.push_str(&format!(" --add-dir \"{}\"", dir.display()));
    }
    out
}

pub fn name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    if name.chars().count() > NAME_MAX {
        format!("{}…", name.chars().take(NAME_MAX - 1).collect::<String>())
    } else {
        name
    }
}

impl SpaceView {
    pub(super) fn browse_folders(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Agregar al espacio".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                let _ = this.update(cx, |view, cx| {
                    view.folders.add(paths);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

/// Los chips de las carpetas y el «+» para agregar. Un clic elige dónde se
/// abren las consolas nuevas; la × la quita del espacio (no la borra).
pub fn bar(view: &SpaceView, cx: &mut Context<SpaceView>) -> impl IntoElement {
    let chips = view.folders.list.iter().enumerate().map(|(index, path)| {
        let active = index == view.folders.active;
        div()
            .id(("space-folder", index))
            .h(px(28.))
            .pl(px(10.))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(14.))
            .text_size(px(12.))
            .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
            .text_color(hsla(if active { TEXT } else { MUTED }))
            .bg(hsla(if active { CHIP_ACTIVE } else { CHIP }))
            .hover(|el| el.bg(hsla(CHIP_HOVER)))
            .cursor_pointer()
            .tooltip(crate::hover::tip_text(SharedString::from(path.display().to_string())))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.folders.select(index);
                cx.notify();
            }))
            .child(svg().path("icons/folder.svg").size(px(13.)).text_color(hsla(if active { TEXT } else { MUTED })))
            .child(name(path))
            .child(
                div()
                    .id(("space-folder-remove", index))
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.))
                    .hover(|el| el.bg(hsla(CHIP_ACTIVE)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        view.folders.remove(index);
                        cx.notify();
                    }))
                    .child(svg().path("icons/x.svg").size(px(11.)).text_color(hsla(MUTED))),
            )
    });
    let empty = view.folders.list.is_empty();
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w(px(0.))
        .overflow_hidden()
        .children(chips)
        .child(
            div()
                .id("space-folder-add")
                .h(px(28.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(14.))
                .text_size(px(12.))
                .text_color(hsla(MUTED))
                .bg(hsla(CHIP))
                .hover(|el| el.bg(hsla(CHIP_HOVER)))
                .cursor_pointer()
                .tooltip(crate::hover::tip("Agregar carpetas al espacio"))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse_folders(cx)))
                .child(svg().path("icons/folder-plus.svg").size(px(13.)).text_color(hsla(MUTED)))
                .when(empty, |el| el.child("Carpetas")),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders(list: &[&str]) -> Folders {
        Folders { list: list.iter().map(PathBuf::from).collect(), active: 0 }
    }

    #[test]
    fn un_agente_en_una_carpeta_del_espacio_recibe_las_otras() {
        let space = folders(&[r"C:\code\atic", r"C:\code\api", r"C:\code\web"]);
        let extras = space.extras(Path::new(r"c:\code\API\"));
        assert_eq!(extras, vec![PathBuf::from(r"C:\code\atic"), PathBuf::from(r"C:\code\web")]);
        assert!(space.extras(Path::new(r"C:\otra")).is_empty());
    }

    #[test]
    fn solo_claude_y_codex_llevan_add_dir() {
        let dirs = [PathBuf::from(r"C:\code\api")];
        assert_eq!(with_add_dirs("claude", "claude", &dirs), r#"claude --add-dir "C:\code\api""#);
        assert_eq!(with_add_dirs("codex", "codex", &dirs), r#"codex --add-dir "C:\code\api""#);
        assert_eq!(with_add_dirs("opencode", "opencode", &dirs), "opencode");
        assert_eq!(with_add_dirs("claude", "claude", &[]), "claude");
    }
}
