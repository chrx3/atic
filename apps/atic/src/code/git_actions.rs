//! Las acciones git del panel de Cambios: preparar, sacar de preparados,
//! descartar, commit, push y pull. Todo `git` corre en segundo plano y el panel
//! se refresca al terminar; si algo falla, se avisa con un toast (nunca en
//! silencio). Las rutas van como argumentos tras `--`, nunca por shell, y los
//! errores se limpian de credenciales antes de mostrarse.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Entity};

use super::git::{self, Section};
use super::CodeView;
use crate::i18n::tf;

/// Descartar los cambios de estos archivos (el diálogo de confirmación).
pub(super) struct GitConfirm {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    /// «3 archivos» o el nombre, para el diálogo.
    pub display: String,
    /// Sin seguimiento: se borran del disco (`git clean -f`).
    pub untracked: bool,
}

/// Lo que `CodeView` guarda de estas acciones (ver `mod.rs`).
pub(super) struct GitState {
    /// Repos con un push o pull en curso (su spinner).
    pub busy: HashSet<PathBuf>,
    /// El mensaje de commit de cada repo, por raíz.
    pub messages: RefCell<HashMap<PathBuf, Entity<gpui_m3::TextField>>>,
    pub confirm: Option<GitConfirm>,
}

impl Default for GitState {
    fn default() -> Self {
        Self { busy: HashSet::new(), messages: RefCell::new(HashMap::new()), confirm: None }
    }
}

/// Sin `://usuario:clave@` en lo que se muestra: git puede repetir el remoto
/// en sus errores y las credenciales no se imprimen ni se mandan.
fn scrub(text: &str) -> String {
    let mut out = text.to_string();
    let mut from = 0;
    while let Some(scheme) = out[from..].find("://") {
        let start = from + scheme + 3;
        let rest = &out[start..];
        let end = rest.find(|c: char| c == '/' || c.is_whitespace()).map(|i| start + i).unwrap_or(out.len());
        if let Some(at) = out[start..end].find('@') {
            out.replace_range(start..start + at + 1, "…@");
            from = start + 3;
        } else {
            from = end;
        }
    }
    out
}

/// Los argumentos de `git -C <root>`: las rutas tras `--`, como argumentos.
fn spawn_git(cx: &mut Context<CodeView>, root: &Path, args: Vec<String>, done: impl FnOnce(&mut CodeView, Result<String, String>, &mut Context<CodeView>) + 'static) {
    let root = root.to_path_buf();
    let scan = cx.background_executor().spawn(async move {
        let mut command = git::git(&root);
        command.args(&args);
        match command.output() {
            Ok(output) if output.status.success() => Ok(String::from_utf8_lossy(&output.stdout).trim().to_string()),
            Ok(output) => {
                let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(if detail.is_empty() { output.status.to_string() } else { detail })
            }
            Err(error) => Err(error.to_string()),
        }
    });
    cx.spawn(async move |this, cx| {
        let result = scan.await;
        let _ = this.update(cx, |view, cx| done(view, result, cx));
    })
    .detach();
}

/// El aviso de un fallo, con la causa limpia de credenciales.
fn note_failed(view: &mut CodeView, error: String, cx: &mut Context<CodeView>) {
    view.show_toast(tf("pill.code.view.gitError", &[("error", &scrub(&error))]), cx);
}

