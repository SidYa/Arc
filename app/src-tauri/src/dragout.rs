//! Dragging entries out of an archive to Explorer, the way 7-Zip does it: the drag starts at once
//! with empty placeholders in a temporary folder, and the entries are extracted only when the
//! mouse button is released over a target that accepts them (in IDropSource::QueryContinueDrag,
//! before the drop is delivered). While extracting, the window keeps pumping messages, so Arc
//! stays responsive and shows the progress; Esc cancels.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, POINT, S_OK};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropSource, IDropSource_Impl, OleInitialize};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{BHID_DataObject, ILCreateFromPathW, ILFree, SHCreateShellItemArrayFromIDLists, SHDoDragDrop};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GA_ROOT, GetAncestor, GetCursorPos, GetWindowThreadProcessId, MSG, MsgWaitForMultipleObjects, PEEK_MESSAGE_REMOVE_TYPE,
    PM_QS_PAINT, PM_QS_POSTMESSAGE, PM_QS_SENDMESSAGE, PM_REMOVE, PeekMessageW, QS_PAINT, QS_POSTMESSAGE, QS_SENDMESSAGE, QS_TIMER,
    TranslateMessage, WindowFromPoint,
};
use windows::core::{BOOL, HRESULT, HSTRING, PCWSTR, implement};

use crate::archive::{self, Conflict, ExtractOptions, Progress};

pub struct DragRequest {
    pub archive: PathBuf,
    pub password: Option<String>,
    pub items: Vec<String>,
    /// Which items are folders (same order as `items`)
    pub dirs: Vec<bool>,
    pub base: String,
    pub expected_files: Option<usize>,
}

