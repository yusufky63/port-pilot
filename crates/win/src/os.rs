//! Windows version for diagnostics, read from the registry because `GetVersionEx` reports the
//! version the executable is manifested for.

use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::{HSTRING, w};

const KEY: windows::core::PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");

fn string_value(name: &str) -> Option<String> {
    let name = HSTRING::from(name);
    let mut buf = vec![0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            KEY,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    rc.is_ok().then(|| crate::wide::from_wide(&buf))
}

fn dword_value(name: &str) -> Option<u32> {
    let name = HSTRING::from(name);
    let mut value = 0u32;
    let mut len = 4u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            KEY,
            &name,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut len),
        )
    };
    rc.is_ok().then_some(value)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsVersion {
    /// "Windows 11" or "Windows 10".
    pub name: String,
    /// "24H2", when Windows reports one.
    pub display_version: Option<String>,
    pub build: u32,
    pub revision: u32,
}

impl OsVersion {
    pub fn label(&self) -> String {
        let mut s = self.name.clone();
        if let Some(v) = &self.display_version {
            s.push(' ');
            s.push_str(v);
        }
        format!("{s} (build {}.{})", self.build, self.revision)
    }

    /// Windows 10 left general support on 2025-10-14; Port Pilot keeps working there but
    /// treats it as best-effort.
    pub fn is_windows_10(&self) -> bool {
        self.build < 22_000
    }
}

pub fn version() -> Option<OsVersion> {
    let build: u32 = string_value("CurrentBuildNumber")?.trim().parse().ok()?;
    Some(OsVersion {
        // ProductName still says "Windows 10" on Windows 11; the build number decides.
        name: if build >= 22_000 {
            "Windows 11"
        } else {
            "Windows 10"
        }
        .to_string(),
        display_version: string_value("DisplayVersion").filter(|s| !s.is_empty()),
        build,
        revision: dword_value("UBR").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_build() {
        let v = super::version().expect("version");
        assert!(v.build >= 10_240);
        assert!(v.label().contains("build"));
    }
}
