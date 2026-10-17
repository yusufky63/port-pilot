/// NUL-terminated UTF-16 for Win32 calls.
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 up to the first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        assert_eq!(from_wide(&to_wide("node.exe")), "node.exe");
        assert_eq!(from_wide(&[0x61, 0, 0x62]), "a");
        assert_eq!(from_wide(&[0x61, 0x62]), "ab");
    }
}
