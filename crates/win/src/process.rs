//! Process inventory and identity: a ToolHelp snapshot for names and parents, and a limited
//! query handle for the creation time, image path and owner.
//!
//! The creation time is the identity token next to the PID. Windows reuses PIDs, and a parent
//! PID may point at a newer process that took the number (Microsoft documents both), so every
//! chain and every action compares creation times.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, FILETIME, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{
    GetLengthSid, GetTokenInformation, LookupAccountSidW, PSID, SID_NAME_USE, TOKEN_ELEVATION,
    TOKEN_QUERY, TOKEN_USER, TokenElevation, TokenUser,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
    WaitForSingleObject,
};
use windows::core::{Owned, PCWSTR, PWSTR};

use crate::wide::from_wide;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

/// Every process with its parent PID and executable name. Works for all processes, including
/// ones the current user cannot open.
pub fn snapshot() -> Result<HashMap<u32, ProcEntry>, String> {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|e| format!("process snapshot failed: {e}"))?;
    let snap = unsafe { Owned::new(snap) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut out = HashMap::new();
    if unsafe { Process32FirstW(*snap, &mut entry) }.is_err() {
        return Ok(out);
    }
    loop {
        out.insert(
            entry.th32ProcessID,
            ProcEntry {
                pid: entry.th32ProcessID,
                ppid: entry.th32ParentProcessID,
                name: from_wide(&entry.szExeFile),
            },
        );
        if unsafe { Process32NextW(*snap, &mut entry) }.is_err() {
            break;
        }
    }
    Ok(out)
}

fn filetime_u64(ft: FILETIME) -> u64 {
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}

/// Creation time of an open process handle as a FILETIME.
pub fn handle_start_time(h: HANDLE) -> Option<u64> {
    let (mut create, mut exit, mut kernel, mut user) = Default::default();
    unsafe { GetProcessTimes(h, &mut create, &mut exit, &mut kernel, &mut user) }.ok()?;
    let ft = filetime_u64(create);
    (ft != 0).then_some(ft)
}

fn image_path(h: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 32_768];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }
        .ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn token_info(
    h: HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Option<Vec<u64>> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(h, TOKEN_QUERY, &mut token) }.ok()?;
    let token = unsafe { Owned::new(token) };
    let mut needed = 0u32;
    let _ = unsafe { GetTokenInformation(*token, class, None, 0, &mut needed) };
    if needed == 0 {
        return None;
    }
    // u64 storage keeps the SID pointer inside TOKEN_USER aligned.
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    unsafe {
        GetTokenInformation(
            *token,
            class,
            Some(buf.as_mut_ptr().cast()),
            (buf.len() * 8) as u32,
            &mut needed,
        )
    }
    .ok()?;
    Some(buf)
}

fn account_cache() -> &'static Mutex<HashMap<Vec<u8>, String>> {
    static CACHE: OnceLock<Mutex<HashMap<Vec<u8>, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn account_name(sid: PSID) -> Option<String> {
    let len = unsafe { GetLengthSid(sid) } as usize;
    let key = unsafe { std::slice::from_raw_parts(sid.0 as *const u8, len) }.to_vec();
    if let Some(name) = account_cache().lock().ok()?.get(&key) {
        return Some(name.clone());
    }
    let mut name = vec![0u16; 256];
    let mut domain = vec![0u16; 256];
    let (mut nlen, mut dlen) = (name.len() as u32, domain.len() as u32);
    let mut use_ = SID_NAME_USE::default();
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(name.as_mut_ptr())),
            &mut nlen,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut dlen,
            &mut use_,
        )
    }
    .ok()?;
    let (n, d) = (from_wide(&name), from_wide(&domain));
    let full = if d.is_empty() { n } else { format!("{d}\\{n}") };
    account_cache().lock().ok()?.insert(key, full.clone());
    Some(full)
}

