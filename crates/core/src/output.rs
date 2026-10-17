//! The output of a managed run, kept in memory only: lines of stdout and stderr with terminal
//! escape codes removed, capped in lines and bytes so a chatty dev server cannot grow it
//! without bound.

use std::collections::VecDeque;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLine {
    /// Increases by one per line over the whole run, also across dropped lines.
    pub seq: u64,
    pub stream: Stream,
    pub text: String,
}

/// Lines after a sequence number, for a viewer that polls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputChunk {
    pub lines: Vec<OutputLine>,
    /// Pass this as `after` next time.
    pub next: u64,
    /// Lines the viewer has not seen were dropped to stay within the cap.
    pub truncated: bool,
}

const MAX_LINE_CHARS: usize = 2000;
const MAX_PARTIAL_BYTES: usize = 64 * 1024;

/// Removes ANSI escape sequences (colours, cursor moves, window titles) and control characters
/// other than tab.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    // CSI: parameters and intermediates up to a final byte in @..~.
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: up to BEL or ESC \.
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One finished line as a terminal would show it: a carriage return starts the line over, so
/// progress bars keep their last state.
fn clean_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim_end_matches('\r');
    let last = text.rsplit('\r').find(|s| !s.is_empty()).unwrap_or("");
    let mut line = strip_ansi(last);
    if line.chars().count() > MAX_LINE_CHARS {
        line = line.chars().take(MAX_LINE_CHARS).collect();
        line.push('…');
    }
    line
}

#[derive(Debug)]
pub struct OutputLog {
    lines: VecDeque<OutputLine>,
    bytes: usize,
    next_seq: u64,
    partial: [Vec<u8>; 2],
    max_lines: usize,
    max_bytes: usize,
}

impl Default for OutputLog {
    fn default() -> Self {
        OutputLog::new(5000, 1024 * 1024)
    }
}

impl OutputLog {
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        OutputLog {
            lines: VecDeque::new(),
            bytes: 0,
            next_seq: 0,
            partial: [Vec::new(), Vec::new()],
            max_lines: max_lines.max(1),
            max_bytes: max_bytes.max(1),
        }
    }

    fn slot(stream: Stream) -> usize {
        match stream {
            Stream::Stdout => 0,
            Stream::Stderr => 1,
        }
    }

    fn push_line(&mut self, stream: Stream, raw: &[u8]) {
        let text = clean_line(raw);
        self.bytes += text.len();
        self.lines.push_back(OutputLine {
            seq: self.next_seq,
            stream,
            text,
        });
        self.next_seq += 1;
        while self.lines.len() > self.max_lines || self.bytes > self.max_bytes {
            match self.lines.pop_front() {
                Some(old) => self.bytes -= old.text.len(),
                None => break,
            }
        }
    }

    /// Appends bytes as they arrive from a pipe. Lines end at `\n`.
    pub fn push(&mut self, stream: Stream, data: &[u8]) {
        let slot = Self::slot(stream);
        let mut buf = std::mem::take(&mut self.partial[slot]);
        buf.extend_from_slice(data);
        let mut start = 0;
        while let Some(i) = buf[start..].iter().position(|b| *b == b'\n') {
            let end = start + i;
            self.push_line(stream, &buf[start..end]);
            start = end + 1;
        }
        let mut rest = buf.split_off(start);
        if rest.len() > MAX_PARTIAL_BYTES {
            self.push_line(stream, &rest);
            rest.clear();
        }
        self.partial[slot] = rest;
    }

    /// The pipe closed: whatever is left is the last line.
    pub fn finish(&mut self, stream: Stream) {
        let slot = Self::slot(stream);
        let rest = std::mem::take(&mut self.partial[slot]);
        if !rest.is_empty() {
            self.push_line(stream, &rest);
        }
    }

    /// Adds a line written by PortPilot itself, such as "CTRL+BREAK sent".
    pub fn note(&mut self, text: &str) {
        self.push_line(Stream::Stderr, text.as_bytes());
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Lines with `seq >= after`, at most `limit` of them.
    pub fn since(&self, after: u64, limit: usize) -> OutputChunk {
        let first = self.lines.front().map_or(self.next_seq, |l| l.seq);
        let lines: Vec<OutputLine> = self
            .lines
            .iter()
            .skip_while(|l| l.seq < after)
            .take(limit)
            .cloned()
            .collect();
        let next = lines.last().map_or(after.max(first), |l| l.seq + 1);
        OutputChunk {
            truncated: after < first,
            lines,
            next,
        }
    }

    /// Every line, for copying.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(&l.text);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escape_codes() {
        assert_eq!(
            strip_ansi("\u{1b}[32m  VITE\u{1b}[39m ready"),
            "  VITE ready"
        );
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(strip_ansi("\u{1b}]8;;http://x\u{1b}\\link"), "link");
        assert_eq!(strip_ansi("a\u{1b}Mb\tc\u{8}"), "ab\tc");
        assert_eq!(strip_ansi("trailing\u{1b}"), "trailing");
    }

    #[test]
    fn splits_lines_across_chunks_and_streams() {
        let mut log = OutputLog::default();
        log.push(Stream::Stdout, b"hel");
        log.push(Stream::Stderr, b"oops\r\n");
        log.push(Stream::Stdout, b"lo\nwor");
        assert_eq!(log.len(), 2);
        log.finish(Stream::Stdout);
        let all = log.since(0, 100);
        let texts: Vec<_> = all
            .lines
            .iter()
            .map(|l| (l.stream, l.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            vec![
                (Stream::Stderr, "oops"),
                (Stream::Stdout, "hello"),
                (Stream::Stdout, "wor")
            ]
        );
        assert_eq!(all.next, 3);
        assert!(!all.truncated);
        assert!(log.since(3, 100).lines.is_empty());
        assert_eq!(log.since(3, 100).next, 3);
    }

    #[test]
    fn carriage_return_keeps_the_last_state() {
        let mut log = OutputLog::default();
        log.push(Stream::Stdout, b" 10%\r 50%\r100%\r\n");
        assert_eq!(log.since(0, 10).lines[0].text, "100%");
    }

    #[test]
    fn caps_lines_and_reports_truncation() {
        let mut log = OutputLog::new(3, 1024);
        for i in 0..5 {
            log.push(Stream::Stdout, format!("line {i}\n").as_bytes());
        }
        assert_eq!(log.len(), 3);
        let chunk = log.since(0, 100);
        assert!(chunk.truncated);
        assert_eq!(chunk.lines[0].text, "line 2");
        assert_eq!(chunk.next, 5);
        let mut small = OutputLog::new(100, 10);
        small.push(Stream::Stdout, b"12345\n67890\nabc\n");
        assert_eq!(small.text(), "67890\nabc\n");
    }

    #[test]
    fn long_lines_are_cut() {
        let mut log = OutputLog::default();
        log.push(Stream::Stdout, "x".repeat(5000).as_bytes());
        log.finish(Stream::Stdout);
        let text = &log.since(0, 1).lines[0].text;
        assert_eq!(text.chars().count(), MAX_LINE_CHARS + 1);
    }

    #[test]
    fn limit_pages_through() {
        let mut log = OutputLog::default();
        for i in 0..10 {
            log.push(Stream::Stdout, format!("{i}\n").as_bytes());
        }
        let first = log.since(0, 4);
        assert_eq!(first.lines.len(), 4);
        assert_eq!(first.next, 4);
        let second = log.since(first.next, 100);
        assert_eq!(second.lines.first().unwrap().text, "4");
        assert_eq!(second.next, 10);
    }
}
