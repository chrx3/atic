//! Una consola: el PTY (ConPTY en Windows), el emulador de Alacritty y lo
//! que la UI necesita saber de ella.
//!
//! El hilo lector de `alacritty_terminal` lee el PTY y alimenta el `Term`
//! directamente; a la UI solo le llega un aviso de «hay contenido nuevo» que
//! se junta en un cuadro. Así una consola que escupe 30 líneas por segundo no
//! dispara 30 renders, y ninguna salida pasa por IPC ni por JavaScript.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use alacritty_terminal::Term;
use gpui::{Hsla, Keystroke};

/// Tamaño de la grilla: columnas y filas visibles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Lo que el hilo del PTY le cuenta a la UI, sin bloquear nada.
#[derive(Clone)]
pub struct Listener {
    shared: Arc<Shared>,
}

struct Shared {
    /// Hubo salida desde el último cuadro.
    dirty: AtomicBool,
    /// Milisegundos desde `epoch` de la última salida (para «trabajando»).
    last_output: AtomicU64,
    exited: AtomicBool,
    epoch: Instant,
    title: std::sync::Mutex<Option<String>>,
    /// A quién avisar de que hay salida nueva. Cambia si se cierra la ventana
    /// del espacio y se abre otra: la consola sigue viva (ver `rewire`).
    wake: std::sync::Mutex<futures::channel::mpsc::UnboundedSender<()>>,
}

impl Shared {
    fn notify(&self) {
        if let Ok(wake) = self.wake.lock() {
            let _ = wake.unbounded_send(());
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                let ms = self.shared.epoch.elapsed().as_millis() as u64;
                self.shared.last_output.store(ms, Ordering::Relaxed);
                // Solo el primer aviso despierta a la UI; los demás esperan a
                // que lea.
                if !self.shared.dirty.swap(true, Ordering::AcqRel) {
                    self.shared.notify();
                }
            }
            Event::Title(title) => {
                if let Ok(mut slot) = self.shared.title.lock() {
                    *slot = Some(title);
                }
            }
            Event::ResetTitle => {
                if let Ok(mut slot) = self.shared.title.lock() {
                    *slot = None;
                }
            }
            Event::ChildExit(_) | Event::Exit => {
                self.shared.exited.store(true, Ordering::Relaxed);
                self.shared.notify();
            }
            _ => {}
        }
    }
}

pub struct Console {
    pub term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    shared: Arc<Shared>,
    pub size: GridSize,
}

/// Qué ejecutar: el programa y sus argumentos, y dónde.
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Variables de entorno extra (la marca de las consolas de agentes).
    pub env: Vec<(String, String)>,
}

/// Escribe en la consola desde otro lado (contestar un permiso desde la
/// bandeja). Sigue sirviendo mientras la consola viva.
#[derive(Clone)]
pub struct Writer(EventLoopSender);

impl Writer {
    pub fn write(&self, text: &str) -> Result<(), String> {
        self.0
            .send(Msg::Input(Cow::Owned(text.as_bytes().to_vec())))
            .map_err(|error| crate::i18n::tf("pill.kit.consoleClosed", &[("error", &format!("{error:?}"))]))
    }
}

impl Console {
    /// Arranca el proceso en un PTY del tamaño `size`. `cell` es el tamaño de
    /// una celda en píxeles (algunas TUIs lo piden).
    pub fn spawn(
        launch: Launch,
        size: GridSize,
        cell: (u16, u16),
        wake: futures::channel::mpsc::UnboundedSender<()>,
    ) -> anyhow::Result<Self> {
        let shared = Arc::new(Shared {
            dirty: AtomicBool::new(true),
            last_output: AtomicU64::new(0),
            exited: AtomicBool::new(false),
            epoch: Instant::now(),
            title: std::sync::Mutex::new(None),
            wake: std::sync::Mutex::new(wake),
        });
        let listener = Listener {
            shared: shared.clone(),
        };
        // `cmd /K <línea>` lee la línea tal cual: escapada, las comillas de
        // una ruta con espacios le llegarían como `\"` y la ruta no sirve.
        #[cfg(windows)]
        let escape_args = !(launch.program.to_ascii_lowercase().ends_with("cmd.exe")
            && launch.args.first().is_some_and(|flag| flag.eq_ignore_ascii_case("/K")));
        let options = tty::Options {
            shell: Some(tty::Shell::new(launch.program, launch.args)),
            working_directory: launch.cwd,
            drain_on_exit: true,
            env: [("TERM".to_string(), "xterm-256color".to_string()), ("COLORTERM".to_string(), "truecolor".to_string())]
                .into_iter()
                .chain(launch.env)
                .collect(),
            #[cfg(windows)]
            escape_args,
        };
        let window = window_size(size, cell);
        let pty = tty::new(&options, window, 0)?;
        let config = Config {
            scrolling_history: 5000,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let sender = event_loop.channel();
        // El hilo del PTY vive hasta que llega `Msg::Shutdown` (al soltar la
        // consola) o termina el proceso.
        drop(event_loop.spawn());
        Ok(Self {
            term,
            sender,
            shared,
            size,
        })
    }

    pub fn writer(&self) -> Writer {
        Writer(self.sender.clone())
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    /// Texto pegado: entre marcas de pegado si la app las pidió, para que una
    /// TUI no lo tome como teclas sueltas (un Enter en medio no envía).
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.write(format!("\x1b[200~{text}\x1b[201~").into_bytes());
        } else {
            self.write(text.into_bytes());
        }
    }

