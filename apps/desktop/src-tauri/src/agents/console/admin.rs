//! Consola de administrador: PowerShell elevado dentro de una tarjeta de Atic.
//!
//! Un proceso sin elevar no puede ser dueño de la consola de uno elevado:
//! `CreateProcess` no eleva, y `ShellExecute` con `runas` crea el proceso en su
//! propia consola. Así que Atic hace de puente, como `gsudo`:
//!
//! 1. Atic crea dos *named pipes* con nombres aleatorios —uno por dirección:
//!    en un handle síncrono, un `ReadFile` bloqueado frena al `WriteFile`—.
//! 2. Se relanza a sí mismo con `runas`: Windows muestra el UAC, Sí o No.
//! 3. La copia elevada (`run_bridge`) abre su propio ConPTY con PowerShell,
//!    se conecta a los pipes y se presenta con la clave que recibió.
//! 4. De ahí en más, teclado y tamaño viajan hacia la copia; la salida y el
//!    código de salida, de vuelta. Para `launch` es una consola más: esto
//!    implementa `MasterPty` y `Child` sobre los pipes.
//!
//! La copia elevada no hace nada más que eso, y muere con su consola o en
//! cuanto se corta el pipe (Atic cerró la tarjeta, salió o se cayó).
//!
//! Protocolo: marcos `[tipo u8][largo u32 LE][datos]`.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, ExitStatus, PtySize};

use atic_core::MutexExt;

use super::{Opened, PtyResize};

/// Argumento con que Atic se relanza como puente elevado.
pub const BRIDGE_ARG: &str = "--atic-admin-bridge";

/// Cuánto se espera a que la copia elevada se conecte tras aceptar el UAC.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

// Atic → puente.
const IN_INPUT: u8 = 1;
const IN_RESIZE: u8 = 2;
const IN_KILL: u8 = 3;
// Puente → Atic.
const OUT_HELLO: u8 = 0;
const OUT_DATA: u8 = 1;
const OUT_EXIT: u8 = 2;
const OUT_ERROR: u8 = 3;

/// Tope de un marco: nada legítimo se acerca; un largo absurdo es un pipe roto.
const MAX_FRAME: usize = 1 << 20;

fn write_frame(w: &mut impl Write, kind: u8, data: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(5 + data.len());
    frame.push(kind);
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(data);
    w.write_all(&frame)?;
    w.flush()
}

fn read_frame(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "marco demasiado grande",
        ));
    }
    let mut data = vec![0u8; len];
    r.read_exact(&mut data)?;
    Ok((head[0], data))
}

