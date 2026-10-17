//! WSL: decoding `wsl.exe` output, the probe script that runs inside a distro, and its parser.
//!
//! The probe is piped to `sh -s` over stdin, so nothing is quoted through the Windows command
//! line. It runs only in distros that are already running; PortPilot never boots a distro.

use std::collections::HashMap;
use std::net::IpAddr;

use crate::git::{RevParse, Status, parse_rev_parse, parse_status_v2};
use crate::model::Protocol;
use crate::net::split_host_port;

/// Docker Desktop's own distros: their sockets are Docker's, reported by the Docker adapter.
pub const INTERNAL_DISTROS: &[&str] = &["docker-desktop", "docker-desktop-data"];

/// `wsl.exe` writes UTF-16LE unless `WSL_UTF8=1` is honoured; accept both.
pub fn decode_output(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
    let odd_zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    if bytes.len() >= 2 && odd_zeros * 2 >= bytes.len() / 2 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units).replace('\0', "");
    }
    String::from_utf8_lossy(bytes).replace('\0', "")
}

fn valid_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Names from `wsl.exe --list --running --quiet`. Messages such as "There are no running
/// distributions." contain spaces and are dropped, whatever their language.
pub fn parse_distro_names(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .filter(|l| valid_distro_name(l))
        .filter(|l| !INTERNAL_DISTROS.iter().any(|d| d.eq_ignore_ascii_case(l)))
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsSocket {
    pub protocol: Protocol,
    pub address: IpAddr,
    pub port: u16,
    pub process: Option<String>,
    pub pid: Option<u32>,
}

/// One line of `ss -Hltnup`: `tcp LISTEN 0 511 0.0.0.0:3000 0.0.0.0:* users:(("node",pid=841,fd=20))`.
pub fn parse_ss_line(line: &str) -> Option<SsSocket> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let protocol = match *tokens.first()? {
        "tcp" => Protocol::Tcp,
        "udp" => Protocol::Udp,
        _ => return None,
    };
    let state = *tokens.get(1)?;
    if protocol == Protocol::Tcp && state != "LISTEN" {
        return None;
    }
    let (address, port) = split_host_port(tokens.get(4)?)?;
    let rest = tokens.get(6..).map(|t| t.join(" ")).unwrap_or_default();
    let (mut process, mut pid) = (None, None);
    if let Some(users) = rest.split_once("users:((").map(|(_, u)| u) {
        process = users
            .strip_prefix('"')
            .and_then(|u| u.split_once('"'))
            .map(|(name, _)| name.to_string());
        pid = users
            .split_once("pid=")
            .and_then(|(_, p)| p.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|p| p.parse().ok());
    }
    Some(SsSocket {
        protocol,
        address,
        port,
        process,
        pid,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinuxProc {
    pub cwd: Option<String>,
    /// Field 22 of `/proc/<pid>/stat`: start time in clock ticks after boot.
    pub start: Option<u64>,
    pub cmdline: Option<String>,
    pub rev: Option<RevParse>,
    pub status: Option<Status>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Probe {
    pub sockets: Vec<SsSocket>,
    pub procs: HashMap<u32, LinuxProc>,
    /// `ss` was missing in the distro.
    pub no_ss: bool,
}

/// The script piped to `sh -s`. It prints the listening sockets, then for each owning PID its
/// working directory, start time, command line and Git state.
pub const PROBE_SCRIPT: &str = r#"export LC_ALL=C
if ! command -v ss >/dev/null 2>&1; then echo "@@NOSS"; exit 0; fi
T=""; if command -v timeout >/dev/null 2>&1; then T="timeout 3"; fi
echo "@@SS"
ss -Hltnup 2>/dev/null
for p in $(ss -Hltnup 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -un); do
  echo "@@PROC $p"
  c=$(readlink /proc/$p/cwd 2>/dev/null)
  echo "cwd=$c"
  echo "start=$(sed 's/^.*) //' /proc/$p/stat 2>/dev/null | cut -d' ' -f20)"
  echo "cmd=$(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null)"
  if [ -n "$c" ] && [ "$c" != "/" ] && command -v git >/dev/null 2>&1; then
    r=$(git -C "$c" rev-parse --path-format=absolute --show-toplevel --git-dir --git-common-dir 2>/dev/null)
    if [ -n "$r" ]; then
      echo "@@REV"
      echo "$r"
      echo "@@STATUS"
      $T git -C "$c" status --porcelain=v2 --branch --untracked-files=normal 2>/dev/null
    fi
  fi
  echo "@@END"
done
"#;

pub fn parse_probe(text: &str) -> Probe {
    #[derive(PartialEq)]
    enum Section {
        None,
        Ss,
        Proc,
        Rev,
        Status,
    }
    let mut probe = Probe::default();
    let mut section = Section::None;
    let mut cur: Option<(u32, LinuxProc)> = None;
    let mut rev = String::new();
    let mut status = String::new();
    fn finish(
        cur: &mut Option<(u32, LinuxProc)>,
        rev: &mut String,
        status: &mut String,
        probe: &mut Probe,
    ) {
        if let Some((pid, mut p)) = cur.take() {
            if !rev.is_empty() {
                p.rev = parse_rev_parse(rev);
                p.status = Some(parse_status_v2(status));
            }
            probe.procs.insert(pid, p);
        }
        rev.clear();
        status.clear();
    }
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line == "@@NOSS" {
            probe.no_ss = true;
            continue;
        }
        if line == "@@SS" {
            section = Section::Ss;
            continue;
        }
        if let Some(pid) = line.strip_prefix("@@PROC ") {
            finish(&mut cur, &mut rev, &mut status, &mut probe);
            cur = pid.trim().parse().ok().map(|p| (p, LinuxProc::default()));
            section = Section::Proc;
            continue;
        }
        match line {
            "@@REV" => {
                section = Section::Rev;
                continue;
            }
            "@@STATUS" => {
                section = Section::Status;
                continue;
            }
            "@@END" => {
                finish(&mut cur, &mut rev, &mut status, &mut probe);
                section = Section::None;
                continue;
            }
            _ => {}
        }
        match section {
            Section::Ss => {
                if let Some(s) = parse_ss_line(line) {
                    probe.sockets.push(s);
                }
            }
            Section::Proc => {
                let Some((_, p)) = cur.as_mut() else { continue };
                let non_empty = |v: &str| (!v.trim().is_empty()).then(|| v.trim().to_string());
                if let Some(v) = line.strip_prefix("cwd=") {
                    p.cwd = non_empty(v);
                } else if let Some(v) = line.strip_prefix("start=") {
                    p.start = v.trim().parse().ok();
                } else if let Some(v) = line.strip_prefix("cmd=") {
                    p.cmdline = non_empty(v);
                }
            }
            Section::Rev => {
                rev.push_str(line);
                rev.push('\n');
            }
            Section::Status => {
                status.push_str(line);
                status.push('\n');
            }
            Section::None => {}
        }
    }
    finish(&mut cur, &mut rev, &mut status, &mut probe);
    probe
}

/// Signal script for one Linux process. It checks the start time first, so a reused PID is
/// never signalled. Prints `gone`, `changed`, `denied`, `exited` or `alive`.
pub fn signal_script(pid: u32, start: u64, signal: &str, wait_tenths: u32) -> String {
    let signal = match signal {
        "KILL" => "KILL",
        _ => "TERM",
    };
    format!(
        r#"s=$(sed 's/^.*) //' /proc/{pid}/stat 2>/dev/null | cut -d' ' -f20)
if [ -z "$s" ]; then echo gone; exit 0; fi
if [ "$s" != "{start}" ]; then echo changed; exit 0; fi
kill -{signal} {pid} 2>/dev/null || {{ echo denied; exit 0; }}
i=0
while [ $i -lt {wait_tenths} ]; do
  [ -e /proc/{pid} ] || {{ echo exited; exit 0; }}
  sleep 0.1
  i=$((i+1))
done
[ -e /proc/{pid} ] && echo alive || echo exited
"#
    )
}

/// `\\wsl.localhost\Ubuntu\home\a\shop` for `/home/a/shop`.
pub fn unc_path(distro: &str, linux_path: &str) -> String {
    format!(r"\\wsl.localhost\{distro}{}", linux_path.replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_utf8() {
        let utf16: Vec<u8> = "Ubuntu\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(decode_output(&utf16), "Ubuntu\r\n");
        let mut bom = vec![0xFF, 0xFE];
        bom.extend(&utf16);
        assert_eq!(decode_output(&bom), "Ubuntu\r\n");
        assert_eq!(decode_output(b"Debian\n"), "Debian\n");
    }

    #[test]
    fn distro_names_skip_messages_and_docker() {
        let text = "Ubuntu-24.04\r\ndocker-desktop\r\n\r\nkali-linux\r\n";
        assert_eq!(
            parse_distro_names(text),
            vec!["Ubuntu-24.04".to_string(), "kali-linux".to_string()]
        );
        assert!(parse_distro_names("There are no running distributions.\r\n").is_empty());
        assert!(parse_distro_names("Çalışan dağıtım yok.\r\n").is_empty());
    }

    #[test]
    fn ss_lines() {
        let s = parse_ss_line(r#"tcp   LISTEN 0      511          0.0.0.0:3000      0.0.0.0:*    users:(("node",pid=841,fd=20))"#).unwrap();
        assert_eq!(
            (s.protocol, s.port, s.pid, s.process.as_deref()),
            (Protocol::Tcp, 3000, Some(841), Some("node"))
        );
        assert_eq!(s.address.to_string(), "0.0.0.0");
        let v6 = parse_ss_line("tcp LISTEN 0 4096 [::]:22 [::]:*").unwrap();
        assert_eq!((v6.port, v6.pid), (22, None));
        let udp = parse_ss_line(r#"udp UNCONN 0 0 127.0.0.53%lo:53 0.0.0.0:* users:(("systemd-resolve",pid=120,fd=13))"#).unwrap();
        assert_eq!(
            (udp.protocol, udp.port, udp.pid),
            (Protocol::Udp, 53, Some(120))
        );
        assert_eq!(
            parse_ss_line("tcp ESTAB 0 0 10.0.0.2:5000 10.0.0.3:6000"),
            None
        );
        assert_eq!(parse_ss_line("Netid State Recv-Q"), None);
    }

    #[test]
    fn probe_output() {
        let text = "@@SS\n\
tcp LISTEN 0 511 0.0.0.0:3000 0.0.0.0:* users:((\"node\",pid=841,fd=20))\n\
tcp LISTEN 0 4096 127.0.0.1:5432 0.0.0.0:*\n\
@@PROC 841\n\
cwd=/home/a/shop\n\
start=123456\n\
cmd=node /home/a/shop/node_modules/.bin/next dev \n\
@@REV\n\
/home/a/shop\n\
/home/a/shop/.git\n\
/home/a/shop/.git\n\
@@STATUS\n\
# branch.oid 5f32a0d1e2c3\n\
# branch.head main\n\
@@END\n\
@@PROC 900\n\
cwd=\n\
start=99\n\
cmd=\n\
@@END\n";
        let p = parse_probe(text);
        assert_eq!(p.sockets.len(), 2);
        let node = &p.procs[&841];
        assert_eq!(node.cwd.as_deref(), Some("/home/a/shop"));
        assert_eq!(node.start, Some(123456));
        assert_eq!(
            node.cmdline.as_deref(),
            Some("node /home/a/shop/node_modules/.bin/next dev")
        );
        assert_eq!(node.rev.as_ref().unwrap().top_level, "/home/a/shop");
        assert_eq!(
            node.status.as_ref().unwrap().branch.as_deref(),
            Some("main")
        );
        let other = &p.procs[&900];
        assert_eq!((other.cwd.as_ref(), other.rev.as_ref()), (None, None));
        assert!(!p.no_ss);
        assert!(parse_probe("@@NOSS\n").no_ss);
    }

    #[test]
    fn signal_script_only_takes_known_signals() {
        let s = signal_script(841, 123456, "KILL", 30);
        assert!(s.contains("kill -KILL 841"));
        assert!(s.contains("\"123456\""));
        assert!(signal_script(1, 2, "rm -rf /", 1).contains("kill -TERM 1"));
    }

    #[test]
    fn unc_paths() {
        assert_eq!(
            unc_path("Ubuntu", "/home/a/shop"),
            r"\\wsl.localhost\Ubuntu\home\a\shop"
        );
    }
}