fn token_user(h: HANDLE) -> Option<String> {
    let buf = token_info(h, TokenUser)?;
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    account_name(user.User.Sid)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detail {
    pub start: Option<u64>,
    pub path: Option<String>,
    pub user: Option<String>,
    /// The process could not be opened even for limited query.
    pub access_denied: bool,
}

/// Opens `pid` with `PROCESS_QUERY_LIMITED_INFORMATION`, the narrowest right that gives the
/// creation time and image path. No debug privilege is used.
pub fn detail(pid: u32) -> Detail {
    if pid == 0 || pid == 4 {
        return Detail {
            access_denied: true,
            ..Default::default()
        };
    }
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(h) => {
            let h = unsafe { Owned::new(h) };
            Detail {
                start: handle_start_time(*h),
                path: image_path(*h),
                user: token_user(*h),
                access_denied: false,
            }
        }
        Err(e) => Detail {
            access_denied: e.code() == ERROR_ACCESS_DENIED.to_hresult(),
            ..Default::default()
        },
    }
}

/// Creation time of a process this program started, read from its own handle: valid even if
/// the process already exited.
pub fn child_start_time(child: &std::process::Child) -> Option<u64> {
    use std::os::windows::io::AsRawHandle;
    handle_start_time(HANDLE(child.as_raw_handle()))
}

/// Creation time of `pid` right now, or `None` if it cannot be opened or has exited.
pub fn start_time(pid: u32) -> Option<u64> {
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let h = unsafe { Owned::new(h) };
    handle_start_time(*h)
}

/// Whether `pid` names a process that has not exited. A process that exited can still be
/// opened while someone holds a handle to it, so this waits on the handle instead.
pub fn is_running(pid: u32) -> bool {
    let Ok(h) = (unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    }) else {
        return false;
    };
    let h = unsafe { Owned::new(h) };
    let event = unsafe { WaitForSingleObject(*h, 0) };
    event != WAIT_OBJECT_0
}

/// `DOMAIN\user` of the current process.
pub fn current_user() -> Option<String> {
    token_user(unsafe { GetCurrentProcess() })
}

/// Whether the current process runs elevated.
pub fn is_elevated() -> bool {
    let Some(buf) = token_info(unsafe { GetCurrentProcess() }, TokenElevation) else {
        return false;
    };
    let elevation = unsafe { &*(buf.as_ptr() as *const TOKEN_ELEVATION) };
    elevation.TokenIsElevated != 0
}

/// The current process followed by its ancestors, as far as creation times stay consistent.
/// PortPilot never offers to stop any of these.
pub fn own_lineage(procs: &HashMap<u32, ProcEntry>) -> Vec<u32> {
    let me = std::process::id();
    let mut out = vec![me];
    let mut child_start = start_time(me);
    let mut cur = procs.get(&me).map(|p| p.ppid);
    while let Some(pid) = cur {
        if pid == 0 || out.contains(&pid) || out.len() > 64 {
            break;
        }
        let start = start_time(pid);
        if let (Some(c), Some(s)) = (child_start, start)
            && s > c
        {
            break;
        }
        out.push(pid);
        child_start = start;
        cur = procs.get(&pid).map(|p| p.ppid);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_itself() {
        let procs = snapshot().unwrap();
        let me = std::process::id();
        let entry = procs.get(&me).expect("own process in snapshot");
        assert!(entry.name.to_ascii_lowercase().ends_with(".exe"));
        let d = detail(me);
        assert!(!d.access_denied);
        assert!(d.start.is_some());
        assert!(d.path.unwrap().to_ascii_lowercase().ends_with(".exe"));
        assert_eq!(d.user, current_user());
        assert_eq!(start_time(me), d.start);
    }

    #[test]
    fn system_process_is_not_opened() {
        assert!(detail(4).access_denied);
        assert_eq!(start_time(0), None);
    }

    #[test]
    fn lineage_starts_with_self() {
        let procs = snapshot().unwrap();
        let line = own_lineage(&procs);
        assert_eq!(line[0], std::process::id());
    }
}