fn resize_payload(size: PtySize) -> [u8; 4] {
    let [c0, c1] = size.cols.to_le_bytes();
    let [r0, r1] = size.rows.to_le_bytes();
    [c0, c1, r0, r1]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// --- Lado de Atic ----------------------------------------------------------

/// Un pipe servidor recién creado. Se cierra solo si nadie lo toma.
struct ServerPipe(isize);

impl ServerPipe {
    fn create(name: &str, inbound: bool) -> Result<Self, String> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND,
        };
        use windows_sys::Win32::System::Pipes::{
            CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
            PIPE_WAIT,
        };
        let access = if inbound {
            PIPE_ACCESS_INBOUND
        } else {
            PIPE_ACCESS_OUTBOUND
        };
        let path = wide(&format!(r"\\.\pipe\{name}"));
        // FIRST_PIPE_INSTANCE: si el nombre ya existía, otro lo creó antes y
        // no es nuestro. Una sola instancia: nadie más se cuelga del pipe.
        // SAFETY: `path` termina en 0 y vive durante la llamada.
        let handle = unsafe {
            CreateNamedPipeW(
                path.as_ptr(),
                access | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!(
                "No se pudo preparar la consola de administrador: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self(handle as isize))
    }

    /// Bloquea hasta que un cliente se conecte.
    fn connect(&self) -> io::Result<()> {
        use windows_sys::Win32::Foundation::ERROR_PIPE_CONNECTED;
        use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
        // SAFETY: handle de pipe propio y vivo; sin OVERLAPPED es bloqueante.
        if unsafe { ConnectNamedPipe(self.0 as _, std::ptr::null_mut()) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) {
            return Ok(());
        }
        Err(error)
    }

    fn into_file(self) -> File {
        let handle = self.0;
        std::mem::forget(self);
        // SAFETY: el handle es nuestro y a partir de acá lo cierra el `File`.
        unsafe { File::from_raw_handle(handle as RawHandle) }
    }
}

impl Drop for ServerPipe {
    fn drop(&mut self) {
        // SAFETY: handle propio, se cierra una vez.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _) };
    }
}

/// Se relanza elevado. `Ok(handle)` del proceso; `Err` si dijiste que no.
fn launch_elevated(params: String) -> Result<isize, String> {
    // ShellExecuteEx quiere COM en el hilo: uno propio, que no es de nadie.
    std::thread::spawn(move || {
        use windows_sys::Win32::Foundation::ERROR_CANCELLED;
        use windows_sys::Win32::UI::Shell::{
            ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let exe = std::env::current_exe()
            .map_err(|e| format!("No se encontró el ejecutable de Atic: {e}"))?;
        let verb = wide("runas");
        let file = wide(&exe.to_string_lossy());
        let params = wide(&params);
        // SAFETY: estructura en cero con su tamaño; los textos viven hasta el final.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        // SAFETY: `info` bien formada; bloquea mientras el UAC espera respuesta.
        let (ok, error) = crate::launcher_icons::with_com(|| {
            let ok = unsafe { ShellExecuteExW(&mut info) } != 0;
            (ok, io::Error::last_os_error())
        });
        if !ok {
            if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
                return Err("Cancelaste el permiso de administrador.".to_string());
            }
            return Err(format!("No se pudo abrir como administrador: {error}"));
        }
        if info.hProcess.is_null() {
            return Err("Windows no devolvió el proceso de administrador.".to_string());
        }
        Ok(info.hProcess as isize)
    })
    .join()
    .map_err(|_| "El permiso de administrador falló.".to_string())?
}

/// Abre la consola de administrador: UAC, puente y apretón de manos.
pub(super) fn open(cwd: Option<&str>, size: PtySize) -> Result<Opened, String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut secret = uuid::Uuid::new_v4().as_bytes().to_vec();
    secret.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    let token = hex(&secret);
    let in_name = format!("atic-admin-{id}-in");
    let out_name = format!("atic-admin-{id}-out");
    // Nombres desde el punto de vista del puente: "in" es lo que él lee.
    let to_bridge = ServerPipe::create(&in_name, false)?;
    let from_bridge = ServerPipe::create(&out_name, true)?;

    let cwd_hex = hex(cwd.map(str::trim).unwrap_or("").as_bytes());
    let params = format!(
        "{BRIDGE_ARG} {in_name} {out_name} {token} {} {} {}",
        size.cols,
        size.rows,
        if cwd_hex.is_empty() {
            "-".to_string()
        } else {
            cwd_hex
        }
    );
    let process = launch_elevated(params)?;

    // Conectar con tope: si la copia elevada muere antes, nadie conectaría y
    // `ConnectNamedPipe` esperaría para siempre. Se destraba conectándose uno
    // mismo como cliente.
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let result = to_bridge
            .connect()
            .and_then(|_| from_bridge.connect())
            .map(|_| (to_bridge, from_bridge));
        let _ = tx.send(());
        result
    });
    let deadline = std::time::Instant::now() + CONNECT_TIMEOUT;
    let mut connected = false;
    while std::time::Instant::now() < deadline {
        if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            connected = true;
            break;
        }
        if process_exited(process) {
            break;
        }
    }
    if !connected {
        // Del lado cliente: `in` se lee y `out` se escribe.
        let _ = OpenOptions::new()
            .read(true)
            .open(format!(r"\\.\pipe\{in_name}"));
        let _ = OpenOptions::new()
            .write(true)
            .open(format!(r"\\.\pipe\{out_name}"));
    }
    let pipes = waiter
        .join()
        .map_err(|_| "El puente de administrador falló.".to_string())?;
    close_handle(process);
    if !connected {
        return Err("La consola de administrador no respondió.".to_string());
    }
    let (to_bridge, from_bridge) =
        pipes.map_err(|e| format!("La consola de administrador no se conectó: {e}"))?;
    let mut input = to_bridge.into_file();
    let mut output = from_bridge.into_file();

    // El puente se presenta con la clave: sin ella, no es el nuestro.
    let (kind, hello) = read_frame(&mut output)
        .map_err(|e| format!("La consola de administrador no respondió: {e}"))?;
    if kind == OUT_ERROR {
        return Err(String::from_utf8_lossy(&hello).into_owned());
    }
    if kind != OUT_HELLO
        || hello.len() != token.len() + 4
        || hello[..token.len()] != *token.as_bytes()
    {
        let _ = write_frame(&mut input, IN_KILL, &[]);
        return Err("La consola de administrador no se identificó.".to_string());
    }
    let pid = u32::from_le_bytes([
        hello[token.len()],
        hello[token.len() + 1],
        hello[token.len() + 2],
        hello[token.len() + 3],
    ]);

    let input = Arc::new(Mutex::new(input));
    let exit = Arc::new((Mutex::new(None::<u32>), Condvar::new()));
    let (data_tx, data_rx) = mpsc::channel();
    spawn_demux(output, data_tx, Arc::clone(&exit));

    Ok(Opened {
        reader: Box::new(ChannelReader {
            rx: data_rx,
            pending: Vec::new(),
            pos: 0,
        }),
        writer: Box::new(InputWriter(Arc::clone(&input))),
        resize: Box::new(AdminResize(Arc::clone(&input))),
        child: Box::new(AdminChild { input, exit, pid }),
    })
}

