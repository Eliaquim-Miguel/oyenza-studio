//! OLE drag-and-drop: files dropped onto the island go to the Shelf, and
//! shelf items can be dragged back out to Explorer, mail, chat apps, …
//!
//! Both directions run on the UI thread (an OLE STA initialised by app.rs):
//! OLE calls the drop target from the message loop, and [`drag_out`] runs the
//! modal `DoDragDrop` loop from a deferred command.

use crate::events::{AppEvent, EventSink};
use poni_core::shelf::MAX_SHELF_ITEMS;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::core::{implement, Interface, Result, HRESULT, HSTRING};
use windows::Win32::Foundation::{
    BOOL, DRAGDROP_E_NOTREGISTERED, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, POINT, POINTL, S_OK,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IDataObject, CLSCTX_INPROC_SERVER, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP,
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DROPEFFECT_NONE, MK_ALT,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MK_RBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DragQueryFileW, IDragSourceHelper, IDragSourceHelper2, IDropTargetHelper, IShellItemArray, SHCreateShellItemArrayFromIDLists,
    SHParseDisplayName, BHID_DataObject, CLSID_DragDropHelper, DSH_ALLOWDROPDESCRIPTIONTEXT, HDROP,
};

/// True while [`drag_out`] runs its modal loop (the island refuses its own
/// drags, and the Shelf page re-checks the dragged file afterwards).
static DRAGGING_OUT: AtomicBool = AtomicBool::new(false);

pub fn is_dragging_out() -> bool {
    DRAGGING_OUT.load(Ordering::Relaxed)
}

/// The registered island window (asked for a drag image; it answers
/// "not handled", so the shell draws its standard one for the items).
static ISLAND: AtomicIsize = AtomicIsize::new(0);

fn hdrop_format() -> FORMATETC {
    FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
}

fn point(pt: &POINTL) -> POINT {
    POINT { x: pt.x, y: pt.y }
}

// ---------------------------------------------------------------- drop target

#[implement(IDropTarget)]
struct DropTarget {
    hwnd: HWND,
    sink: EventSink,
    /// Shell helper that draws the dragged items' image over our window.
    helper: Option<IDropTargetHelper>,
    /// The current drag carries files we can take.
    accept: Cell<bool>,
    /// We told the app a drag is hovering (so DragLeave/Drop must undo it).
    hovering: Cell<bool>,
}

impl DropTarget {
    /// Copy by default, link with Alt, within what the source allows. Never
    /// "move": the shelf only keeps references, a move would lose the file.
    fn effect(&self, keys: MODIFIERKEYS_FLAGS, allowed: DROPEFFECT) -> DROPEFFECT {
        if !self.accept.get() {
            return DROPEFFECT_NONE;
        }
        let want = if keys.0 & MK_ALT != 0 { DROPEFFECT_LINK } else { DROPEFFECT_COPY };
        [want, DROPEFFECT_COPY, DROPEFFECT_LINK].into_iter().find(|e| allowed.0 & e.0 != 0).unwrap_or(DROPEFFECT_NONE)
    }

    fn set_hover(&self, on: bool) {
        if self.hovering.replace(on) != on {
            self.sink.send(AppEvent::DragHover(on));
        }
    }
}

/// Read and write the in/out `pdweffect` parameter.
fn allowed_effects(p: *mut DROPEFFECT) -> DROPEFFECT {
    if p.is_null() {
        DROPEFFECT_COPY | DROPEFFECT_LINK
    } else {
        unsafe { *p }
    }
}

fn set_effect(p: *mut DROPEFFECT, e: DROPEFFECT) {
    if !p.is_null() {
        unsafe { *p = e };
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, pdataobj: Option<&IDataObject>, grfkeystate: MODIFIERKEYS_FLAGS, pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
        let has_files = pdataobj.is_some_and(|d| unsafe { d.QueryGetData(&hdrop_format()) } == S_OK);
        self.accept.set(has_files && !is_dragging_out());
        let effect = self.effect(grfkeystate, allowed_effects(pdweffect));
        set_effect(pdweffect, effect);
        if let (Some(h), Some(d)) = (&self.helper, pdataobj) {
            unsafe {
                let _ = h.DragEnter(self.hwnd, d, &point(pt), effect);
            }
        }
        if self.accept.get() {
            self.set_hover(true);
        }
        Ok(())
    }

    fn DragOver(&self, grfkeystate: MODIFIERKEYS_FLAGS, pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
        let effect = self.effect(grfkeystate, allowed_effects(pdweffect));
        set_effect(pdweffect, effect);
        if let Some(h) = &self.helper {
            unsafe {
                let _ = h.DragOver(&point(pt), effect);
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if let Some(h) = &self.helper {
            unsafe {
                let _ = h.DragLeave();
            }
        }
        self.accept.set(false);
        self.set_hover(false);
        Ok(())
    }

    fn Drop(&self, pdataobj: Option<&IDataObject>, grfkeystate: MODIFIERKEYS_FLAGS, pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
        let mut effect = self.effect(grfkeystate, allowed_effects(pdweffect));
        let mut paths = Vec::new();
        if effect != DROPEFFECT_NONE {
            if let Some(d) = pdataobj {
                match unsafe { dropped_paths(d) } {
                    Ok(p) => paths = p,
                    Err(e) => crate::pwarn!("drop: reading file list failed: {e}"),
                }
            }
        }
        if paths.is_empty() {
            effect = DROPEFFECT_NONE;
        }
        set_effect(pdweffect, effect);
        if let (Some(h), Some(d)) = (&self.helper, pdataobj) {
            unsafe {
                let _ = h.Drop(d, &point(pt), effect);
            }
        }
        self.accept.set(false);
        if paths.is_empty() {
            self.set_hover(false);
        } else {
            // ShelfAdd also ends the hover state on the app side.
            self.hovering.set(false);
            self.sink.send(AppEvent::ShelfAdd(paths));
        }
        Ok(())
    }
}

/// All file system paths in a CF_HDROP payload, in the order the shelf should
/// show them (the shelf inserts newest-first, so the list is reversed).
unsafe fn dropped_paths(data: &IDataObject) -> Result<Vec<String>> {
    let mut medium = data.GetData(&hdrop_format())?;
    let mut out = Vec::new();
    if medium.tymed == TYMED_HGLOBAL.0 as u32 {
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None);
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
            if n > 0 {
                out.push(String::from_utf16_lossy(&buf[..n.min(len as usize)]));
            }
            if out.len() >= MAX_SHELF_ITEMS {
                break;
            }
        }
    }
    ReleaseStgMedium(&mut medium);
    out.reverse();
    Ok(out)
}

