//! Un monitor en vivo con DXGI Desktop Duplication (Windows 8+): la pantalla del PC en
//! el celular.
//!
//! A diferencia de `BitBlt`, el sistema avisa cuándo cambió algo (con la pantalla quieta
//! no hay nada que copiar) y la imagen ya está en la GPU: solo se baja a memoria el
//! cuadro que se va a mandar, por una textura de lectura.
//!
//! Si no se puede (sesión remota, pantalla girada, otra GPU, Windows viejo), quien la
//! usa vuelve a `BitBlt`.

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};

use crate::frame::Frame;
use crate::geometry::Rect;
use crate::monitors::MonitorInfo;

/// La duplicación de un monitor. Si se pierde (cambio de modo, UAC, bloqueo),
/// [`Duplicator::next_frame`] devuelve `Err` y hay que crear otro.
pub struct Duplicator {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    /// La textura de lectura, del tamaño del monitor (se crea con el primer cuadro).
    staging: Option<ID3D11Texture2D>,
    bounds: Rect,
}

// SAFETY: el dispositivo de D3D11 es libre de hilos (no se crea con
// `D3D11_CREATE_DEVICE_SINGLETHREADED`) y el contexto inmediato solo lo usa quien tiene
// el `Duplicator` (`&mut self`).
unsafe impl Send for Duplicator {}

impl Duplicator {
    /// Duplica el monitor `monitor` (lo busca por su rectángulo en el escritorio).
    pub fn new(monitor: &MonitorInfo) -> Result<Self, String> {
        let b = monitor.bounds;
        // SAFETY: llamadas COM de DXGI/D3D11 con punteros de salida locales.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| format!("DXGI: {e}"))?;
            let mut adapter_index = 0;
            while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
                adapter_index += 1;
                let mut output_index = 0;
                while let Ok(output) = adapter.EnumOutputs(output_index) {
                    output_index += 1;
                    let Ok(desc) = output.GetDesc() else { continue };
                    let r = desc.DesktopCoordinates;
                    if (r.left, r.top, r.right - r.left, r.bottom - r.top) != (b.x, b.y, b.width as i32, b.height as i32) {
                        continue;
                    }
                    if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY && desc.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
                        return Err("la pantalla está girada".into());
                    }
                    let mut device = None;
                    let mut context = None;
                    D3D11CreateDevice(
                        &adapter.cast::<IDXGIAdapter>().map_err(|e| e.to_string())?,
                        D3D_DRIVER_TYPE_UNKNOWN,
                        HMODULE::default(),
                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                        None,
                        D3D11_SDK_VERSION,
                        Some(&mut device),
                        None,
                        Some(&mut context),
                    )
                    .map_err(|e| format!("D3D11: {e}"))?;
                    let device = device.ok_or("D3D11 sin dispositivo")?;
                    let context = context.ok_or("D3D11 sin contexto")?;
                    let duplication = output.cast::<IDXGIOutput1>().map_err(|e| e.to_string())?.DuplicateOutput(&device).map_err(|e| format!("DuplicateOutput: {e}"))?;
                    return Ok(Self { device, context, duplication, staging: None, bounds: b });
                }
            }
        }
        Err("DXGI no encontró ese monitor".into())
    }

    /// El próximo cuadro, esperando hasta `timeout_ms`. `Ok(None)` si en ese rato no
    /// cambió la imagen (solo el puntero, o nada).
    pub fn next_frame(&mut self, timeout_ms: u32) -> Result<Option<Frame>, String> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        // SAFETY: llamadas COM sobre objetos vivos; el cuadro adquirido se suelta antes
        // de volver (aunque falle la copia).
        unsafe {
            match self.duplication.AcquireNextFrame(timeout_ms, &mut info, &mut resource) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
                Err(e) => return Err(format!("AcquireNextFrame: {e}")),
            }
            let copied = self.copy_frame(&info, resource);
            let _ = self.duplication.ReleaseFrame();
            copied
        }
    }

    unsafe fn copy_frame(&mut self, info: &DXGI_OUTDUPL_FRAME_INFO, resource: Option<IDXGIResource>) -> Result<Option<Frame>, String> {
        // Solo se movió el puntero: la imagen es la misma.
        if info.LastPresentTime == 0 {
            return Ok(None);
        }
        let texture: ID3D11Texture2D = resource.ok_or("cuadro sin imagen")?.cast().map_err(|e| e.to_string())?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: getter sobre la textura viva.
        unsafe { texture.GetDesc(&mut desc) };
        if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(format!("formato inesperado: {:?}", desc.Format));
        }
        let staging = match &self.staging {
            Some(staging) => staging.clone(),
            None => {
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width: desc.Width,
                    Height: desc.Height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: desc.Format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };
                let mut staging = None;
                // SAFETY: crea una textura con la descripción local.
                unsafe { self.device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }.map_err(|e| format!("CreateTexture2D: {e}"))?;
                let staging = staging.ok_or("sin textura de lectura")?;
                self.staging = Some(staging.clone());
                staging
            }
        };
        let (width, height) = (desc.Width as usize, desc.Height as usize);
        let mut bgra = vec![0u8; width * height * 4];
        // SAFETY: copia en la GPU y lectura de la textura mapeada fila por fila (cada
        // una mide `RowPitch`, que puede ser más que el ancho).
        unsafe {
            self.context.CopyResource(&staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|e| format!("Map: {e}"))?;
            let pitch = mapped.RowPitch as usize;
            let source = mapped.pData as *const u8;
            for row in 0..height {
                let from = std::slice::from_raw_parts(source.add(row * pitch), width * 4);
                bgra[row * width * 4..(row + 1) * width * 4].copy_from_slice(from);
            }
            self.context.Unmap(&staging, 0);
        }
        Ok(Some(Frame::new(Rect::new(self.bounds.x, self.bounds.y, width as u32, height as u32), bgra)))
    }
}
