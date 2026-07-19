//! Enumeración nativa de endpoints WASAPI para persistir IDs estables.

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToString};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

struct ComGuard(bool);

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}

pub fn active_endpoints(capture: bool) -> Result<Vec<Endpoint>, String> {
    // Puede devolver RPC_E_CHANGED_MODE si el hilo ya fue inicializado como STA;
    // en ese caso COM ya está disponible y continuamos.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let _com_guard = ComGuard(initialized);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| error.to_string())?;
        let collection = enumerator
            .EnumAudioEndpoints(
                if capture { eCapture } else { eRender },
                DEVICE_STATE_ACTIVE,
            )
            .map_err(|error| error.to_string())?;
        let count = collection.GetCount().map_err(|error| error.to_string())?;
        let mut endpoints = Vec::with_capacity(count as usize);
        for index in 0..count {
            let device = collection.Item(index).map_err(|error| error.to_string())?;
            let id_ptr = device.GetId().map_err(|error| error.to_string())?;
            let id = id_ptr.to_string().map_err(|error| error.to_string())?;
            CoTaskMemFree(Some(id_ptr.0.cast()));

            let store = device
                .OpenPropertyStore(STGM_READ)
                .map_err(|error| error.to_string())?;
            let mut value = store
                .GetValue(&PKEY_Device_FriendlyName)
                .map_err(|error| error.to_string())?;
            let mut buffer = vec![0u16; 512];
            let converted = PropVariantToString(&value, &mut buffer);
            let _ = PropVariantClear(&mut value);
            converted.map_err(|error| error.to_string())?;
            let length = buffer
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(buffer.len());
            let name = String::from_utf16_lossy(&buffer[..length]);
            endpoints.push(Endpoint { id, name });
        }
        Ok(endpoints)
    }
}

pub fn take_matching_id(endpoints: &mut Vec<Endpoint>, name: &str) -> Option<String> {
    let index = endpoints
        .iter()
        .position(|endpoint| endpoint.name.eq_ignore_ascii_case(name))?;
    Some(endpoints.remove(index).id)
}

pub fn friendly_name(id: &str, capture: bool) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    active_endpoints(capture)
        .ok()?
        .into_iter()
        .find(|endpoint| endpoint.id == id)
        .map(|endpoint| endpoint.name)
}
