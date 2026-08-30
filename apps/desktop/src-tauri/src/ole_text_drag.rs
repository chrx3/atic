//! Arrastre OLE propio: texto plano y archivos.
//!
//! El plugin `tauri-plugin-drag` en Windows solo implementa `CF_HDROP` (archivo).
//! Arrastrar un `.atic-drag-*.txt` a Cursor/Notepad inserta la **ruta**, no el
//! contenido. Acá hacemos `DoDragDrop` con texto de verdad.
//!
//! Los archivos también pasaron a este camino, aunque el plugin sí sabía
//! hacerlos: lo que el plugin no da es **cancelar** el arrastre. Ver `Payload`.
//!
//! # Por qué no alcanza con `CF_UNICODETEXT`
//!
//! El portapapeles SINTETIZA formatos: si copiás `CF_UNICODETEXT` y la app pide
//! `CF_TEXT`, Windows lo convierte solo. **El arrastre OLE no hace eso.** Acá el
//! `IDataObject` es la única fuente, así que un target que pide `CF_TEXT` —y hay
//! varios que lo piden primero, o que solo entienden eso— recibía
//! `DV_E_FORMATETC` y rechazaba el drop: cursor de "prohibido" y no pasaba nada.
//! Es la razón de que el arrastre de texto anduviera en unas apps y en otras no,
//! mientras la imagen (`CF_HDROP`, que entiende todo el mundo) andaba siempre.
//!
//! `CF_LOCALE` acompaña a los ANSI: es el que le dice al target en qué página de
//! códigos leer el `CF_TEXT`. Sin él, un acento puede llegar cambiado.

#![cfg(windows)]

use std::sync::Once;

use windows_sys::Win32::{Foundation::POINT as SysPoint, UI::Shell::DROPFILES as DropFiles};

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Globalization::{GetUserDefaultLCID, WideCharToMultiByte, CP_ACP, CP_OEMCP},
        System::{
            Com::{
                IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumFORMATETC_Impl,
                IEnumSTATDATA, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0,
                TYMED_HGLOBAL,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            Ole::{
                DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, CF_HDROP, CF_LOCALE,
                CF_OEMTEXT, CF_TEXT, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY,
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

/// Qué se está arrastrando.
///
/// El archivo también pasa por acá —y no por `tauri-plugin-drag`— por el
/// `QueryContinueDrag` de abajo: soltar sobre agentes tiene que CANCELAR el
/// OLE. Con el plugin no hay forma de cancelar, y como el overlay es
/// click-through durante el arrastre, el `CF_HDROP` caía además en la app de
/// atrás: la misma imagen entraba dos veces.
enum Payload {
    Text(String),
    Files(Vec<String>),
}

impl Payload {
    /// Lo que ofrecemos, EN ORDEN DE PREFERENCIA.
    ///
    /// `EnumFormatEtc` los enumera en este orden y un target que recorre la
    /// lista se queda con el primero que entiende: Unicode antes que ANSI,
    /// siempre.
    fn formats(&self) -> Vec<u16> {
        match self {
            Payload::Text(_) => vec![CF_UNICODETEXT.0, CF_TEXT.0, CF_OEMTEXT.0, CF_LOCALE.0],
            Payload::Files(_) => vec![CF_HDROP.0],
        }
    }

    /// El bloque de memoria que le toca a cada formato.
    fn hglobal(&self, cf: u16) -> Result<HGLOBAL> {
        match self {
            Payload::Text(text) => hglobal_for(text, cf),
            Payload::Files(paths) => bytes_to_hglobal(&hdrop_bytes(paths)),
        }
    }
}

fn format_etc(cf: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Cuál de los formatos que ofrecemos está pidiendo el target, si alguno.
fn requested_format(pformatetc: *const FORMATETC, formats: &[u16]) -> Option<u16> {
    // SAFETY: puntero del caller; `as_ref` ya cubre el nulo.
    let fe = unsafe { pformatetc.as_ref() }?;
    if (fe.tymed & (TYMED_HGLOBAL.0 as u32)) == 0 || fe.dwAspect != DVASPECT_CONTENT.0 {
        return None;
    }
    formats.iter().copied().find(|cf| *cf == fe.cfFormat)
}

fn bytes_to_hglobal(bytes: &[u8]) -> Result<HGLOBAL> {
    // SAFETY: tamaño > 0 (al menos el NUL, o los 4 del LCID).
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len())? };
    // SAFETY: handle recién alocado.
    let ptr = unsafe { GlobalLock(handle) };
    if ptr.is_null() {
        return Err(Error::from_win32());
    }
    // SAFETY: destino con `bytes.len()` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        let _ = GlobalUnlock(handle);
    }
    Ok(handle)
}

/// UTF-16 → página de códigos de 8 bits. `wide` trae el NUL, así que la salida
/// también lo lleva.
///
/// Lo que no entra en la página (un emoji en CP-1252) sale como `?`. Es lo mismo
/// que hace cualquier otra fuente de arrastre, y solo lo ve un target que pidió
/// ANSI pudiendo haber pedido Unicode.
fn wide_to_codepage(wide: &[u16], codepage: u32) -> Vec<u8> {
    // SAFETY: consulta de tamaño (destino `None`); no escribe nada.
    let len = unsafe { WideCharToMultiByte(codepage, 0, wide, None, PCSTR::null(), None) };
    if len <= 0 {
        return vec![0];
    }
    let mut out = vec![0u8; len as usize];
    // SAFETY: `out` mide exactamente lo que pidió la consulta de arriba.
    let written =
        unsafe { WideCharToMultiByte(codepage, 0, wide, Some(&mut out), PCSTR::null(), None) };
    if written <= 0 {
        return vec![0];
    }
    out.truncate(written as usize);
    out
}

/// El bloque de memoria que le toca a cada formato de texto.
fn hglobal_for(text: &str, cf: u16) -> Result<HGLOBAL> {
    if cf == CF_LOCALE.0 {
        // SAFETY: sin parámetros ni punteros.
        let lcid = unsafe { GetUserDefaultLCID() };
        return bytes_to_hglobal(&lcid.to_ne_bytes());
    }

    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);

    if cf == CF_UNICODETEXT.0 {
        let bytes = wide.len() * std::mem::size_of::<u16>();
        // SAFETY: `wide` está vivo y se relee como bytes para copiarlo tal cual.
        let raw = unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, bytes) };
        return bytes_to_hglobal(raw);
    }

    let codepage = if cf == CF_OEMTEXT.0 { CP_OEMCP } else { CP_ACP };
    bytes_to_hglobal(&wide_to_codepage(&wide, codepage))
}

