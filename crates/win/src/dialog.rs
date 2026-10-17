//! The native "choose a folder" dialog. The desktop app opens it from Rust, so a folder reaches
//! the run commands only because the user picked it, never because the WebView named it.

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
    SIGDN_FILESYSPATH,
};
use windows::core::HSTRING;

struct Apartment(bool);

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Shows the folder picker owned by window `owner` (a raw `HWND`, or 0) and returns the chosen
/// folder, or `None` when the user cancelled. Call it from a worker thread: it blocks until
/// the dialog closes.
pub fn pick_folder(owner: isize, title: &str) -> Result<Option<String>, String> {
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let _apartment = if hr == RPC_E_CHANGED_MODE {
        Apartment(false)
    } else {
        hr.ok()
            .map_err(|e| format!("COM initialization failed: {e}"))?;
        Apartment(true)
    };
    let fail = |e: windows::core::Error| format!("the folder dialog failed: {e}");
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(fail)?;
        let options = dialog.GetOptions().map_err(fail)?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
            .map_err(fail)?;
        dialog.SetTitle(&HSTRING::from(title)).map_err(fail)?;
        let owner = (owner != 0).then_some(HWND(owner as *mut _));
        match dialog.Show(owner) {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(e) => return Err(fail(e)),
        }
        let item = dialog.GetResult().map_err(fail)?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(fail)?;
        let path = name.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(Some)
    }
}
