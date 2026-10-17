//! Local time for people. Identity comparisons always use the raw FILETIME.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// `2026-10-17 17:22:03` in the machine's time zone, including its daylight saving rules.
pub fn local_datetime(ft: u64) -> Option<String> {
    let ft = FILETIME {
        dwLowDateTime: ft as u32,
        dwHighDateTime: (ft >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_a_date() {
        let s = super::local_datetime(portpilot_core::time::unix_ms_to_filetime(1_792_247_323_000))
            .unwrap();
        assert!(s.starts_with("2026-10-1"), "{s}");
        assert_eq!(s.len(), 19);
    }
}
