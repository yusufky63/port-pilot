//! Opening URLs and folders with the user's default handlers, and starting a terminal the way
//! the Run dialog would.

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

fn check(result: windows::Win32::Foundation::HINSTANCE, what: &str) -> Result<(), String> {
    // Values above 32 mean success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "could not open {what} (code {})",
            result.0 as usize
        ))
    }
}

/// `ShellExecuteW(open)`: the default browser for `http://` URLs, Explorer for folders.
/// Callers validate `target` first; this function opens whatever it is given.
pub fn open(target: &str) -> Result<(), String> {
    let file = HSTRING::from(target);
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL) };
    check(result, target)
}

/// Starts `program` with `params` in `dir`. A console program gets a console window of its
/// own, and `dir` becomes its working directory without being quoted into a command line.
/// Nothing of this process's standard handles reaches the new program.
pub fn start_in(program: &str, params: &str, dir: Option<&str>) -> Result<(), String> {
    let file = HSTRING::from(program);
    let params = HSTRING::from(params);
    let dir = dir.map(HSTRING::from);
    let dir_ptr = dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr()));
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, &params, dir_ptr, SW_SHOWNORMAL) };
    check(result, program)
}