impl CodeView {
    /// El campo del mensaje de commit de este repo (uno por raíz).
    pub(super) fn commit_field(&self, root: &Path, cx: &mut Context<Self>) -> Entity<gpui_m3::TextField> {
        if let Some(field) = self.git.messages.borrow().get(root).cloned() {
            return field;
        }
        let field = cx.new(|cx| {
            gpui_m3::TextField::new(cx)
                .height(gpui::px(34.))
                .placeholder(crate::i18n::t("pill.code.view.commitMessage"))
        });
        let key = root.to_path_buf();
        cx.subscribe(&field, move |view: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
            gpui_m3::TextFieldEvent::Submitted(_) => view.do_commit(key.clone(), cx),
            gpui_m3::TextFieldEvent::Changed(_) => cx.notify(),
            gpui_m3::TextFieldEvent::Cancelled => {}
        })
        .detach();
        self.git.messages.borrow_mut().insert(root.to_path_buf(), field.clone());
        field
    }

    /// Hay algo preparado y un mensaje escrito (y nada en curso): se puede confirmar.
    pub(super) fn can_commit(&self, root: &Path, cx: &gpui::App) -> bool {
        let staged =
            self.repos.iter().find(|r| r.root.as_path() == root).is_some_and(|r| !r.in_section(Section::Staged).is_empty());
        let text = self.git.messages.borrow().get(root).map(|f| f.read(cx).text().trim().is_empty()).unwrap_or(true);
        staged && !text && !self.git.busy.contains(root)
    }

    /// Las rutas relativas al repo (git ya corre con `-C` en la raíz).
    fn git_paths(root: &Path, paths: &[PathBuf]) -> Vec<String> {
        paths.iter().map(|path| path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")).collect()
    }

    /// `git add -- <rutas>` y el panel al día (o el error, con un toast).
    pub(super) fn stage_paths(&mut self, root: PathBuf, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let mut args = vec!["add".to_string(), "--".to_string()];
        args.extend(Self::git_paths(&root, &paths));
        self.refresh_changes_now(cx);
        spawn_git(cx, &root, args, |view, result, cx| {
            if let Err(error) = result {
                note_failed(view, error, cx);
            }
            view.refresh_changes(cx);
            cx.notify();
        });
    }

    /// `git restore --staged -- <rutas>` y el panel al día.
    pub(super) fn unstage_paths(&mut self, root: PathBuf, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let mut args = vec!["restore".to_string(), "--staged".to_string(), "--".to_string()];
        args.extend(Self::git_paths(&root, &paths));
        self.refresh_changes_now(cx);
        spawn_git(cx, &root, args, |view, result, cx| {
            if let Err(error) = result {
                note_failed(view, error, cx);
            }
            view.refresh_changes(cx);
            cx.notify();
        });
    }

    /// Preparar todo lo de una sección (sin preparar o sin seguimiento).
    pub(super) fn stage_section(&mut self, root: PathBuf, section: Section, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .repos
            .iter()
            .find(|r| r.root == root)
            .map(|r| r.in_section(section).iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default();
        self.stage_paths(root, paths, cx);
    }

    /// Sacar todo lo preparado de un repo.
    pub(super) fn unstage_all(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .repos
            .iter()
            .find(|r| r.root == root)
            .map(|r| r.in_section(Section::Staged).iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default();
        self.unstage_paths(root, paths, cx);
    }

    /// Pedir confirmación antes de descartar (descartar siempre pregunta).
    pub(super) fn request_discard(&mut self, root: PathBuf, paths: Vec<PathBuf>, untracked: bool, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let display = if paths.len() == 1 {
            paths[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| paths[0].display().to_string())
        } else {
            tf("pill.code.view.filesChanged", &[("total", &paths.len())])
        };
        self.git.confirm = Some(GitConfirm { root, paths, display, untracked });
        cx.notify();
    }

    /// Lo respondido en el diálogo de descartar.
    pub(super) fn answer_discard(&mut self, discard: bool, cx: &mut Context<Self>) {
        let Some(confirm) = self.git.confirm.take() else {
            return;
        };
        if !discard {
            cx.notify();
            return;
        }
        // Lo seguido: del disco; lo demás: del árbol (lo preparado queda).
        let mut args = if confirm.untracked {
            vec!["clean".to_string(), "-f".to_string(), "--".to_string()]
        } else {
            vec!["restore".to_string(), "--".to_string()]
        };
        args.extend(Self::git_paths(&confirm.root, &confirm.paths));
        self.refresh_changes_now(cx);
        spawn_git(cx, &confirm.root, args, |view, result, cx| {
            if let Err(error) = result {
                note_failed(view, error, cx);
            }
            view.refresh_changes(cx);
            cx.notify();
        });
    }

    /// `git commit -m <mensaje>` con lo preparado; sin mensaje o sin nada
    /// preparado, no hace nada (el botón ya va deshabilitado).
    pub(super) fn do_commit(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let message = self.git.messages.borrow().get(&root).map(|f| f.read(cx).text().trim().to_string()).unwrap_or_default();
        let staged: Vec<PathBuf> = self
            .repos
            .iter()
            .find(|r| r.root == root)
            .map(|r| r.in_section(Section::Staged).iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default();
        if message.is_empty() || staged.is_empty() || self.git.busy.contains(&root) {
            return;
        }
        let args = vec!["commit".to_string(), "-m".to_string(), message];
        self.git.busy.insert(root.clone());
        self.refresh_changes_now(cx);
        let key = root.clone();
        spawn_git(cx, &root, args, move |view, result, cx| {
            view.git.busy.remove(&key);
            match result {
                Ok(_) => {
                    if let Some(field) = view.git.messages.borrow().get(&key).cloned() {
                        field.update(cx, |field, cx| field.set_text("", cx));
                    }
                }
                Err(error) => note_failed(view, error, cx),
            }
            view.refresh_changes(cx);
            cx.notify();
        });
    }

    /// `git push`, con spinner y sin tragar el error.
    pub(super) fn do_push(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.git.busy.insert(root.clone()) {
            return;
        }
        cx.notify();
        let key = root.clone();
        spawn_git(cx, &root, vec!["push".to_string()], move |view, result, cx| {
            view.git.busy.remove(&key);
            if let Err(error) = result {
                note_failed(view, error, cx);
            }
            view.refresh_changes(cx);
            cx.notify();
        });
    }

    /// `git pull --ff-only`, con spinner y sin tragar el error.
    pub(super) fn do_pull(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.git.busy.insert(root.clone()) {
            return;
        }
        cx.notify();
        let key = root.clone();
        spawn_git(cx, &root, vec!["pull".to_string(), "--ff-only".to_string()], move |view, result, cx| {
            view.git.busy.remove(&key);
            if let Err(error) = result {
                note_failed(view, error, cx);
            }
            view.refresh_changes(cx);
            cx.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_errores_no_muestran_credenciales() {
        assert_eq!(scrub("fatal: no se pudo leer 'https://usuario:clave@git.com/r.git'"), "fatal: no se pudo leer 'https://…@git.com/r.git'");
        // Sin credenciales, queda igual.
        assert_eq!(scrub("error: pathspec 'x' no coincide"), "error: pathspec 'x' no coincide");
        // Un correo fuera de una URL no se toca.
        assert_eq!(scrub("escribe a ana@ejemplo.com"), "escribe a ana@ejemplo.com");
    }
}
