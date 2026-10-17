//! Command lines through WMI (`Win32_Process`), the public way to read another process's
//! arguments. Reading the PEB through `NtQueryInformationProcess` would be faster, but that
//! interface is documented as internal and subject to change.
//!
//! Callers run this on a worker thread: the first query in a thread initializes COM as
//! multithreaded.

use std::collections::HashMap;

use portpilot_core::time::parse_cim_datetime;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    CoSetProxyBlanket, CoTaskMemFree, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{VARIANT, VariantClear, VariantToStringAlloc};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE, WbemLocator,
};
use windows::core::{BSTR, HSTRING, PCWSTR};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WmiProcess {
    pub pid: u32,
    /// `None` when the process belongs to another user or is protected.
    pub command_line: Option<String>,
    pub created: Option<u64>,
    pub path: Option<String>,
}

struct ComScope {
    initialized: bool,
}

impl ComScope {
    fn enter() -> Result<ComScope, String> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            // The thread already runs an apartment of the other kind; WMI works in either.
            return Ok(ComScope { initialized: false });
        }
        hr.ok()
            .map_err(|e| format!("COM initialization failed: {e}"))?;
        Ok(ComScope { initialized: true })
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { CoUninitialize() };
        }
    }
}

fn connect() -> windows::core::Result<IWbemServices> {
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)? };
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from(r"ROOT\CIMV2"),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )?
    };
    // RPC_C_AUTHN_WINNT and RPC_C_AUTHZ_NONE.
    unsafe {
        CoSetProxyBlanket(
            &services,
            10,
            0,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?
    };
    Ok(services)
}

fn property(obj: &IWbemClassObject, name: &str) -> Option<String> {
    let name = HSTRING::from(name);
    let mut value = VARIANT::default();
    unsafe { obj.Get(PCWSTR(name.as_ptr()), 0, &mut value, None, None) }.ok()?;
    let text = unsafe { VariantToStringAlloc(&value) }.ok().map(|p| {
        let s = unsafe { p.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
        s
    });
    let _ = unsafe { VariantClear(&mut value) };
    text.filter(|s| !s.is_empty())
}

fn query(
    services: &IWbemServices,
    wql: &str,
    out: &mut HashMap<u32, WmiProcess>,
) -> windows::core::Result<()> {
    let rows = unsafe {
        services.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(wql),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )?
    };
    loop {
        let mut objs = [None];
        let mut returned = 0u32;
        unsafe { rows.Next(WBEM_INFINITE, &mut objs, &mut returned) }.ok()?;
        let Some(obj) = objs[0].take().filter(|_| returned > 0) else {
            break;
        };
        let Some(pid) = property(&obj, "ProcessId").and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        out.insert(
            pid,
            WmiProcess {
                pid,
                command_line: property(&obj, "CommandLine"),
                created: property(&obj, "CreationDate").and_then(|d| parse_cim_datetime(&d)),
                path: property(&obj, "ExecutablePath"),
            },
        );
    }
    Ok(())
}

/// Command line, creation time and path for the given PIDs. PIDs that exited in the meantime
/// are simply missing from the result.
pub fn processes(pids: &[u32]) -> Result<HashMap<u32, WmiProcess>, String> {
    let mut out = HashMap::new();
    if pids.is_empty() {
        return Ok(out);
    }
    let _com = ComScope::enter()?;
    let services = connect().map_err(|e| format!("WMI connection failed: {e}"))?;
    // Short WHERE clauses keep each query cheap; WMI has no IN operator.
    for chunk in pids.chunks(48) {
        let filter: Vec<String> = chunk.iter().map(|p| format!("ProcessId = {p}")).collect();
        let wql = format!(
            "SELECT ProcessId, CommandLine, CreationDate, ExecutablePath FROM Win32_Process WHERE {}",
            filter.join(" OR ")
        );
        query(&services, &wql, &mut out).map_err(|e| format!("WMI query failed: {e}"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_command_line() {
        let me = std::process::id();
        let map = processes(&[me]).unwrap();
        let p = map.get(&me).expect("own process");
        let cmd = p
            .command_line
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(
            cmd.contains("portpilot_win"),
            "unexpected command line: {cmd}"
        );
        assert!(p.created.is_some());
    }

    #[test]
    fn empty_input_needs_no_com() {
        assert!(processes(&[]).unwrap().is_empty());
    }
}
