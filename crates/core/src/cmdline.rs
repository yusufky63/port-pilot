//! Command-line splitting the way the Microsoft C runtime does it (`CommandLineToArgvW` rules),
//! plus a raw splitter that keeps each token's original text for redaction.

/// Splits a Windows command line into arguments.
///
/// Rules: whitespace separates arguments outside quotes; `2n` backslashes before a quote become
/// `n` backslashes and the quote toggles quoting; `2n+1` backslashes become `n` backslashes and
/// a literal quote; backslashes not followed by a quote are literal; `""` inside quotes is a
/// literal quote.
pub fn split_windows(cmd: &str) -> Vec<String> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' if !in_quotes => {
                if has_token {
                    args.push(std::mem::take(&mut cur));
                    has_token = false;
                }
                i += 1;
            }
            '\\' => {
                let mut n = 0;
                while i < chars.len() && chars[i] == '\\' {
                    n += 1;
                    i += 1;
                }
                has_token = true;
                if i < chars.len() && chars[i] == '"' {
                    cur.extend(std::iter::repeat_n('\\', n / 2));
                    if n % 2 == 1 {
                        cur.push('"');
                        i += 1;
                    }
                } else {
                    cur.extend(std::iter::repeat_n('\\', n));
                }
            }
            '"' => {
                has_token = true;
                if in_quotes && chars.get(i + 1) == Some(&'"') {
                    cur.push('"');
                    i += 2;
                } else {
                    in_quotes = !in_quotes;
                    i += 1;
                }
            }
            _ => {
                has_token = true;
                cur.push(c);
                i += 1;
            }
        }
    }
    if has_token {
        args.push(cur);
    }
    args
}

/// Byte ranges of whitespace-separated tokens, quotes respected, original text untouched.
pub fn raw_spans(cmd: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut in_quotes = false;
    let mut backslashes = 0usize;
    for (i, c) in cmd.char_indices() {
        match c {
            ' ' | '\t' if !in_quotes => {
                if let Some(s) = start.take() {
                    spans.push((s, i));
                }
                backslashes = 0;
                continue;
            }
            '"' if backslashes.is_multiple_of(2) => in_quotes = !in_quotes,
            _ => {}
        }
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
        if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        spans.push((s, cmd.len()));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn plain_and_quoted() {
        assert_eq!(
            split_windows(
                r#""C:\Program Files\nodejs\node.exe" C:\src\shop\node_modules\vite\bin\vite.js --port 5173"#
            ),
            s(&[
                r"C:\Program Files\nodejs\node.exe",
                r"C:\src\shop\node_modules\vite\bin\vite.js",
                "--port",
                "5173"
            ])
        );
        assert_eq!(split_windows("  a   b\t c  "), s(&["a", "b", "c"]));
        assert_eq!(split_windows(""), Vec::<String>::new());
        assert_eq!(split_windows(r#""""#), s(&[""]));
    }

    #[test]
    fn backslash_rules() {
        assert_eq!(split_windows(r#"a\\\"b"#), s(&[r#"a\"b"#]));
        assert_eq!(split_windows(r#""a\\" b"#), s(&[r"a\", "b"]));
        assert_eq!(split_windows(r"a\\b"), s(&[r"a\\b"]));
        assert_eq!(split_windows(r#""C:\dir\" next"#), s(&[r#"C:\dir" next"#]));
        assert_eq!(split_windows(r#""say ""hi""""#), s(&[r#"say "hi""#]));
    }

    #[test]
    fn spans_keep_original_text() {
        let cmd = r#"node "C:\my app\server.js" --token=abc"#;
        let spans = raw_spans(cmd);
        let parts: Vec<&str> = spans.iter().map(|&(a, b)| &cmd[a..b]).collect();
        assert_eq!(
            parts,
            vec!["node", r#""C:\my app\server.js""#, "--token=abc"]
        );
    }
}