fn process_exited(process: isize) -> bool {
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;
    // SAFETY: handle de proceso propio, todavía abierto.
    unsafe { WaitForSingleObject(process as _, 0) == WAIT_OBJECT_0 }
}

fn close_handle(handle: isize) {
    // SAFETY: handle propio, se cierra una vez.
    unsafe { windows_sys::Win32::Foundation::CloseHandle(handle as _) };
}

/// Separa lo que manda el puente: la salida va al lector de la consola; el
/// código de salida, a quien espera. El orden del pipe asegura que la salida
/// llega entera antes del código.
fn spawn_demux(mut output: File, data: Sender<Vec<u8>>, exit: Arc<(Mutex<Option<u32>>, Condvar)>) {
    std::thread::spawn(move || {
        let code = loop {
            match read_frame(&mut output) {
                Ok((OUT_DATA, bytes)) => {
                    if data.send(bytes).is_err() {
                        break 1;
                    }
                }
                Ok((OUT_EXIT, bytes)) if bytes.len() == 4 => {
                    break u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                }
                Ok(_) => {}
                // Pipe cortado sin código: el puente murió.
                Err(_) => break 1,
            }
        };
        drop(data);
        let (lock, cvar) = &*exit;
        *lock.lock_or_recover() = Some(code);
        cvar.notify_all();
    });
}

