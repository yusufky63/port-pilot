//! Addresses, exposure, URLs, Windows excluded port ranges and free-port selection.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

use crate::model::{Endpoint, Exposure, Protocol};

/// Treats IPv4-mapped IPv6 (`::ffff:127.0.0.1`) as the IPv4 address it carries.
pub fn canonical(addr: IpAddr) -> IpAddr {
    match addr {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

pub fn classify(addr: IpAddr) -> Exposure {
    let addr = canonical(addr);
    if addr.is_loopback() {
        Exposure::Loopback
    } else if addr.is_unspecified() {
        Exposure::All
    } else {
        Exposure::Lan
    }
}

pub fn endpoint(addr: IpAddr, port: u16) -> Endpoint {
    let addr = canonical(addr);
    Endpoint {
        address: addr.to_string(),
        port,
        exposure: classify(addr),
    }
}

/// Parses an address as tools print it: `0.0.0.0`, `[::]`, `*`, `127.0.0.53%lo`, `[fe80::1%eth0]`.
pub fn parse_address(text: &str) -> Option<IpAddr> {
    let t = text.trim().trim_start_matches('[').trim_end_matches(']');
    if t == "*" {
        return Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    }
    let t = t.split('%').next().unwrap_or(t);
    t.parse::<IpAddr>().ok().map(canonical)
}

/// Splits `host:port` where host may be bracketed IPv6 or carry a zone (`%lo`).
pub fn split_host_port(text: &str) -> Option<(IpAddr, u16)> {
    let (host, port) = text.trim().rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    let ip = if host.is_empty() {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    } else {
        parse_address(host)?
    };
    Some((ip, port))
}

pub fn widest(endpoints: &[Endpoint]) -> Exposure {
    endpoints
        .iter()
        .map(|e| e.exposure)
        .max()
        .unwrap_or(Exposure::Loopback)
}

/// A browser URL for a TCP listener. Loopback and wildcard binds open as `localhost`; a listener
/// bound to one LAN address opens on that address.
pub fn url_for(protocol: Protocol, port: u16, endpoints: &[Endpoint]) -> Option<String> {
    if protocol != Protocol::Tcp || endpoints.is_empty() {
        return None;
    }
    if endpoints
        .iter()
        .any(|e| matches!(e.exposure, Exposure::Loopback | Exposure::All))
    {
        return Some(format!("http://localhost:{port}"));
    }
    let e = &endpoints[0];
    let host = match e.address.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{}]", e.address),
        _ => e.address.clone(),
    };
    Some(format!("http://{host}:{port}"))
}

/// An inclusive port range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub fn contains(&self, port: u16) -> bool {
        (self.start..=self.end).contains(&port)
    }
}

/// `3000-3999` or a single port.
pub fn parse_port_range(text: &str) -> Result<PortRange, String> {
    let text = text.trim();
    let (a, b) = text.split_once('-').unwrap_or((text, text));
    let start = a
        .trim()
        .parse::<u16>()
        .map_err(|_| format!("not a port: {a}"))?;
    let end = b
        .trim()
        .parse::<u16>()
        .map_err(|_| format!("not a port: {b}"))?;
    if start == 0 || end < start {
        return Err(format!("invalid range: {text}"));
    }
    Ok(PortRange { start, end })
}

/// Reads `netsh interface ipv4 show excludedportrange protocol=tcp`. The headers are localized,
/// so only rows of two numbers count; a trailing `*` marks administered exclusions.
pub fn parse_excluded_ranges(text: &str) -> Vec<PortRange> {
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 || parts.len() > 3 {
            continue;
        }
        if parts.len() == 3 && parts[2] != "*" {
            continue;
        }
        if let (Ok(a), Ok(b)) = (parts[0].parse::<u16>(), parts[1].parse::<u16>())
            && a <= b
        {
            out.push(PortRange { start: a, end: b });
        }
    }
    out
}

pub fn excluded_range_for(port: u16, ranges: &[PortRange]) -> Option<PortRange> {
    ranges.iter().copied().find(|r| r.contains(port))
}

