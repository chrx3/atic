//! Arrastre OLE de texto plano (`CF_UNICODETEXT`).
//!
//! El plugin `tauri-plugin-drag` en Windows solo implementa `CF_HDROP` (archivo).
//! Arrastrar un `.atic-drag-*.txt` a Cursor/Notepad inserta la **ruta**, no el
//! contenido. Acá hacemos `DoDragDrop` con texto Unicode de verdad.

#![cfg(windows)]

use std::sync::Once;

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            Com::{
                IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumFORMATETC_Impl,
                IEnumSTATDATA, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0,
                TYMED_HGLOBAL,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            Ole::{
                DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, CF_UNICODETEXT,
                DROPEFFECT, DROPEFFECT_COPY,
            },
            SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        },
    },
};

static OLE_INIT: Once = Once::new();

fn ensure_ole() {
    OLE_INIT.call_once(|| {
        // SAFETY: una vez por proceso; S_FALSE (ya init) no importa.
        unsafe {
            let _ = OleInitialize(None);
        }
    });
}

fn format_etc() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_UNICODETEXT.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn text_to_hglobal(text: &str) -> Result<HGLOBAL> {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let bytes = wide.len() * std::mem::size_of::<u16>();
    // SAFETY: tamaño > 0 (al menos el NUL).
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes)? };
    // SAFETY: handle recién alocado.
    let ptr = unsafe { GlobalLock(handle) };
    if ptr.is_null() {
        return Err(Error::from_win32());
    }
    // SAFETY: destino con `bytes` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
        let _ = GlobalUnlock(handle);
    }
    Ok(handle)
}

fn hresult_err(code: HRESULT) -> Error {
    Error::from(code)
}

#[implement(IDropSource)]
struct DropSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if (grfkeystate & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
            // Soltar sobre agentes: cancelar OLE (si no, pega en la app de atrás
            // por el click-through) y el caller inserta en el composer.
            if crate::overlay::cursor_over_hit_id("agents") {
                return DRAGDROP_S_CANCEL;
            }
            return DRAGDROP_S_DROP;
        }
        S_OK
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        // Rearmar hit-rects (agentes) mientras corre el loop modal de OLE.
        crate::overlay::nudge_item_drag_arm();
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[implement(IEnumFORMATETC)]
struct FormatEnum {
    index: std::cell::Cell<u32>,
}

#[allow(non_snake_case)]
impl IEnumFORMATETC_Impl for FormatEnum_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        if rgelt.is_null() {
            return E_POINTER;
        }
        let mut fetched = 0u32;
        if self.index.get() == 0 && celt > 0 {
            // SAFETY: caller garantiza al menos un FORMATETC.
            unsafe { *rgelt = format_etc() };
            self.index.set(1);
            fetched = 1;
        }
        if !pceltfetched.is_null() {
            // SAFETY: opcional writable.
            unsafe { *pceltfetched = fetched };
        }
        if fetched == celt {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        self.index.set(self.index.get().saturating_add(celt).min(1));
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumFORMATETC> {
        Ok(FormatEnum {
            index: std::cell::Cell::new(self.index.get()),
        }
        .into())
    }
}

#[implement(IDataObject)]
struct TextDataObject {
    text: String,
}

impl TextDataObject {
    fn matches(pformatetc: *const FORMATETC) -> bool {
        let Some(fe) = (unsafe { pformatetc.as_ref() }) else {
            return false;
        };
        fe.cfFormat == CF_UNICODETEXT.0
            && (fe.tymed & (TYMED_HGLOBAL.0 as u32)) != 0
            && fe.dwAspect == DVASPECT_CONTENT.0
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for TextDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        if !TextDataObject::matches(pformatetcin) {
            return Err(hresult_err(DV_E_FORMATETC));
        }
        let handle = text_to_hglobal(&self.text)?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: handle },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        Err(hresult_err(E_NOTIMPL))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        if TextDataObject::matches(pformatetc) {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        if !pformatetcout.is_null() {
            unsafe { (*pformatetcout).ptd = std::ptr::null_mut() };
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> Result<()> {
        Err(hresult_err(E_NOTIMPL))
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(hresult_err(E_NOTIMPL));
        }
        Ok(FormatEnum {
            index: std::cell::Cell::new(0),
        }
        .into())
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: Ref<'_, IAdviseSink>,
    ) -> Result<u32> {
        Err(hresult_err(OLE_E_ADVISENOTSUPPORTED))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(hresult_err(OLE_E_ADVISENOTSUPPORTED))
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(hresult_err(OLE_E_ADVISENOTSUPPORTED))
    }
}

/// Bloquea el hilo hasta soltar: llamar desde el hilo UI.
///
/// Devuelve el `DROPEFFECT` final (`0` = none / cancel sin drop útil).
pub fn drag_unicode_text(text: &str) -> std::result::Result<u32, String> {
    if text.is_empty() {
        return Err("texto vacío".into());
    }
    ensure_ole();

    let data: IDataObject = TextDataObject {
        text: text.to_string(),
    }
    .into();
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT::default();

    // SAFETY: data/source vivos durante DoDragDrop.
    let hr = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    if hr == DRAGDROP_S_DROP || hr == DRAGDROP_S_CANCEL || hr == S_OK {
        Ok(effect.0)
    } else {
        Err(format!("DoDragDrop falló: {hr:?}"))
    }
}