/// Lector de la salida: EOF cuando el puente terminó.
struct ChannelReader {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.pending.len() {
            match self.rx.recv() {
                Ok(bytes) => {
                    self.pending = bytes;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len() - self.pos);
        buf[..n].copy_from_slice(&self.pending[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct InputWriter(Arc<Mutex<File>>);

impl Write for InputWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_frame(&mut *self.0.lock_or_recover(), IN_INPUT, buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct AdminResize(Arc<Mutex<File>>);

impl PtyResize for AdminResize {
    fn resize(&self, size: PtySize) -> Result<(), String> {
        write_frame(
            &mut *self.0.lock_or_recover(),
            IN_RESIZE,
            &resize_payload(size),
        )
        .map_err(|e| e.to_string())
    }
}

#[derive(Debug)]
struct AdminKiller(Arc<Mutex<File>>);

impl ChildKiller for AdminKiller {
    fn kill(&mut self) -> io::Result<()> {
        // Si el pipe ya se cortó, el puente ya se fue: nada que matar.
        let _ = write_frame(&mut *self.0.lock_or_recover(), IN_KILL, &[]);
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(AdminKiller(Arc::clone(&self.0)))
    }
}

struct AdminChild {
    input: Arc<Mutex<File>>,
    exit: Arc<(Mutex<Option<u32>>, Condvar)>,
    pid: u32,
}

impl std::fmt::Debug for AdminChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminChild")
            .field("pid", &self.pid)
            .finish()
    }
}

impl ChildKiller for AdminChild {
    fn kill(&mut self) -> io::Result<()> {
        AdminKiller(Arc::clone(&self.input)).kill()
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(AdminKiller(Arc::clone(&self.input)))
    }
}

impl Child for AdminChild {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(self
            .exit
            .0
            .lock_or_recover()
            .map(ExitStatus::with_exit_code))
    }

    fn wait(&mut self) -> io::Result<ExitStatus> {
        let (lock, cvar) = &*self.exit;
        let mut code = lock.lock_or_recover();
        while code.is_none() {
            code = cvar.wait(code).unwrap_or_else(|e| e.into_inner());
        }
        Ok(ExitStatus::with_exit_code(code.unwrap_or(1)))
    }

    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn as_raw_handle(&self) -> Option<RawHandle> {
        None
    }
}

// --- Lado elevado ----------------------------------------------------------

/// Punto de entrada de la copia elevada. No vuelve: sale con el código de la
/// consola.
///
/// `args` es lo que viene después de `BRIDGE_ARG`: pipe de entrada, pipe de
/// salida, clave, columnas, filas y carpeta (hex o `-`).
pub fn run_bridge(args: &[String]) -> ! {
    let code = bridge(args).unwrap_or(1);
    std::process::exit(code as i32)
}

fn open_pipe(name: &str, read: bool) -> io::Result<File> {
    let path = format!(r"\\.\pipe\{name}");
    let mut last = None;
    for _ in 0..50 {
        let opened = if read {
            OpenOptions::new().read(true).open(&path)
        } else {
            OpenOptions::new().write(true).open(&path)
        };
        match opened {
            Ok(file) => return Ok(file),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("pipe no disponible")))
}

fn bridge(args: &[String]) -> Result<u32, String> {
    let [in_name, out_name, token, cols, rows, cwd] = args else {
        return Err("argumentos del puente".into());
    };
    // Solo nombres propios: esto corre elevado y no abre pipes ajenos.
    let valid = |n: &str| {
        n.starts_with("atic-admin-") && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if !valid(in_name) || !valid(out_name) {
        return Err("nombre de pipe inválido".into());
    }
    let mut input = open_pipe(in_name, true).map_err(|e| e.to_string())?;
    let output = Arc::new(Mutex::new(
        open_pipe(out_name, false).map_err(|e| e.to_string())?,
    ));
    let fail = |message: String| {
        let _ = write_frame(
            &mut *output.lock_or_recover(),
            OUT_ERROR,
            message.as_bytes(),
        );
        message
    };

    let size = PtySize {
        cols: cols.parse().unwrap_or(80).max(2),
        rows: rows.parse().unwrap_or(24).max(2),
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|e| fail(format!("No se pudo abrir la consola: {e}")))?;
    let mut cmd = CommandBuilder::new(powershell());
    cmd.arg("-NoLogo");
    super::apply_terminal_color_env(&mut cmd);
    super::apply_clean_script_env(&mut cmd);
    if cwd != "-" {
        if let Some(dir) = unhex(cwd).and_then(|b| String::from_utf8(b).ok()) {
            super::apply_cwd(&mut cmd, Some(&dir));
        }
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| fail(format!("No se pudo abrir PowerShell: {e}")))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| fail(e.to_string()))?;
    let mut writer = pair.master.take_writer().map_err(|e| fail(e.to_string()))?;
    let mut killer = child.clone_killer();

    let mut hello = token.as_bytes().to_vec();
    hello.extend_from_slice(&child.process_id().unwrap_or(0).to_le_bytes());
    write_frame(&mut *output.lock_or_recover(), OUT_HELLO, &hello).map_err(|e| e.to_string())?;

    // Salida de la consola → Atic.
    let out = Arc::clone(&output);
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if write_frame(&mut *out.lock_or_recover(), OUT_DATA, &buf[..n]).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Fin de la consola → código a Atic y salida del puente.
    let out = Arc::clone(&output);
    std::thread::spawn(move || {
        let code = child.wait().map(|s| s.exit_code()).unwrap_or(1);
        // ConPTY entrega la última salida después de que el proceso terminó.
        std::thread::sleep(Duration::from_millis(200));
        let _ = write_frame(&mut *out.lock_or_recover(), OUT_EXIT, &code.to_le_bytes());
        std::process::exit(code as i32);
    });

    // Teclado y tamaño desde Atic. Pipe cortado = Atic se fue: cerrar todo.
    loop {
        match read_frame(&mut input) {
            Ok((IN_INPUT, bytes)) => {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
            Ok((IN_RESIZE, bytes)) if bytes.len() == 4 => {
                let _ = pair.master.resize(PtySize {
                    cols: u16::from_le_bytes([bytes[0], bytes[1]]).max(2),
                    rows: u16::from_le_bytes([bytes[2], bytes[3]]).max(2),
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
            Ok((IN_KILL, _)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    let _ = killer.kill();
    Ok(1)
}

fn powershell() -> std::path::PathBuf {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::path::PathBuf::from(sysroot).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_survive_the_round_trip() {
        let mut wire = Vec::new();
        write_frame(&mut wire, IN_INPUT, b"dir\r").unwrap();
        write_frame(
            &mut wire,
            IN_RESIZE,
            &resize_payload(PtySize {
                cols: 120,
                rows: 40,
                pixel_width: 0,
                pixel_height: 0,
            }),
        )
        .unwrap();
        write_frame(&mut wire, IN_KILL, &[]).unwrap();
        let mut r = io::Cursor::new(wire);
        assert_eq!(read_frame(&mut r).unwrap(), (IN_INPUT, b"dir\r".to_vec()));
        assert_eq!(
            read_frame(&mut r).unwrap(),
            (IN_RESIZE, vec![120, 0, 40, 0])
        );
        assert_eq!(read_frame(&mut r).unwrap(), (IN_KILL, vec![]));
        assert!(
            read_frame(&mut r).is_err(),
            "sin más marcos es EOF, no basura"
        );
    }

    #[test]
    fn rejects_absurd_frame_lengths() {
        let mut wire = vec![IN_INPUT];
        wire.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut io::Cursor::new(wire)).is_err());
    }

    #[test]
    fn folder_travels_as_hex_even_with_a_trailing_backslash() {
        // `"C:\"` entre comillas rompería el parseo de argumentos de Windows.
        for dir in [r"C:\", r"C:\Users\Ana María\proyecto", ""] {
            assert_eq!(unhex(&hex(dir.as_bytes())).unwrap(), dir.as_bytes());
        }
        assert!(unhex("zz").is_none());
        assert!(unhex("abc").is_none());
    }
}
