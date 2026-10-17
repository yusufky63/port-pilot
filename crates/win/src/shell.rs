//! Opening URLs and folders with the user's default handlers.

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

/// `ShellExecuteW(open)`: the default browser for `http://` URLs, Explorer for folders.
/// Callers validate `target` first; this function opens whatever it is given.
pub fn open(target: &str) -> Result<(), String> {
    let file = HSTRING::from(target);
    let result = unsafe { ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL) };
    // Values above 32 mean success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "could not open {target} (code {})",
            result.0 as usize
        ))
    }
}