/// Runs the whole drag on the UI thread. "dropped" | "cancelled", or the extraction error.
pub fn run(app: &AppHandle, window: &WebviewWindow, id: u32, req: DragRequest) -> Result<String, String> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dest = crate::config::temp_dir().join(format!("drag-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    // Placeholders: Explorer sees real files and folders with the right names and icons
    let mut paths = Vec::new();
    for (item, is_dir) in req.items.iter().zip(req.dirs.iter().copied().chain(std::iter::repeat(false))) {
        let relative = if req.base.is_empty() { item.as_str() } else { item.strip_prefix(&req.base).unwrap_or(item).trim_start_matches('/') };
        let Some(path) = archive::safe_join(&dest, relative) else { continue };
        let made = if is_dir { std::fs::create_dir_all(&path) } else { std::fs::File::create(&path).map(drop) };
        if made.is_ok() {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err("not_found".into());
    }

    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0 as _);
    let data = data_object(&paths).ok_or("drag_failed")?;

    let app = app.clone();
    let temp = dest.clone();
    let extract = Box::new(move || extract_with_pump(&app, id, &req, &dest));
    let state = Rc::new(DragState { busy: Cell::new(false), effect: Cell::new(DROPEFFECT_NONE), extract: RefCell::new(Some(extract)), result: RefCell::new(None) });
    let source: IDropSource = DropSource(state.clone()).into();
    unsafe {
        let _ = OleInitialize(None);
        // Copy or move: the files are temporary, so on the same disk Explorer just moves them (instant)
        let result = SHDoDragDrop(Some(hwnd), &data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE);
        drop(source);
        // The drop goes ahead only once the extraction has succeeded
        let outcome = state.result.borrow_mut().take();
        let dropped = result.is_ok() && matches!(outcome, Some(Ok(())));
        if !dropped {
            // Explorer copies after the drop, so dropped files stay until the next start
            let _ = std::fs::remove_dir_all(&temp);
        }
        match (result, outcome) {
            (_, Some(Err(e))) => Err(e),
            _ if dropped => Ok("dropped".into()),
            (Err(e), _) => Err(e.to_string()),
            _ => Ok("cancelled".into()),
        }
    }
}

/// The shell's own data object for the files: Explorer gets everything it wants from it
/// (file list, item IDs, drag image with the file icons).
fn data_object(paths: &[PathBuf]) -> Option<IDataObject> {
    unsafe {
        let ids: Vec<*const ITEMIDLIST> = paths
            .iter()
            .map(|p| ILCreateFromPathW(PCWSTR(HSTRING::from(p.as_os_str()).as_ptr())) as *const ITEMIDLIST)
            .filter(|p| !p.is_null())
            .collect();
        let array = SHCreateShellItemArrayFromIDLists(&ids);
        for id in &ids {
            ILFree(Some(*id));
        }
        array.ok()?.BindToHandler(None, &BHID_DataObject).ok()
    }
}

type Extract = Box<dyn FnOnce() -> Result<(), String>>;

#[implement(IDropSource)]
struct DropSource(Rc<DragState>);

struct DragState {
    /// Extracting right now
    busy: Cell<bool>,
    /// The last effect the target under the cursor reported
    effect: Cell<DROPEFFECT>,
    extract: RefCell<Option<Extract>>,
    result: RefCell<Option<Result<(), String>>>,
}

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        // Called again while extracting (from a nested message loop): just keep waiting
        if self.0.busy.get() {
            return S_OK;
        }
        if escape.as_bool() {
            return DRAGDROP_S_CANCEL;
        }
        if keys.0 & MK_LBUTTON.0 != 0 {
            return S_OK;
        }
        // Released: nothing accepts the files here, or it's Arc's own window
        if self.0.effect.get() == DROPEFFECT_NONE || over_own_window() {
            return DRAGDROP_S_CANCEL;
        }
        let Some(extract) = self.0.extract.borrow_mut().take() else { return DRAGDROP_S_CANCEL };
        self.0.busy.set(true);
        let result = extract();
        self.0.busy.set(false);
        let ok = result.is_ok();
        *self.0.result.borrow_mut() = Some(result);
        if ok { DRAGDROP_S_DROP } else { DRAGDROP_S_CANCEL }
    }

    fn GiveFeedback(&self, effect: DROPEFFECT) -> HRESULT {
        self.0.effect.set(effect);
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

fn over_own_window() -> bool {
    unsafe {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return false;
        }
        // The WebView2 child windows belong to the browser process: look at the top-level window
        let root = GetAncestor(WindowFromPoint(point), GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(root, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

/// Extracts on a worker thread while this (UI) thread keeps handling messages,
/// so the windows repaint and the progress reaches the page.
fn extract_with_pump(app: &AppHandle, id: u32, req: &DragRequest, dest: &Path) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let progress = state.jobs.register(id);

    let worker = {
        let progress = progress.clone();
        let archive = req.archive.clone();
        let password = req.password.clone();
        let items = req.items.iter().cloned().collect();
        let base = req.base.clone();
        let dest = dest.to_path_buf();
        let expected_files = req.expected_files;
        std::thread::spawn(move || {
            let opts = ExtractOptions {
                dest: &dest,
                password: password.as_deref(),
                items: Some(items),
                base,
                // Over the placeholders
                conflict: Conflict::Overwrite,
                expected_files,
            };
            archive::extract(&archive, &opts, &progress).map_err(|e| e.to_string())
        })
    };

    let mut last = Instant::now() - Duration::from_secs(1);
    while !worker.is_finished() {
        unsafe {
            // Everything but input: the mouse and keys stay queued for the drag loop. Handing them
            // out here would let OLE re-enter QueryContinueDrag (cancelling the drop) and give the
            // page stale button states.
            MsgWaitForMultipleObjects(None, false, 50, QS_POSTMESSAGE | QS_SENDMESSAGE | QS_PAINT | QS_TIMER);
            let mut msg = MSG::default();
            let filter = PEEK_MESSAGE_REMOVE_TYPE(PM_REMOVE.0 | PM_QS_POSTMESSAGE.0 | PM_QS_PAINT.0 | PM_QS_SENDMESSAGE.0);
            while PeekMessageW(&mut msg, None, 0, 0, filter).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if GetAsyncKeyState(VK_ESCAPE.0 as i32) < 0 {
                progress.cancel();
            }
        }
        // From this thread events go straight to the page (other threads' wait for the drag to end)
        if last.elapsed() >= Duration::from_millis(150) {
            last = Instant::now();
            emit_progress(app, id, &progress);
        }
    }
    let result = worker.join().unwrap_or_else(|_| Err("drag_failed".into()));
    state.jobs.unregister(id);
    result
}

fn emit_progress(app: &AppHandle, id: u32, progress: &Arc<Progress>) {
    let (done, total) = progress.snapshot();
    let current = progress.current.lock().unwrap().clone();
    let _ = app.emit("job-progress", vec![crate::jobs::ProgressEvent { id, done, total, current }]);
}