    pub fn resize(&mut self, size: GridSize, cell: (u16, u16)) {
        if size == self.size || size.cols < 2 || size.rows < 2 {
            return;
        }
        self.size = size;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(window_size(size, cell)));
    }

    /// Escribe `text` en la pantalla como si lo hubiera escrito el programa
    /// («[proceso terminado]» de Atic Code). No llega al proceso.
    pub fn note(&self, text: &str) {
        let mut parser: alacritty_terminal::vte::ansi::Processor = alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *self.term.lock(), text.as_bytes());
        self.shared.dirty.store(true, Ordering::Release);
    }

    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    /// Dónde está la vista: (líneas que subió en la historia, líneas de historia, filas en
    /// pantalla). Para la barra de desplazamiento.
    pub fn scroll_state(&self) -> (usize, usize, usize) {
        let term = self.term.lock();
        let grid = term.grid();
        (grid.display_offset(), grid.history_size(), grid.screen_lines())
    }

    /// Sube la vista hasta `offset` líneas en la historia (0: abajo del todo).
    pub fn scroll_to(&self, offset: usize) {
        let mut term = self.term.lock();
        let now = term.grid().display_offset() as i32;
        term.scroll_display(Scroll::Delta(offset as i32 - now));
    }

    /// La rueda, `lines` hacia arriba (positivo) o abajo, con el puntero en la celda
    /// (`col`, `row`): al programa si la pidió (ver `wheel_bytes`); si no, por la historia.
    pub fn wheel(&self, lines: i32, col: usize, row: usize) {
        let mode = *self.term.lock().mode();
        match wheel_bytes(mode, lines, col, row) {
            Some(bytes) => self.write(bytes),
            None => self.scroll(lines),
        }
    }

    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Relaxed)
    }

}

/// Lo que manda la rueda al programa: como rueda del mouse si la pidió (un editor, `htop`)
/// y, en la pantalla alternativa sin eso (`less`, `man`), como flechas. `None`: mueve la
/// historia.
pub fn wheel_bytes(mode: TermMode, lines: i32, col: usize, row: usize) -> Option<Vec<u8>> {
    if lines == 0 {
        return None;
    }
    let times = lines.unsigned_abs() as usize;
    if mode.intersects(TermMode::MOUSE_MODE) {
        let button: u8 = if lines > 0 { 64 } else { 65 };
        let one = if mode.contains(TermMode::SGR_MOUSE) {
            format!("\x1b[<{button};{};{}M", col + 1, row + 1).into_bytes()
        } else {
            // El formato de siempre: cada número + 32 en un byte (hasta la columna 222).
            let at = |n: usize| (n + 33).min(255) as u8;
            vec![0x1b, b'[', b'M', 32 + button, at(col), at(row)]
        };
        return Some(one.repeat(times));
    }
    if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        let app = mode.contains(TermMode::APP_CURSOR);
        let key: &[u8] = match (lines > 0, app) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        return Some(key.repeat(times));
    }
    None
}

impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

fn window_size(size: GridSize, cell: (u16, u16)) -> WindowSize {
    WindowSize {
        num_lines: size.rows as u16,
        num_cols: size.cols as u16,
        cell_width: cell.0,
        cell_height: cell.1,
    }
}

// --- Colores ---------------------------------------------------------------

/// Los colores con que se pinta una consola: los 16 ANSI, el texto y el
/// fondo de la terminal. Atic Code arma la suya según el estilo
/// (`code::terminal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub ansi: [u32; 16],
    pub fg: u32,
    pub bg: u32,
    /// El texto apagado (SGR 2 sin color propio).
    pub dim_fg: u32,
}

fn rgb_u32(rgb: Rgb) -> u32 {
    (rgb.r as u32) << 16 | (rgb.g as u32) << 8 | rgb.b as u32
}

