//! El visor de archivos de Atic Code: un archivo abierto en un panel, al lado
//! de los agentes, sin salir a otro programa.
//!
//! Muestra el archivo con sus números de línea o, si cambió, lo que cambió
//! (`git diff` contra el último commit, o todo como nuevo si git no lo
//! conoce). Es de solo lectura: lo editan los agentes. Las filas se dibujan
//! con `uniform_list`, así un archivo largo no cuesta más que lo que se ve.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Un archivo más grande que esto no se abre entero.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Hunk,
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// El número de la línea en el archivo de ahora (nada si se borró).
    pub number: Option<usize>,
    pub text: String,
}

pub struct Doc {
    pub id: u64,
    pub path: PathBuf,
    pub lines: Arc<Vec<String>>,
    pub diff: Option<Arc<Vec<DiffLine>>>,
    /// Se están mirando los cambios y no el archivo.
    pub show_diff: bool,
    /// Por qué no se ve el contenido (binario, muy grande, no se pudo leer).
    pub note: Option<String>,
}

fn hidden(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Las tabulaciones a espacios: la letra es de ancho fijo y la tabulación no.
fn clean(line: &str) -> String {
    line.trim_end_matches('\r').replace('\t', "    ")
}

impl Doc {
    pub fn load(id: u64, path: &Path, show_diff: bool) -> Self {
        let mut doc = Self { id, path: path.to_path_buf(), lines: Arc::default(), diff: None, show_diff: false, note: None };
        doc.reload();
        doc.show_diff = show_diff && doc.diff.is_some();
        doc
    }

    pub fn reload(&mut self) {
        self.note = None;
        match std::fs::metadata(&self.path) {
            Err(_) => self.note = Some("El archivo ya no existe.".into()),
            Ok(meta) if meta.len() > MAX_BYTES => self.note = Some("El archivo es muy grande para mostrarlo.".into()),
            Ok(_) => match std::fs::read(&self.path) {
                Err(error) => self.note = Some(format!("No se pudo leer: {error}")),
                Ok(bytes) if bytes.iter().take(8192).any(|b| *b == 0) => {
                    self.note = Some("Es un archivo binario.".into())
                }
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.lines = Arc::new(text.lines().map(clean).collect());
                }
            },
        }
        self.diff = diff(&self.path, &self.lines).map(Arc::new);
        if self.diff.is_none() {
            self.show_diff = false;
        }
    }
}

/// Lo que cambió en el archivo respecto del último commit. Nada si no está en
/// un repositorio o no cambió.
fn diff(path: &Path, lines: &[String]) -> Option<Vec<DiffLine>> {
    let dir = path.parent()?;
    let name = path.file_name()?;
    let output = hidden("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", "--no-color", "--no-ext-diff", "HEAD", "--"])
        .arg(name)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if !text.trim().is_empty() {
        return Some(parse_diff(&text));
    }
    // Sin diferencias: o no cambió, o git no lo conoce (archivo nuevo).
    let known = hidden("git").arg("-C").arg(dir).args(["ls-files", "--error-unmatch", "--"]).arg(name).output().ok()?;
    if known.status.success() {
        return None;
    }
    Some(
        lines
            .iter()
            .enumerate()
            .map(|(index, line)| DiffLine { kind: LineKind::Added, number: Some(index + 1), text: line.clone() })
            .collect(),
    )
}

/// Un diff unificado a filas: los encabezados de cada tramo, el contexto, lo
/// agregado y lo quitado, con el número de línea del archivo de ahora.
pub fn parse_diff(text: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let mut number = 0usize;
    let mut in_hunk = false;
    for raw in text.lines() {
        if let Some(rest) = raw.strip_prefix("@@") {
            in_hunk = true;
            // `@@ -a,b +c,d @@ contexto`: la línea nueva empieza en c.
            number = rest
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|part| part.split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            out.push(DiffLine { kind: LineKind::Hunk, number: None, text: clean(raw) });
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (kind, body) = match raw.chars().next() {
            Some('+') => (LineKind::Added, &raw[1..]),
            Some('-') => (LineKind::Removed, &raw[1..]),
            Some(' ') => (LineKind::Context, &raw[1..]),
            Some('\\') => continue,
            _ => (LineKind::Context, raw),
        };
        let shown = match kind {
            LineKind::Removed => None,
            _ => {
                number += 1;
                Some(number - 1)
            }
        };
        out.push(DiffLine { kind, number: shown, text: clean(body) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_diff_lleva_los_numeros_del_archivo_nuevo() {
        let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -3,3 +3,4 @@ fn main\n uno\n-dos\n+DOS\n+tres\n cuatro\n\\ No newline at end of file\n";
        let lines = parse_diff(text);
        let summary: Vec<(LineKind, Option<usize>, &str)> =
            lines.iter().map(|l| (l.kind, l.number, l.text.as_str())).collect();
        assert_eq!(
            summary,
            vec![
                (LineKind::Hunk, None, "@@ -3,3 +3,4 @@ fn main"),
                (LineKind::Context, Some(3), "uno"),
                (LineKind::Removed, None, "dos"),
                (LineKind::Added, Some(4), "DOS"),
                (LineKind::Added, Some(5), "tres"),
                (LineKind::Context, Some(6), "cuatro"),
            ]
        );
    }

    #[test]
    fn las_tabulaciones_pasan_a_espacios() {
        assert_eq!(clean("\tlet x = 1;\r"), "    let x = 1;");
    }
}
