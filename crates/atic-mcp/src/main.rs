//! `atic-mcp`: el servidor MCP de Atic (stdio).
//!
//! Binario chico, sin ventana: lo carga el host (Cursor, `codex`, `claude`,
//! OpenCode) y traduce `tools/call` a pedidos del hub por HTTP en localhost.
//! Nada, nunca, a stdout fuera de `rmcp`: los logs van a stderr o se rompe
//! el transporte.

use atic_mcp::tools;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // A mano, sin `clap`: son dos flags.
    let mut host: Option<String> = None;
    let mut wait: Option<u64> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => host = args.next().map(|v| v.trim().to_string()),
            "--wait" => {
                wait = args.next().and_then(|v| v.parse().ok());
            }
            _ => {}
        }
    }
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    vigilar_al_padre();
    let (servidor, _) = tools::router(host, wait);
    let servicio = servidor.serve(stdio()).await?;
    servicio.waiting().await?;
    Ok(())
}

/// Sale cuando muere el host. Cerrar stdin no basta en Windows: si el host
/// muere sin cerrar el pipe (o un nieto heredó el handle), el EOF no llega y
/// el servidor queda huérfano para siempre.
#[cfg(windows)]
fn vigilar_al_padre() {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
    };

    let propio = unsafe { GetCurrentProcessId() };
    let mut padre = None;
    unsafe {
        let foto = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if foto == INVALID_HANDLE_VALUE {
            return;
        }
        let mut entrada: PROCESSENTRY32W = std::mem::zeroed();
        entrada.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut hay = Process32FirstW(foto, &mut entrada) != 0;
        while hay {
            if entrada.th32ProcessID == propio {
                padre = Some(entrada.th32ParentProcessID);
                break;
            }
            hay = Process32NextW(foto, &mut entrada) != 0;
        }
        CloseHandle(foto);
    }
    let Some(padre) = padre else {
        return;
    };
    // El handle se toma ahora: si el pid se reusara después, seguimos
    // esperando al proceso de verdad.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, padre) };
    if handle.is_null() {
        return;
    }
    let handle = handle as usize;
    std::thread::spawn(move || {
        unsafe { WaitForSingleObject(handle as _, INFINITE) };
        std::process::exit(0);
    });
}

#[cfg(unix)]
fn vigilar_al_padre() {
    let padre = std::os::unix::process::parent_id();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        // Huérfano: lo adoptó init (o un subreaper) y el pid cambió.
        if std::os::unix::process::parent_id() != padre {
            std::process::exit(0);
        }
    });
}
