//! Listening TCP sockets and bound UDP sockets with their owning PIDs, from the IP Helper
//! extended tables (`GetExtendedTcpTable` / `GetExtendedUdpTable`). Both IPv4 and IPv6.

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use portpilot_core::Protocol;
use portpilot_core::assemble::HostSocket;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;

/// Calls a size-probing table function until the buffer is large enough. The table can grow
/// between calls, so it retries a few times. The buffer is `u32`-aligned for the row structs.
fn read_table(call: impl Fn(Option<*mut c_void>, *mut u32) -> u32) -> Result<Vec<u32>, String> {
    let mut size: u32 = 0;
    let _ = call(None, &mut size);
    for _ in 0..5 {
        let mut buf = vec![0u32; (size as usize).div_ceil(4) + 64];
        size = (buf.len() * 4) as u32;
        let rc = call(Some(buf.as_mut_ptr().cast()), &mut size);
        if rc == NO_ERROR.0 {
            return Ok(buf);
        }
        if rc != ERROR_INSUFFICIENT_BUFFER.0 {
            return Err(format!("IP Helper table call failed with error {rc}"));
        }
    }
    Err("the socket table kept growing while it was read".into())
}

/// Rows of a `MIB_*TABLE_OWNER_PID` buffer: a `u32` count followed by the rows.
fn rows<T: Copy>(buf: &[u32]) -> Vec<T> {
    let count = buf.first().copied().unwrap_or(0) as usize;
    // Every row type here is 4-byte aligned, so the rows start right after the count.
    let available = (buf.len() * 4 - 4) / size_of::<T>();
    let count = count.min(available);
    let first = unsafe { buf.as_ptr().add(1).cast::<T>() };
    (0..count)
        .map(|i| unsafe { std::ptr::read_unaligned(first.add(i)) })
        .collect()
}

fn port(raw: u32) -> u16 {
    u16::from_be((raw & 0xFFFF) as u16)
}

fn v4(raw: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes()))
}

fn tcp_family(af: u32) -> Result<Vec<HostSocket>, String> {
    let buf = read_table(|ptr, size| unsafe {
        GetExtendedTcpTable(ptr, size, false, af, TCP_TABLE_OWNER_PID_LISTENER, 0)
    })?;
    Ok(if af == AF_INET {
        rows::<MIB_TCPROW_OWNER_PID>(&buf)
            .into_iter()
            .map(|r| HostSocket {
                protocol: Protocol::Tcp,
                address: v4(r.dwLocalAddr),
                port: port(r.dwLocalPort),
                pid: r.dwOwningPid,
            })
            .collect()
    } else {
        rows::<MIB_TCP6ROW_OWNER_PID>(&buf)
            .into_iter()
            .map(|r| HostSocket {
                protocol: Protocol::Tcp,
                address: IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                port: port(r.dwLocalPort),
                pid: r.dwOwningPid,
            })
            .collect()
    })
}

fn udp_family(af: u32) -> Result<Vec<HostSocket>, String> {
    let buf = read_table(|ptr, size| unsafe {
        GetExtendedUdpTable(ptr, size, false, af, UDP_TABLE_OWNER_PID, 0)
    })?;
    Ok(if af == AF_INET {
        rows::<MIB_UDPROW_OWNER_PID>(&buf)
            .into_iter()
            .map(|r| HostSocket {
                protocol: Protocol::Udp,
                address: v4(r.dwLocalAddr),
                port: port(r.dwLocalPort),
                pid: r.dwOwningPid,
            })
            .collect()
    } else {
        rows::<MIB_UDP6ROW_OWNER_PID>(&buf)
            .into_iter()
            .map(|r| HostSocket {
                protocol: Protocol::Udp,
                address: IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
                port: port(r.dwLocalPort),
                pid: r.dwOwningPid,
            })
            .collect()
    })
}

/// Every listening TCP socket (IPv4 and IPv6) with its owning PID.
pub fn tcp_listeners() -> Result<Vec<HostSocket>, String> {
    let mut out = tcp_family(AF_INET)?;
    out.extend(tcp_family(AF_INET6)?);
    Ok(out)
}

/// Every bound UDP socket (IPv4 and IPv6) with its owning PID.
pub fn udp_endpoints() -> Result<Vec<HostSocket>, String> {
    let mut out = udp_family(AF_INET)?;
    out.extend(udp_family(AF_INET6)?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, UdpSocket};

    #[test]
    fn finds_own_tcp_listener_on_both_stacks() {
        let v4 = TcpListener::bind("127.0.0.1:0").unwrap();
        let v6 = TcpListener::bind("[::1]:0").unwrap();
        let me = std::process::id();
        let all = tcp_listeners().unwrap();
        let p4 = v4.local_addr().unwrap().port();
        let p6 = v6.local_addr().unwrap().port();
        assert!(
            all.iter().any(|s| s.port == p4
                && s.pid == me
                && s.address == IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
        assert!(
            all.iter().any(|s| s.port == p6
                && s.pid == me
                && s.address == IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
    }

    #[test]
    fn finds_own_udp_socket() {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = sock.local_addr().unwrap().port();
        let me = std::process::id();
        assert!(
            udp_endpoints()
                .unwrap()
                .iter()
                .any(|s| s.port == port && s.pid == me)
        );
    }

    #[test]
    fn port_byte_order() {
        // 3000 = 0x0BB8, stored in network order in the low word.
        assert_eq!(port(u32::from(u16::from_ne_bytes([0x0B, 0xB8]))), 3000);
    }
}
