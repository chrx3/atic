//! El dispositivo de gráficos que comparten el reproductor, el compositor y el exportador
//! del editor (Windows): un `ID3D11Device` de hardware con soporte BGRA y de video, protegido
//! para varios hilos, y el administrador DXGI que Media Foundation necesita para dejar los
//! cuadros en la GPU.
//!
//! Cada hilo tiene el suyo (`Gfx::shared`): la interfaz del editor usa uno para el
//! reproductor y el compositor (así se pasan texturas sin copiarlas) y el hilo del
//! exportador crea otro aparte.

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::sync::Mutex;

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;
use windows::Win32::Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager};

use super::info_win::win;

#[derive(Clone)]
pub(super) struct Gfx {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub manager: IMFDXGIDeviceManager,
}

thread_local! {
    /// Sin `Drop` a propósito: liberar el dispositivo cuando el hilo muere (con un codificador de
    /// hardware que aún lo retiene) deja el proceso colgado. Quien exporta lo devuelve al pool
    /// (`release_thread`) y el de la interfaz vive lo que la app.
    static SHARED: RefCell<ManuallyDrop<Option<Gfx>>> = const { RefCell::new(ManuallyDrop::new(None)) };
}

/// Un dispositivo guardado para otro hilo. Direct3D lo deja usar desde varios (se protegió en `new`).
struct Pooled(Gfx);
// SAFETY: el dispositivo es de hilos protegidos (`SetMultithreadProtected`) y solo un hilo a la vez
// lo tiene en su `SHARED`; el pool lo entrega una vez.
unsafe impl Send for Pooled {}

/// Dispositivos libres de hilos que ya terminaron (las exportaciones corren cada una en el suyo).
static POOL: Mutex<Vec<Pooled>> = Mutex::new(Vec::new());

impl Gfx {
    pub(super) fn new() -> Result<Self, String> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        // SAFETY: crea un dispositivo y lo protege para varios hilos; el administrador
        // DXGI toma el dispositivo con el token que él mismo entrega.
        unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|error| format!("no se pudo crear el dispositivo de gráficos: {}", win(error)))?;
            let device = device.ok_or("Direct3D no entregó el dispositivo")?;
            let context = context.ok_or("Direct3D no entregó el contexto")?;
            if let Ok(multithread) = device.cast::<ID3D10Multithread>() {
                let _ = multithread.SetMultithreadProtected(true);
            }
            let mut token = 0u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(win)?;
            let manager = manager.ok_or("Media Foundation no dio el administrador DXGI")?;
            manager.ResetDevice(&device, token).map_err(win)?;
            Ok(Self { device, context, manager })
        }
    }

    /// El dispositivo de este hilo (se crea la primera vez).
    pub(super) fn shared() -> Result<Self, String> {
        SHARED.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some(gfx) = slot.as_ref() {
                return Ok(gfx.clone());
            }
            let pooled = POOL.lock().ok().and_then(|mut pool| pool.pop());
            let gfx = match pooled {
                Some(Pooled(gfx)) => gfx,
                None => Self::new()?,
            };
            *slot = ManuallyDrop::new(Some(gfx.clone()));
            Ok(gfx)
        })
    }

    /// El hilo termina: su dispositivo pasa al pool para el próximo (y no se libera con el hilo).
    pub(super) fn release_thread() {
        let taken = SHARED.with(|slot| slot.borrow_mut().take());
        if let (Some(gfx), Ok(mut pool)) = (taken, POOL.lock()) {
            pool.push(Pooled(gfx));
        }
    }

    /// Los píxeles de una textura BGRA de la GPU, arriba a la izquierda primero: `(ancho, alto, bytes)`.
    pub(super) fn read(&self, texture: &ID3D11Texture2D) -> Result<(u32, u32, Vec<u8>), String> {
        // SAFETY: copia a una textura de lectura y la mapea; se desmapea antes de salir.
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..desc
            };
            let mut staging: Option<ID3D11Texture2D> = None;
            self.device.CreateTexture2D(&staging_desc, None, Some(&mut staging)).map_err(win)?;
            let staging = staging.ok_or("Direct3D no creó la textura de lectura")?;
            let (to, from): (ID3D11Resource, ID3D11Resource) = (staging.cast().map_err(win)?, texture.cast().map_err(win)?);
            self.context.CopyResource(&to, &from);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&to, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(win)?;
            let (w, h) = (desc.Width as usize, desc.Height as usize);
            let pitch = mapped.RowPitch as usize;
            let source = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * h);
            let mut bytes = Vec::with_capacity(w * h * 4);
            for row in source.chunks_exact(pitch) {
                bytes.extend_from_slice(&row[..w * 4]);
            }
            self.context.Unmap(&to, 0);
            Ok((desc.Width, desc.Height, bytes))
        }
    }
}
