//! Lo común de las terminales de Atic Code: el shell de cada sistema, la
//! grilla de una consola (`console`, alacritty) y cómo se pinta, la barra de
//! título de las ventanas (`chrome`), los paneles divididos, el visor de diffs,
//! el explorador de archivos y la lista de proyectos (`workspaces`).

pub(crate) mod explorer;
pub(crate) mod chrome;
pub(crate) mod console;
pub(crate) mod input;
pub(crate) mod panes;
pub(crate) mod viewer;
pub(crate) mod workspaces;

use std::path::PathBuf;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui::{point, px, size, App, Bounds, Font, FontStyle, FontWeight, Pixels, TextRun, Window};

use console::Console;

const PAD: f32 = 8.0;
pub(crate) const GRID_PAD: f32 = PAD;
const TEXT_ZOOM: f32 = 0.65;
const TEXT: u32 = crate::theme::TEXT;

/// Un rectángulo en unidades del plano.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Area {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
}

/// Lo que mide la cámara del cuadro, en píxeles de pantalla.
#[derive(Clone, Copy)]
pub(crate) struct Cell {
    pub(crate) w: f32,
    pub(crate) h: f32,
}

/// El shell de una terminal nueva: PowerShell en Windows; en macOS el del
/// usuario (`$SHELL`, normalmente zsh) como shell de inicio de sesión, con su
/// PATH de Homebrew, nvm y compañía.
pub(crate) fn shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        return (powershell(), vec!["-NoLogo".into()]);
    }
    (user_shell(), vec!["-l".into()])
}

/// Un CLI de agente que, al terminar, deja el prompt (el `cmd /K` de
/// Windows). En macOS: el shell del usuario corre la línea y sigue como shell
/// interactivo.
pub(crate) fn agent_shell(line: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        return ("cmd.exe".into(), vec!["/K".into(), line.into()]);
    }
    let shell = user_shell();
    let then = format!("{line}; exec {} -l", shell_quote(&shell));
    (shell, vec!["-l".into(), "-i".into(), "-c".into(), then])
}

fn user_shell() -> String {
    std::env::var("SHELL").ok().filter(|s| !s.is_empty() && std::path::Path::new(s).exists()).unwrap_or_else(|| "/bin/zsh".into())
}

/// Entre comillas simples para el shell.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub(crate) fn powershell() -> String {
    let pwsh = std::env::var_os("ProgramFiles")
        .map(|dir| PathBuf::from(dir).join("PowerShell").join("7").join("pwsh.exe"));
    match pwsh.filter(|path| path.exists()) {
        Some(path) => path.to_string_lossy().into_owned(),
        None => "powershell.exe".into(),
    }
}

/// Las filas visibles de una consola con la paleta dada, para la terminal de
/// Atic Code.
pub(crate) fn grid_of(console: &Console, fonts: &[Font; 4], palette: &console::Palette) -> Grid {
    let term = console.term.lock();
    let content = term.renderable_content();
    let colors = content.colors;
    let rows = term.grid().screen_lines();
    let cols = term.grid().columns();
    let offset = content.display_offset as i32;
    let mut out: Vec<Row> = (0..rows).map(|_| Row::default()).collect();
    for indexed in content.display_iter {
        let row = (indexed.point.line.0 + offset) as usize;
        let col = indexed.point.column.0;
        let Some(slot) = out.get_mut(row) else {
            continue;
        };
        let cell = indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let mut fg = console::resolve(cell.fg, colors, palette);
        let mut bg = console::resolve(cell.bg, colors, palette);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.intersects(Flags::DIM) {
            fg = console::dim(fg);
        }
        if bg != palette.bg {
            let wide = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
            match slot.backgrounds.last_mut() {
                Some((start, len, color)) if *color == bg && *start + *len == col => {
                    *len += wide
                }
                _ => slot.backgrounds.push((col, wide, bg)),
            }
        }
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            slot.underlines.push((col, fg));
        }
        {
            while slot.cols < col {
                slot.text.push(' ');
                extend_run(slot, 1, palette.fg, 0, fonts);
                slot.cols += 1;
            }
            let c = if cell.flags.contains(Flags::HIDDEN) || cell.c == '\0' { ' ' } else { cell.c };
            let before = slot.text.len();
            slot.text.push(c);
            let style = cell.flags.contains(Flags::BOLD) as u8
                | (cell.flags.contains(Flags::ITALIC) as u8) << 1;
            extend_run(slot, slot.text.len() - before, fg, style, fonts);
            slot.cols += 1;
            if c != ' ' {
                slot.ink.push((col, fg));
            }
        }
    }
    let cursor = (content.cursor.shape != CursorShape::Hidden)
        .then(|| {
            let row = content.cursor.point.line.0 + offset;
            (row >= 0 && (row as usize) < rows)
                .then_some((row as usize, content.cursor.point.column.0.min(cols - 1)))
        })
        .flatten();
    drop(term);
    Grid {
        rows: out,
        cursor,
        cols,
    }
}

