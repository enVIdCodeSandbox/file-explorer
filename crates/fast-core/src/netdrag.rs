//! A drag that offers only a list of file paths (CF_HDROP) and never touches the files.
//!
//! The system's own data object for a dragged item looks the item up (and for a folder, its contents) when the
//! drag starts or when the other program asks, on the thread that owns the window. On a network share that can
//! take minutes and freezes the whole app. A plain list of paths costs nothing; the program that receives the
//! drop reads the files from the share itself.

use std::path::PathBuf;
use windows::core::{implement, Ref, Result, BOOL, HRESULT};
use windows::Win32::Foundation::{
    DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, OLE_E_ADVISENOTSUPPORTED, S_OK,
};
use windows::Win32::System::Com::{IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::SHCreateStdEnumFmtEtc;

#[implement(IDataObject)]
struct PathList {
    paths: Vec<PathBuf>,
}

fn wanted(f: *const FORMATETC) -> bool {
    match unsafe { f.as_ref() } {
        Some(f) => f.cfFormat == CF_HDROP.0 && (f.tymed & TYMED_HGLOBAL.0 as u32) != 0 && f.dwAspect == DVASPECT_CONTENT.0 as u32,
        None => false,
    }
}

impl PathList {
    /// DROPFILES header (20 bytes: offset, point, "non-client", "wide") followed by NUL-ended UTF-16 paths and a last NUL.
    fn hglobal(&self) -> Result<HGLOBAL> {
        use std::os::windows::ffi::OsStrExt;
        let mut text: Vec<u16> = Vec::new();
        for p in &self.paths {
            text.extend(p.as_os_str().encode_wide());
            text.push(0);
        }
        text.push(0);
        let mut bytes: Vec<u8> = Vec::with_capacity(20 + text.len() * 2);
        bytes.extend(20u32.to_le_bytes());
        bytes.extend(0i32.to_le_bytes());
        bytes.extend(0i32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        for u in text {
            bytes.extend(u.to_le_bytes());
        }
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let dst = GlobalLock(h) as *mut u8;
            if dst.is_null() {
                return Err(windows::core::Error::from_hresult(HRESULT(0x8007000Eu32 as i32)));
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            let _ = GlobalUnlock(h);
            Ok(h)
        }
    }
}

#[allow(non_snake_case)]
impl IDataObject_Impl for PathList_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        if !wanted(pformatetcin) {
            return Err(windows::core::Error::from_hresult(DV_E_FORMATETC));
        }
        Ok(STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: self.hglobal()? }, pUnkForRelease: std::mem::ManuallyDrop::new(None) })
    }
    fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
        Err(windows::core::Error::from_hresult(DV_E_FORMATETC))
    }
    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        if wanted(pformatetc) {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }
    fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
        if let Some(o) = unsafe { out.as_mut() } {
            o.ptd = std::ptr::null_mut();
        }
        DATA_S_SAMEFORMATETC
    }
    fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
        Err(windows::core::Error::from_hresult(E_NOTIMPL))
    }
    fn EnumFormatEtc(&self, direction: u32) -> Result<IEnumFORMATETC> {
        if direction != 1 {
            // DATADIR_GET only
            return Err(windows::core::Error::from_hresult(E_NOTIMPL));
        }
        let f = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0 as u32, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
        unsafe { SHCreateStdEnumFmtEtc(&[f]) }
    }
    fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<'_, IAdviseSink>) -> Result<u32> {
        Err(windows::core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }
    fn DUnadvise(&self, _: u32) -> Result<()> {
        Err(windows::core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }
    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(windows::core::Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }
}

#[implement(IDropSource)]
struct Source;

#[allow(non_snake_case)]
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() || (keys & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
            // button released: drop (the other program decides whether it accepts); Escape: cancel
            if escape.as_bool() {
                DRAGDROP_S_CANCEL
            } else {
                DRAGDROP_S_DROP
            }
        } else {
            S_OK
        }
    }
    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Starts a drag of these paths from the calling thread (which must be the window's thread) and returns when it ends.
/// `true` if the paths were dropped.
pub fn drag_paths(paths: Vec<PathBuf>) -> std::result::Result<bool, String> {
    unsafe {
        let _ = OleInitialize(None);
        let data: IDataObject = PathList { paths }.into();
        let source: IDropSource = Source.into();
        let mut effect = DROPEFFECT::default();
        let r = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
        Ok(r == DRAGDROP_S_DROP)
    }
}
