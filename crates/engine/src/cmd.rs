//! Running helper tools (`git`, `wsl.exe`, `netsh`) without a console window and with a time
//! limit. A hung tool must never hang a refresh.

use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

pub struct Spec<'a> {
    pub program: &'a str,
    pub args: &'a [&'a str],
    pub stdin: Option<&'a [u8]>,
    pub env: &'a [(&'a str, &'a str)],
    pub timeout: Duration,
}

impl<'a> Spec<'a> {
    pub fn new(program: &'a str, args: &'a [&'a str], timeout: Duration) -> Self {
        Spec {
            program,
            args,
            stdin: None,
            env: &[],
            timeout,
        }
    }
}

/// Runs a program and collects its output. Returns an error when it cannot start or does not
/// finish within the timeout (it is killed then).
pub fn run(spec: &Spec) -> Result<Output, String> {
    let mut cmd = Command::new(spec.program);
    cmd.args(spec.args)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    for (k, v) in spec.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", spec.program))?;
    if let (Some(input), Some(mut pipe)) = (spec.stdin, child.stdin.take()) {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
    let mut out_pipe = child.stdout.take().expect("stdout is piped");
    let mut err_pipe = child.stderr.take().expect("stderr is piped");
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + spec.timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not finish in {} s",
                    spec.program,
                    spec.timeout.as_secs()
                ));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(Output {
        code: status.code(),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_exit_code() {
        let out = run(&Spec::new(
            "cmd.exe",
            &["/d", "/c", "echo hello& exit 3"],
            Duration::from_secs(10),
        ))
        .unwrap();
        assert_eq!(out.code, Some(3));
        assert!(out.stdout_text().contains("hello"));
    }

    #[test]
    fn feeds_stdin() {
        let spec = Spec {
            stdin: Some(b"piped\r\n"),
            ..Spec::new("findstr.exe", &["piped"], Duration::from_secs(10))
        };
        assert!(run(&spec).unwrap().stdout_text().contains("piped"));
    }

    #[test]
    fn kills_on_timeout() {
        let started = Instant::now();
        let err = run(&Spec::new(
            "ping.exe",
            &["-n", "30", "127.0.0.1"],
            Duration::from_millis(300),
        ))
        .unwrap_err();
        assert!(err.contains("did not finish"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn missing_program() {
        assert!(
            run(&Spec::new(
                "definitely-not-a-program-pp",
                &[],
                Duration::from_secs(1)
            ))
            .is_err()
        );
    }
}