/// `CF_HDROP`: cabecera `DROPFILES` y detrás las rutas en UTF-16, cada una con
/// su NUL y un NUL extra que cierra la lista.
///
/// La cabecera sale de `windows-sys` (que ya trae `Win32_UI_Shell`) en vez de
/// `windows`, para no sumarle ese módulo entero al build por una struct de
/// datos: acá solo se necesita su layout.
fn hdrop_bytes(paths: &[String]) -> Vec<u8> {
    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        list.extend(path.encode_utf16());
        list.push(0);
    }
    list.push(0);

    let header = DropFiles {
        pFiles: std::mem::size_of::<DropFiles>() as u32,
        pt: SysPoint { x: 0, y: 0 },
        fNC: 0,
        fWide: 1,
    };
    let head = std::mem::size_of::<DropFiles>();
    let mut bytes = Vec::with_capacity(head + list.len() * 2);
    // SAFETY: `header` es POD `#[repr(C)]`; se relee como bytes para copiarlo.
    bytes.extend_from_slice(unsafe {
        std::slice::from_raw_parts((&header as *const DropFiles) as *const u8, head)
    });
    // SAFETY: `list` sigue viva y se relee como bytes.
    bytes.extend_from_slice(unsafe {
        std::slice::from_raw_parts(list.as_ptr() as *const u8, list.len() * 2)
    });
    bytes
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
    formats: Vec<u16>,
    index: std::cell::Cell<u32>,
}

#[allow(non_snake_case)]
impl IEnumFORMATETC_Impl for FormatEnum_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        if rgelt.is_null() {
            return E_POINTER;
        }
        let formats = &self.formats;
        let mut fetched = 0u32;
        while fetched < celt {
            let i = self.index.get() as usize;
            if i >= formats.len() {
                break;
            }
            // SAFETY: el caller promete sitio para `celt` y no pasamos de ahí.
            unsafe { *rgelt.add(fetched as usize) = format_etc(formats[i]) };
            self.index.set(self.index.get() + 1);
            fetched += 1;
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
        let total = self.formats.len() as u32;
        self.index
            .set(self.index.get().saturating_add(celt).min(total));
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumFORMATETC> {
        Ok(FormatEnum {
            formats: self.formats.clone(),
            index: std::cell::Cell::new(self.index.get()),
        }
        .into())
    }
}

#[implement(IDataObject)]
struct DragDataObject {
    payload: Payload,
}

#[allow(non_snake_case)]
impl IDataObject_Impl for DragDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        let Some(cf) = requested_format(pformatetcin, &self.payload.formats()) else {
            return Err(hresult_err(DV_E_FORMATETC));
        };
        let handle = self.payload.hglobal(cf)?;
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
        if requested_format(pformatetc, &self.payload.formats()).is_some() {
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
            formats: self.payload.formats(),
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

/// Cómo terminó el arrastre.
///
/// La distinción importa y es la base del plan B: `dropped: true, effect: 0`
/// significa "lo soltaste sobre algo que no acepta texto OLE" —una consola, un
/// terminal Electron— y ahí conviene pegar. Un Esc, en cambio, deja
/// `dropped: false`: cancelaste, y pegar sería meter texto en una ventana que
/// nadie eligió.
pub struct DragOutcome {
    /// Hubo un drop de verdad (no un Esc ni un cancel).
    pub dropped: bool,
    /// `DROPEFFECT` final. `0` = el target no se lo quedó.
    pub effect: u32,
}

/// Bloquea el hilo hasta soltar: llamar desde el hilo UI.
pub fn drag_unicode_text(text: &str) -> std::result::Result<DragOutcome, String> {
    if text.is_empty() {
        return Err("texto vacío".into());
    }
    run_drag(Payload::Text(text.to_string()))
}

/// Arrastra archivos como `CF_HDROP`. Bloquea el hilo hasta soltar.
pub fn drag_files(paths: &[String]) -> std::result::Result<DragOutcome, String> {
    if paths.is_empty() {
        return Err("sin archivos que arrastrar".into());
    }
    run_drag(Payload::Files(paths.to_vec()))
}

fn run_drag(payload: Payload) -> std::result::Result<DragOutcome, String> {
    ensure_ole();

    let data: IDataObject = DragDataObject { payload }.into();
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT::default();

    // SAFETY: data/source vivos durante DoDragDrop.
    let hr = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    if hr == DRAGDROP_S_DROP || hr == DRAGDROP_S_CANCEL || hr == S_OK {
        Ok(DragOutcome {
            dropped: hr == DRAGDROP_S_DROP,
            effect: effect.0,
        })
    } else {
        Err(format!("DoDragDrop falló: {hr:?}"))
    }
}