/// Un color del terminal en RGB, con lo que la app haya redefinido (OSC 4)
/// por encima de la paleta.
pub fn resolve(color: Color, colors: &alacritty_terminal::term::color::Colors, palette: &Palette) -> u32 {
    match color {
        Color::Spec(rgb) => rgb_u32(rgb),
        Color::Indexed(index) => {
            if let Some(rgb) = colors[index as usize] {
                return rgb_u32(rgb);
            }
            indexed(index, palette)
        }
        Color::Named(named) => {
            if let Some(rgb) = colors[named] {
                return rgb_u32(rgb);
            }
            named_color(named, palette)
        }
    }
}

fn named_color(named: NamedColor, palette: &Palette) -> u32 {
    let index = named as usize;
    match named {
        NamedColor::Foreground | NamedColor::BrightForeground => palette.fg,
        NamedColor::Background => palette.bg,
        NamedColor::Cursor => palette.fg,
        NamedColor::DimForeground => palette.dim_fg,
        _ if index < 16 => palette.ansi[index],
        // DimBlack..DimWhite: el normal, apagado.
        _ => dim(palette.ansi[(index - NamedColor::DimBlack as usize).min(7)]),
    }
}

/// Los 256 colores: 16 ANSI, el cubo 6×6×6 y la escala de grises.
fn indexed(index: u8, palette: &Palette) -> u32 {
    match index {
        0..=15 => palette.ansi[index as usize],
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
            level(i / 36) << 16 | level((i / 6) % 6) << 8 | level(i % 6)
        }
        _ => {
            let v = 8 + (index as u32 - 232) * 10;
            v << 16 | v << 8 | v
        }
    }
}

pub fn dim(color: u32) -> u32 {
    let ch = |shift: u32| ((color >> shift & 0xff) * 2 / 3) << shift;
    ch(16) | ch(8) | ch(0)
}

/// Un color de la interfaz del espacio: cambia con el tema (`theme`).
pub fn hsla(color: u32) -> Hsla {
    crate::theme::hsla(color)
}

/// Un color de lo que pinta el programa en la terminal: igual en los dos
/// temas, porque la terminal siempre es oscura.
pub fn term(color: u32) -> Hsla {
    crate::theme::raw(color)
}

// --- Teclado ---------------------------------------------------------------

