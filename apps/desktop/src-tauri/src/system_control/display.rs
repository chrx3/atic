//! Monitores y brillo. Si el SO no expone brillo, `brightness` queda `None`.

use super::SystemDisplay;

pub fn list() -> Result<Vec<SystemDisplay>, String> {
    imp::list()
}

pub fn set_brightness(id: &str, brightness: f32) -> Result<(), String> {
    imp::set_brightness(id, brightness)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::mem::size_of;
    use windows_sys::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW,
        DISPLAY_DEVICE_PRIMARY_DEVICE, HDC, HMONITOR, MONITORINFOEXW,
    };

    struct Collect {
        out: Vec<SystemDisplay>,
    }

    pub fn list() -> Result<Vec<SystemDisplay>, String> {
        let mut collect = Collect { out: Vec::new() };
        let ok = unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(enum_mon),
                &mut collect as *mut Collect as LPARAM,
            )
        };
        if ok == 0 {
            return Err("no se pudieron listar las pantallas".into());
        }
        Ok(collect.out)
    }

    pub fn set_brightness(id: &str, brightness: f32) -> Result<(), String> {
        let hwnd = parse_hmonitor(id)?;
        set_hmonitor_brightness(hwnd, brightness)
    }

    fn parse_hmonitor(id: &str) -> Result<HMONITOR, String> {
        let n: isize = id.parse().map_err(|_| "pantalla desconocida".to_string())?;
        Ok(n as HMONITOR)
    }

    unsafe extern "system" fn enum_mon(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let collect = &mut *(data as *mut Collect);
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut _) == 0 {
            return TRUE;
        }
        let device = wchar_to_string(&info.szDevice);
        let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
        dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        let name = if EnumDisplayDevicesW(info.szDevice.as_ptr(), 0, &mut dd, 0) != 0 {
            wchar_to_string(&dd.DeviceString)
        } else {
            device.clone()
        };
        let primary = (info.monitorInfo.dwFlags & 1) != 0
            || (dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE) != 0;
        collect.out.push(SystemDisplay {
            id: (monitor as isize).to_string(),
            name: if name.is_empty() { device } else { name },
            primary,
            brightness: hmonitor_brightness(monitor),
        });
        TRUE
    }

    fn hmonitor_brightness(monitor: HMONITOR) -> Option<f32> {
        unsafe {
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count) == 0 || count == 0 {
                return None;
            }
            let mut phys = vec![std::mem::zeroed::<PHYSICAL_MONITOR>(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(monitor, count, phys.as_mut_ptr()) == 0 {
                return None;
            }
            let mut min = 0u32;
            let mut cur = 0u32;
            let mut max = 0u32;
            let ok = GetMonitorBrightness(phys[0].hPhysicalMonitor, &mut min, &mut cur, &mut max);
            let _ = DestroyPhysicalMonitors(count, phys.as_mut_ptr());
            if ok == 0 || max <= min {
                return None;
            }
            Some(((cur.saturating_sub(min)) as f32) / ((max - min) as f32))
        }
    }

    fn set_hmonitor_brightness(monitor: HMONITOR, brightness: f32) -> Result<(), String> {
        unsafe {
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count) == 0 || count == 0 {
                return Err("este monitor no deja cambiar el brillo".into());
            }
            let mut phys = vec![std::mem::zeroed::<PHYSICAL_MONITOR>(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(monitor, count, phys.as_mut_ptr()) == 0 {
                return Err("este monitor no deja cambiar el brillo".into());
            }
            let mut min = 0u32;
            let mut cur = 0u32;
            let mut max = 0u32;
            let ok = GetMonitorBrightness(phys[0].hPhysicalMonitor, &mut min, &mut cur, &mut max);
            if ok == 0 || max <= min {
                let _ = DestroyPhysicalMonitors(count, phys.as_mut_ptr());
                return Err("este monitor no deja cambiar el brillo".into());
            }
            let value = min + ((max - min) as f32 * brightness).round() as u32;
            let set = SetMonitorBrightness(phys[0].hPhysicalMonitor, value.clamp(min, max));
            let _ = DestroyPhysicalMonitors(count, phys.as_mut_ptr());
            if set == 0 {
                return Err("no se pudo cambiar el brillo".into());
            }
            Ok(())
        }
    }

    fn wchar_to_string(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSString};
    use std::ffi::CString;
    use std::os::raw::{c_int, c_void};

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {}

    const RTLD_LAZY: i32 = 1;

    extern "C" {
        fn dlopen(filename: *const i8, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
        fn CGMainDisplayID() -> u32;
    }

    type GetBright = unsafe extern "C" fn(u32, *mut f32) -> c_int;
    type SetBright = unsafe extern "C" fn(u32, f32) -> c_int;
    type CoreGet = unsafe extern "C" fn(u32) -> f64;
    type CoreSet = unsafe extern "C" fn(u32, f64);

    fn nsstring_to_string(value: *mut AnyObject) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let value: &NSString = unsafe { &*value.cast() };
        Some(value.to_string())
    }

    fn display_services() -> (*mut c_void, Option<GetBright>, Option<SetBright>) {
        let path = CString::new(
            "/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices",
        )
        .ok();
        let Some(path) = path else {
            return (std::ptr::null_mut(), None, None);
        };
        unsafe {
            let handle = dlopen(path.as_ptr(), RTLD_LAZY);
            if handle.is_null() {
                return (std::ptr::null_mut(), None, None);
            }
            let get_n = CString::new("DisplayServicesGetBrightness").unwrap();
            let set_n = CString::new("DisplayServicesSetBrightness").unwrap();
            let get = dlsym(handle, get_n.as_ptr());
            let set = dlsym(handle, set_n.as_ptr());
            (
                handle,
                if get.is_null() {
                    None
                } else {
                    Some(std::mem::transmute(get))
                },
                if set.is_null() {
                    None
                } else {
                    Some(std::mem::transmute(set))
                },
            )
        }
    }

    fn core_display() -> (Option<CoreGet>, Option<CoreSet>) {
        let path =
            CString::new("/System/Library/Frameworks/CoreDisplay.framework/CoreDisplay").ok();
        let Some(path) = path else {
            return (None, None);
        };
        unsafe {
            let handle = dlopen(path.as_ptr(), RTLD_LAZY);
            if handle.is_null() {
                return (None, None);
            }
            let get_n = CString::new("CoreDisplay_Display_GetUserBrightness").unwrap();
            let set_n = CString::new("CoreDisplay_Display_SetUserBrightness").unwrap();
            let get = dlsym(handle, get_n.as_ptr());
            let set = dlsym(handle, set_n.as_ptr());
            (
                if get.is_null() {
                    None
                } else {
                    Some(std::mem::transmute(get))
                },
                if set.is_null() {
                    None
                } else {
                    Some(std::mem::transmute(set))
                },
            )
        }
    }

    fn brightness_of(id: u32) -> Option<f32> {
        let (_, get, _) = display_services();
        if let Some(get) = get {
            let mut value = 0f32;
            if unsafe { get(id, &mut value) } == 0 {
                return Some(value.clamp(0.0, 1.0));
            }
        }
        let (get, _) = core_display();
        if let Some(get) = get {
            let value = unsafe { get(id) };
            if value.is_finite() {
                return Some((value as f32).clamp(0.0, 1.0));
            }
        }
        None
    }

    pub fn list() -> Result<Vec<SystemDisplay>, String> {
        autoreleasepool(|_| unsafe {
            let screens: *mut AnyObject = objc2::msg_send![objc2::class!(NSScreen), screens];
            if screens.is_null() {
                return Err("no se pudieron listar las pantallas".into());
            }
            let screens: &NSArray<AnyObject> = &*screens.cast();
            let main_id = CGMainDisplayID();
            let mut out = Vec::new();
            for (i, screen) in screens.iter().enumerate() {
                let desc: *mut AnyObject = objc2::msg_send![&*screen, deviceDescription];
                let mut display_id = if i == 0 { main_id } else { 0u32 };
                if !desc.is_null() {
                    let key = NSString::from_str("NSScreenNumber");
                    let num: *mut AnyObject = objc2::msg_send![desc, objectForKey: &*key];
                    if !num.is_null() {
                        let n: u32 = objc2::msg_send![num, unsignedIntValue];
                        display_id = n;
                    }
                }
                let name = nsstring_to_string(objc2::msg_send![&*screen, localizedName])
                    .unwrap_or_else(|| format!("Pantalla {}", i + 1));
                out.push(SystemDisplay {
                    id: display_id.to_string(),
                    name,
                    primary: display_id == main_id,
                    brightness: brightness_of(display_id),
                });
            }
            Ok(out)
        })
    }

    pub fn set_brightness(id: &str, brightness: f32) -> Result<(), String> {
        let display_id: u32 = id.parse().map_err(|_| "pantalla desconocida".to_string())?;
        let (_, _, set) = display_services();
        if let Some(set) = set {
            if unsafe { set(display_id, brightness) } == 0 {
                return Ok(());
            }
        }
        let (_, set) = core_display();
        if let Some(set) = set {
            unsafe { set(display_id, brightness as f64) };
            return Ok(());
        }
        Err("este monitor no deja cambiar el brillo".into())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;
    pub fn list() -> Result<Vec<SystemDisplay>, String> {
        Err("no soportado en esta plataforma todavía".into())
    }
    pub fn set_brightness(_: &str, _: f32) -> Result<(), String> {
        Err("no soportado".into())
    }
}