/// Make the island a drop target for files.
pub fn register(hwnd: HWND, sink: EventSink) -> windows::core::Result<()> {
    let helper: Option<IDropTargetHelper> = match unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) } {
        Ok(h) => Some(h),
        Err(e) => {
            crate::pinfo!("drop target: no drag image helper ({e})");
            None
        }
    };
    let target: IDropTarget = DropTarget { hwnd, sink, helper, accept: Cell::new(false), hovering: Cell::new(false) }.into();
    // OLE keeps its own reference until RevokeDragDrop.
    unsafe { RegisterDragDrop(hwnd, &target)? };
    ISLAND.store(hwnd.0 as isize, Ordering::Relaxed);
    Ok(())
}

pub fn revoke(hwnd: HWND) {
    let _ = ISLAND.compare_exchange(hwnd.0 as isize, 0, Ordering::Relaxed, Ordering::Relaxed);
    if let Err(e) = unsafe { RevokeDragDrop(hwnd) } {
        if e.code() != DRAGDROP_E_NOTREGISTERED {
            crate::pwarn!("RevokeDragDrop failed: {e}");
        }
    }
}

// ---------------------------------------------------------------- drag out

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() || grfkeystate.contains(MK_RBUTTON) {
            DRAGDROP_S_CANCEL
        } else if !grfkeystate.contains(MK_LBUTTON) {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// PIDLs from `SHParseDisplayName`, freed with `CoTaskMemFree` on drop.
struct Pidls(Vec<*mut ITEMIDLIST>);

impl Drop for Pidls {
    fn drop(&mut self) {
        for p in self.0.drain(..) {
            unsafe { CoTaskMemFree(Some(p as *const _)) };
        }
    }
}

/// Start a modal OLE drag of `paths` (copy/move/link).
pub fn drag_out(paths: &[String]) -> windows::core::Result<()> {
    unsafe {
        let mut pidls = Pidls(Vec::with_capacity(paths.len()));
        for p in paths {
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            // Fails for files that were moved or deleted: skip those.
            match SHParseDisplayName(&HSTRING::from(p.as_str()), None, &mut pidl, 0, None) {
                Ok(()) if !pidl.is_null() => pidls.0.push(pidl),
                Ok(()) => {}
                Err(e) => crate::pinfo!("drag out: skipping {} ({e})", poni_core::shelf::display_name(p)),
            }
        }
        if pidls.0.is_empty() {
            return Ok(());
        }
        let list: Vec<*const ITEMIDLIST> = pidls.0.iter().map(|p| *p as *const ITEMIDLIST).collect();
        let items: IShellItemArray = SHCreateShellItemArrayFromIDLists(&list)?;
        drop(pidls);
        let data: IDataObject = items.BindToHandler(None, &BHID_DataObject)?;

        // The standard shell drag image (thumbnail/icon + count) for the
        // items, plus "Copy to …"/"Move to …" labels from drop targets.
        match CoCreateInstance::<_, IDragSourceHelper>(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) {
            Ok(helper) => {
                if let Ok(h2) = helper.cast::<IDragSourceHelper2>() {
                    let _ = h2.SetFlags(DSH_ALLOWDROPDESCRIPTIONTEXT.0 as u32);
                }
                let hwnd = HWND(ISLAND.load(Ordering::Relaxed) as *mut core::ffi::c_void);
                if let Err(e) = helper.InitializeFromWindow(hwnd, None, &data) {
                    crate::pinfo!("drag out: no drag image ({e})");
                }
            }
            Err(e) => crate::pinfo!("drag out: no drag image helper ({e})"),
        }

        let source: IDropSource = DropSource.into();
        let mut effect = DROPEFFECT_NONE;
        let was = DRAGGING_OUT.swap(true, Ordering::Relaxed);
        // DRAGDROP_S_DROP / DRAGDROP_S_CANCEL are success codes.
        let hr = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK, &mut effect);
        DRAGGING_OUT.store(was, Ordering::Relaxed);
        hr.ok()
    }
}