/// Lo que una tecla le manda al programa, como xterm. `None` si la tecla no
/// es para el terminal (la atiende el espacio).
pub fn key_bytes(keystroke: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    let key = keystroke.key.as_str();
    // En macOS lo que va con Comando es de la app (copiar, pegar, zoom), no
    // del shell: como en Terminal.
    if cfg!(target_os = "macos") && m.platform {
        return None;
    }
    // Flechas e Inicio/Fin: con modificadores van con el parámetro de xterm
    // (1;5 = Ctrl), sin ellos según el modo del cursor.
    let modifier = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.control as u8;
    let cursor = |letter: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{letter}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{code};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let bytes = match key {
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "delete" => tilde(3),
        "insert" => tilde(2),
        "enter" if m.alt => b"\x1b\r".to_vec(),
        // Shift+Enter: salto de línea sin enviar, como lo espera Claude Code.
        "enter" if m.shift => b"\x1b\r".to_vec(),
        "enter" => b"\r".to_vec(),
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" if m.control => b"\x17".to_vec(),
        "backspace" if m.alt => b"\x1b\x7f".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "space" if m.control => vec![0],
        "space" if m.alt => b"\x1b ".to_vec(),
        f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
            let n: u8 = f[1..].parse().ok()?;
            match n {
                1 => b"\x1bOP".to_vec(),
                2 => b"\x1bOQ".to_vec(),
                3 => b"\x1bOR".to_vec(),
                4 => b"\x1bOS".to_vec(),
                5 => tilde(15),
                6 => tilde(17),
                7 => tilde(18),
                8 => tilde(19),
                9 => tilde(20),
                10 => tilde(21),
                11 => tilde(23),
                12 => tilde(24),
                _ => return None,
            }
        }
        _ => {
            // Ctrl+letra → el carácter de control (Ctrl+C = 0x03).
            if m.control && !m.alt && key.len() == 1 {
                let c = key.as_bytes()[0].to_ascii_lowercase();
                return match c {
                    b'a'..=b'z' => Some(vec![c - b'a' + 1]),
                    b'[' => Some(vec![0x1b]),
                    b'\\' => Some(vec![0x1c]),
                    b']' => Some(vec![0x1d]),
                    _ => None,
                };
            }
            // Alt+letra: ESC y la letra, como xterm.
            let text = keystroke.key_char.as_deref()?;
            if m.alt && !m.control {
                let mut out = vec![0x1b];
                out.extend_from_slice(text.as_bytes());
                return Some(out);
            }
            // El resto es texto (letras, símbolos, espacio, AltGr, tildes) y no se
            // atiende aquí: lo entrega el sistema al manejador de texto del
            // espacio (`input.rs`). Si lo escribiera esta función, una tecla
            // muerta como `´` saldría sola y la `a` que sigue no se juntaría con
            // ella (`´a` en vez de `á`), y AltGr —que GPUI no reconoce en la
            // distribución latinoamericana y trae como Ctrl+Alt— se perdería.
            return None;
        }
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn la_rueda_va_al_programa_solo_si_la_pidio() {
        // Un shell: mueve la historia.
        assert_eq!(wheel_bytes(TermMode::default(), 3, 0, 0), None);
        // `less` en la pantalla alternativa: flechas, una por línea.
        let alt = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(wheel_bytes(alt, 2, 0, 0), Some(b"\x1b[A\x1b[A".to_vec()));
        assert_eq!(wheel_bytes(alt | TermMode::APP_CURSOR, -1, 0, 0), Some(b"\x1bOB".to_vec()));
        // Un editor que pidió el mouse: la rueda en la celda del puntero.
        let mouse = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(wheel_bytes(mouse, -1, 4, 9), Some(b"\x1b[<65;5;10M".to_vec()));
        assert_eq!(wheel_bytes(TermMode::MOUSE_REPORT_CLICK, 1, 0, 0), Some(vec![0x1b, b'[', b'M', 96, 33, 33]));
    }

    #[test]
    fn teclas_como_xterm() {
        assert_eq!(key_bytes(&ks("ctrl-c"), false), Some(vec![3]));
        assert_eq!(key_bytes(&ks("up"), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(&ks("up"), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(&ks("ctrl-right"), false), Some(b"\x1b[1;5C".to_vec()));
        assert_eq!(key_bytes(&ks("enter"), false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(&ks("shift-tab"), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_bytes(&ks("f5"), false), Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn el_texto_lo_entrega_el_sistema_y_no_key_bytes() {
        // Letras, símbolos y mayúsculas: no se atienden, para que una tecla
        // muerta (`´`) pueda juntarse con la vocal que sigue y salga `á`.
        assert_eq!(key_bytes(&ks("a->a"), false), None);
        assert_eq!(key_bytes(&ks("shift-a->A"), false), None);
        assert_eq!(key_bytes(&ks("´->´"), false), None);
        // La barra espaciadora (GPUI en Windows no le pone `key_char`) tampoco:
        // llega como texto, igual que cualquier letra.
        assert_eq!(ks("space").key_char, None);
        assert_eq!(key_bytes(&ks("space"), false), None);
        assert_eq!(key_bytes(&ks("shift-space"), false), None);
        // AltGr: en la distribución latinoamericana GPUI lo ve como Ctrl+Alt y
        // trae el carácter ya resuelto. Tampoco se atiende: sale por el texto.
        assert_eq!(key_bytes(&ks("ctrl-alt-q->@"), false), None);
        assert_eq!(key_bytes(&ks("ctrl-alt-\\->\\"), false), None);
    }

    #[test]
    fn los_atajos_con_ctrl_o_alt_siguen_yendo_al_pty() {
        assert_eq!(key_bytes(&ks("ctrl-c"), false), Some(vec![3]));
        assert_eq!(key_bytes(&ks("ctrl-space"), false), Some(vec![0]));
        assert_eq!(key_bytes(&ks("alt-space"), false), Some(b"\x1b ".to_vec()));
        assert_eq!(key_bytes(&ks("alt-b->b"), false), Some(b"\x1bb".to_vec()));
        // Ctrl+Alt+C en un teclado sin AltGr no trae carácter: no escribe nada.
        assert_eq!(key_bytes(&ks("ctrl-alt-c"), false), None);
    }

    #[test]
    fn paleta_de_256() {
        let p = Palette { ansi: [0xe06c6c; 16], fg: 0xe8e8e0, bg: 0x161615, dim_fg: 0xa0a098 };
        assert_eq!(indexed(16, &p), 0x000000);
        assert_eq!(indexed(231, &p), 0xffffff);
        assert_eq!(indexed(232, &p), 0x080808);
        assert_eq!(indexed(1, &p), 0xe06c6c);
        // Con otra paleta cambian los 16 primeros, no el cubo.
        let other = Palette { ansi: [0x123456; 16], ..p };
        assert_eq!(indexed(1, &other), 0x123456);
        assert_eq!(indexed(231, &other), 0xffffff);
    }
}