/// Ports in `range` that no listener uses and Windows has not reserved, in order. The caller
/// still binds each candidate before trusting it.
pub fn free_candidates(
    range: PortRange,
    used: &HashSet<u16>,
    excluded: &[PortRange],
    limit: usize,
) -> Vec<u16> {
    (range.start..=range.end)
        .filter(|p| !used.contains(p) && excluded_range_for(*p, excluded).is_none())
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn exposure_classes() {
        assert_eq!(classify(ip("127.0.0.1")), Exposure::Loopback);
        assert_eq!(classify(ip("127.0.1.1")), Exposure::Loopback);
        assert_eq!(classify(ip("::1")), Exposure::Loopback);
        assert_eq!(classify(ip("::ffff:127.0.0.1")), Exposure::Loopback);
        assert_eq!(classify(ip("0.0.0.0")), Exposure::All);
        assert_eq!(classify(ip("::")), Exposure::All);
        assert_eq!(classify(ip("192.168.1.50")), Exposure::Lan);
        assert_eq!(classify(ip("fe80::1")), Exposure::Lan);
    }

    #[test]
    fn tool_addresses() {
        assert_eq!(split_host_port("0.0.0.0:3000"), Some((ip("0.0.0.0"), 3000)));
        assert_eq!(split_host_port("[::]:22"), Some((ip("::"), 22)));
        assert_eq!(split_host_port("*:5000"), Some((ip("0.0.0.0"), 5000)));
        assert_eq!(
            split_host_port("127.0.0.53%lo:53"),
            Some((ip("127.0.0.53"), 53))
        );
        assert_eq!(
            split_host_port("[fe80::1%eth0]:80"),
            Some((ip("fe80::1"), 80))
        );
        assert_eq!(
            split_host_port("[::ffff:127.0.0.1]:8080"),
            Some((ip("127.0.0.1"), 8080))
        );
        assert_eq!(split_host_port("nonsense"), None);
    }

    #[test]
    fn urls() {
        let lo = endpoint(ip("127.0.0.1"), 5173);
        let lan = endpoint(ip("192.168.1.5"), 8080);
        let lan6 = endpoint(ip("fe80::5"), 8080);
        assert_eq!(
            url_for(Protocol::Tcp, 5173, &[lo]).as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            url_for(Protocol::Tcp, 8080, std::slice::from_ref(&lan)).as_deref(),
            Some("http://192.168.1.5:8080")
        );
        assert_eq!(
            url_for(Protocol::Tcp, 8080, &[lan6]).as_deref(),
            Some("http://[fe80::5]:8080")
        );
        assert_eq!(url_for(Protocol::Udp, 8080, &[lan]), None);
    }

    #[test]
    fn widest_wins() {
        let eps = vec![endpoint(ip("127.0.0.1"), 1), endpoint(ip("::"), 1)];
        assert_eq!(widest(&eps), Exposure::All);
        assert_eq!(widest(&[]), Exposure::Loopback);
    }

    #[test]
    fn ranges() {
        assert_eq!(
            parse_port_range("3000-3999"),
            Ok(PortRange {
                start: 3000,
                end: 3999
            })
        );
        assert_eq!(
            parse_port_range("8080"),
            Ok(PortRange {
                start: 8080,
                end: 8080
            })
        );
        assert!(parse_port_range("0-10").is_err());
        assert!(parse_port_range("10-5").is_err());
        assert!(parse_port_range("x").is_err());
    }

    #[test]
    fn excluded_ranges_ignore_localized_headers() {
        let text = "\nProtocol tcp Port Exclusion Ranges\n\nStart Port    End Port\n----------    --------\n      1080        1179\n      2869        2869\n     50000       50059     *\n\n* - Administered port exclusions.\n";
        let tr = "\nProtokol tcp Bağlantı Noktası Dışlama Aralıkları\n\nBaşlangıç     Bitiş\n----------    --------\n      5357        5357\n";
        assert_eq!(
            parse_excluded_ranges(text),
            vec![
                PortRange {
                    start: 1080,
                    end: 1179
                },
                PortRange {
                    start: 2869,
                    end: 2869
                },
                PortRange {
                    start: 50000,
                    end: 50059
                }
            ]
        );
        assert_eq!(
            parse_excluded_ranges(tr),
            vec![PortRange {
                start: 5357,
                end: 5357
            }]
        );
    }

    #[test]
    fn free_ports_skip_used_and_excluded() {
        let used: HashSet<u16> = [3000, 3002].into_iter().collect();
        let excluded = vec![PortRange {
            start: 3003,
            end: 3004,
        }];
        assert_eq!(
            free_candidates(
                PortRange {
                    start: 3000,
                    end: 3010
                },
                &used,
                &excluded,
                3
            ),
            vec![3001, 3005, 3006]
        );
    }
}