fn extend_run(slot: &mut Row, len: usize, fg: u32, style: u8, fonts: &[Font; 4]) {
    if let Some((last_fg, last_style)) = slot.run_key {
        if last_fg == fg && last_style == style {
            if let Some(run) = slot.runs.last_mut() {
                run.len += len;
                return;
            }
        }
    }
    slot.runs.push(TextRun {
        len,
        font: fonts[style as usize & 3].clone(),
        color: console::term(fg),
        background_color: None,
        underline: None,
        strikethrough: None,
    });
    slot.run_key = Some((fg, style));
}

#[derive(Default)]
pub(crate) struct Row {
    /// El renglón entero, una letra por columna, y sus tramos de estilo.
    text: String,
    runs: Vec<TextRun>,
    run_key: Option<(u32, u8)>,
    cols: usize,
    /// Las columnas con letra y su color: la silueta del renglón.
    ink: Vec<(usize, u32)>,
    /// (columna, celdas, color) de los fondos que no son el de la terminal.
    backgrounds: Vec<(usize, usize, u32)>,
    underlines: Vec<(usize, u32)>,
}

/// Lo mismo para cualquier fuente base (la de la terminal de Atic Code).
pub(crate) fn fonts_of(base: Font) -> [Font; 4] {
    let variant = |bold: bool, italic: bool| {
        let mut font = base.clone();
        if bold {
            font.weight = FontWeight::BOLD;
        }
        if italic {
            font.style = FontStyle::Italic;
        }
        font
    };
    [
        variant(false, false),
        variant(true, false),
        variant(false, true),
        variant(true, true),
    ]
}

pub(crate) struct Grid {
    pub(crate) rows: Vec<Row>,
    pub(crate) cursor: Option<(usize, usize)>,
    pub(crate) cols: usize,
}

/// Cómo se dibuja la grilla: el margen, el color del cursor y el del contorno
/// del cursor cuando la consola no tiene el foco.
#[derive(Clone, Copy)]
pub(crate) struct GridLook {
    pub(crate) pad: f32,
    pub(crate) cursor: u32,
    pub(crate) idle: u32,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_grid(
    grid: &Grid,
    content: Bounds<Pixels>,
    cell: Cell,
    z: f32,
    font_size: f32,
    focused: bool,
    look: GridLook,
    window: &mut Window,
    cx: &mut App,
) {
    let cw = cell.w * z;
    let lh = cell.h * z;
    let origin = (f32::from(content.origin.x) + look.pad * z, f32::from(content.origin.y) + 2. * z);
    let mask = window.content_mask().bounds;
    let baseline = lh * 0.78;
    let silhouette = z < TEXT_ZOOM;
    for (index, row) in grid.rows.iter().enumerate() {
        let y = origin.1 + index as f32 * lh;
        // Filas fuera de la vista (tarjeta cortada por el borde): ni se tocan.
        if y + lh < f32::from(mask.origin.y) || y > f32::from(mask.bottom()) {
            continue;
        }
        for &(col, len, color) in &row.backgrounds {
            window.paint_quad(gpui::fill(
                Bounds::new(point(px(origin.0 + col as f32 * cw), px(y)), size(px(len as f32 * cw), px(lh))),
                console::term(color),
            ));
        }
        if row.text.trim_end().is_empty() {
            continue;
        }
        if silhouette {
            // De lejos: cada tramo con letra es una barra de su color. Se ve
            // la salida moverse sin dibujar texto ilegible.
            let mut start: Option<(usize, usize, u32)> = None;
            for &(col, color) in row.ink.iter().chain(std::iter::once(&(usize::MAX, 0))) {
                match start {
                    Some((from, to, c)) if c == color && to + 1 >= col => start = Some((from, col, c)),
                    _ => {
                        if let Some((from, to, c)) = start {
                            window.paint_quad(gpui::fill(
                                Bounds::new(
                                    point(px(origin.0 + from as f32 * cw), px(y + lh * 0.3)),
                                    size(px((to - from + 1) as f32 * cw), px(lh * 0.45)),
                                ),
                                console::term(c).opacity(0.55),
                            ));
                        }
                        start = (col != usize::MAX).then_some((col, col, color));
                    }
                }
            }
            continue;
        }
        // El renglón compuesto de una vez: GPUI lo reusa en los cuadros
        // siguientes si no cambió. `force_width` pone cada letra en su
        // columna aunque venga de otra fuente.
        let line = window.text_system().shape_line(
            row.text.clone().into(),
            px(font_size),
            &row.runs,
            Some(px(cw)),
        );
        let _ = line.paint(point(px(origin.0), px(y)), px(lh), window, cx);
        for &(col, color) in &row.underlines {
            window.paint_quad(gpui::fill(
                Bounds::new(
                    point(px(origin.0 + col as f32 * cw), px(y + baseline + 2. * z)),
                    size(px(cw), px(z.max(1.0))),
                ),
                console::term(color),
            ));
        }
    }
    if let Some((row, col)) = grid.cursor {
        let bounds = Bounds::new(
            point(px(origin.0 + col.min(grid.cols) as f32 * cw), px(origin.1 + row as f32 * lh)),
            size(px(cw), px(lh)),
        );
        if focused {
            window.paint_quad(gpui::fill(bounds, console::term(look.cursor).opacity(0.75)));
        } else {
            window.paint_quad(gpui::outline(bounds, console::term(look.idle), gpui::BorderStyle::Solid));
        }
    }
}

